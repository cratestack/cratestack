//! A proxy that misbehaves on request: the hop ADR 0006 exists to survive.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use cratestack::axum::Router;
use cratestack::axum::body::{Body, Bytes, to_bytes};
use cratestack::axum::extract::State;
use cratestack::axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST};
use cratestack::axum::http::{HeaderMap, HeaderName, Request, StatusCode};
use cratestack::axum::response::Response;

use super::tamper::Tamper;

#[derive(Clone)]
struct Recorded {
    status: StatusCode,
    content_type: Option<String>,
    /// The `Cratestack-Frames` value of the answer, which the proxy passes
    /// on (it forwards no other header but `Content-Type`).
    frames: Option<String>,
    body: Bytes,
}

/// A request as it reached the proxy.
#[derive(Debug, Clone)]
pub struct Captured {
    pub method: String,
    pub path_and_query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

struct State_ {
    upstream: SocketAddr,
    http: reqwest::Client,
    tamper: Mutex<Tamper>,
    previous: Mutex<Option<Recorded>>,
    request_content_types: Mutex<Vec<Option<String>>>,
    captured: Mutex<Vec<Captured>>,
}

pub struct Proxy {
    pub addr: SocketAddr,
    state: Arc<State_>,
}

impl Proxy {
    pub fn tamper(&self, tamper: Tamper) {
        *self.state.tamper.lock().unwrap() = tamper;
    }

    /// Every request that reached the proxy, in order, bytes and all.
    pub fn captured(&self) -> Vec<Captured> {
        self.state.captured.lock().unwrap().clone()
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
        captured: Mutex::new(Vec::new()),
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
    state.captured.lock().unwrap().push(Captured {
        method: parts.method.to_string(),
        path_and_query: parts
            .uri
            .path_and_query()
            .map_or("/", |p| p.as_str())
            .to_owned(),
        headers: parts
            .headers
            .iter()
            .filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_owned())))
            .collect(),
        body: body.to_vec(),
    });
    state.request_content_types.lock().unwrap().push(
        parts
            .headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    );
    if let Tamper::Redirect(code) = &tamper
        && parts.uri.path() != "/redirected"
    {
        return Response::builder()
            .status(StatusCode::from_u16(*code).expect("a status"))
            .header("location", "/redirected")
            .body(Body::empty())
            .expect("response");
    }
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
        frames: answer
            .headers()
            .get("cratestack-frames")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
        body: answer.bytes().await.expect("upstream body"),
    };
    let previous = state.previous.lock().unwrap().replace(current.clone());
    let sent = match tamper {
        Tamper::ReplayPrevious => previous.expect("a previous response to replay"),
        Tamper::Status(status) => Recorded { status, ..current },
        Tamper::Answer(status, body) => Recorded {
            status,
            content_type: Some("application/cbor".to_owned()),
            frames: None,
            body: Bytes::from(body),
        },
        Tamper::Plain(body) => Recorded {
            status: StatusCode::OK,
            content_type: Some("application/cbor".to_owned()),
            frames: None,
            body: Bytes::from(body),
        },
        Tamper::StripFrames => Recorded {
            frames: None,
            ..current
        },
        Tamper::ContentType(content_type) => Recorded {
            content_type: Some(content_type.to_owned()),
            ..current
        },
        Tamper::Pass | Tamper::DropRequestHeader(_) | Tamper::Redirect(_) => current,
    };
    let mut response = Response::builder().status(sent.status);
    if let Some(content_type) = sent.content_type {
        response = response.header(CONTENT_TYPE, content_type);
    }
    if let Some(frames) = sent.frames {
        response = response.header("cratestack-frames", frames);
    }
    response.body(Body::from(sent.body)).expect("response")
}
