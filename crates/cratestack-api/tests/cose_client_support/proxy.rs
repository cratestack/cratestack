//! A proxy that misbehaves on request: the hop ADR 0006 exists to survive.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use cratestack::axum::Router;
use cratestack::axum::body::{Body, Bytes, to_bytes};
use cratestack::axum::extract::State;
use cratestack::axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST};
use cratestack::axum::http::{HeaderMap, HeaderName, Request, StatusCode};
use cratestack::axum::response::Response;

/// What the proxy does to the traffic passing through it.
#[derive(Debug, Clone)]
pub enum Tamper {
    /// Forward faithfully.
    Pass,
    /// Answer with the response recorded for the *previous* request, sealed
    /// for that one: a replay of a genuine, validly signed answer.
    ReplayPrevious,
    /// Forward faithfully but change the status to this one.
    Status(StatusCode),
    /// Answer `200` with this body as plain `application/cbor`: the seal
    /// stripped (or never applied) and the payload forged.
    Plain(Vec<u8>),
    /// Remove this request header before forwarding.
    DropRequestHeader(&'static str),
}

#[derive(Clone)]
struct Recorded {
    status: StatusCode,
    content_type: Option<String>,
    body: Bytes,
}

struct State_ {
    upstream: SocketAddr,
    http: reqwest::Client,
    tamper: Mutex<Tamper>,
    previous: Mutex<Option<Recorded>>,
    request_content_types: Mutex<Vec<Option<String>>>,
}

pub struct Proxy {
    pub addr: SocketAddr,
    state: Arc<State_>,
}

impl Proxy {
    pub fn tamper(&self, tamper: Tamper) {
        *self.state.tamper.lock().unwrap() = tamper;
    }

    /// The `Content-Type` of every request that reached the proxy, in order.
    pub fn request_content_types(&self) -> Vec<Option<String>> {
        self.state.request_content_types.lock().unwrap().clone()
    }
}

/// A proxy in front of `upstream`.
pub async fn proxy(upstream: SocketAddr) -> Proxy {
    let state = Arc::new(State_ {
        upstream,
        http: reqwest::Client::new(),
        tamper: Mutex::new(Tamper::Pass),
        previous: Mutex::new(None),
        request_content_types: Mutex::new(Vec::new()),
    });
    let router = Router::new().fallback(forward).with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        cratestack::axum::serve(listener, router).await.unwrap();
    });
    Proxy { addr, state }
}

async fn forward(State(state): State<Arc<State_>>, request: Request<Body>) -> Response {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, usize::MAX).await.expect("request body");
    let mut headers: HeaderMap = parts.headers.clone();
    headers.remove(HOST);
    headers.remove(CONTENT_LENGTH);
    let tamper = state.tamper.lock().unwrap().clone();
    if let Tamper::DropRequestHeader(name) = &tamper {
        headers.remove(HeaderName::from_static(name));
    }
    state.request_content_types.lock().unwrap().push(
        parts
            .headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    );
    let url = format!(
        "http://{}{}",
        state.upstream,
        parts.uri.path_and_query().map_or("/", |p| p.as_str())
    );
    let answer = state
        .http
        .request(parts.method, url)
        .headers(headers)
        .body(body)
        .send()
        .await
        .expect("upstream");
    let current = Recorded {
        status: answer.status(),
        content_type: answer
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
        body: answer.bytes().await.expect("upstream body"),
    };
    let previous = state.previous.lock().unwrap().replace(current.clone());
    let sent = match tamper {
        Tamper::ReplayPrevious => previous.expect("a previous response to replay"),
        Tamper::Status(status) => Recorded { status, ..current },
        Tamper::Plain(body) => Recorded {
            status: StatusCode::OK,
            content_type: Some("application/cbor".to_owned()),
            body: Bytes::from(body),
        },
        Tamper::Pass | Tamper::DropRequestHeader(_) => current,
    };
    let mut response = Response::builder().status(sent.status);
    if let Some(content_type) = sent.content_type {
        response = response.header(CONTENT_TYPE, content_type);
    }
    response.body(Body::from(sent.body)).expect("response")
}
