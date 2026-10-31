//! ADR 0021 §3 (cratestack#1243): the provider is handed the canonical
//! request: the declared route as `path`, whatever prefix the router is
//! mounted under, the raw query, and the exact body bytes.
//!
//! No database: the recording provider refuses every request after noting
//! what it saw, so the lazy pool never connects.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use cratestack_core::{AuthProvider, CratestackContext, CratestackError, RequestContext};
use cratestack_sqlx::sqlx::PgPool;
use cratestack_sync::server::{SyncServer, sync_router};
use tower::ServiceExt;

#[derive(Debug, Clone, PartialEq)]
struct Seen {
    method: String,
    path: String,
    query: Option<String>,
    body: Vec<u8>,
}

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<Seen>>>);

impl AuthProvider for Recorder {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        self.0.lock().unwrap().push(Seen {
            method: request.method.to_owned(),
            path: request.path.to_owned(),
            query: request.query.map(str::to_owned),
            body: request.body.to_vec(),
        });
        Err(CratestackError::Unauthorized("recorded".into()))
    }
}

fn app(recorder: &Recorder) -> Router {
    let pool = PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap();
    let server = SyncServer::builder(pool, Default::default(), |_: &CratestackContext| None)
        .build()
        .unwrap();
    Router::new().nest("/api/v1", sync_router(server, recorder.clone()))
}

async fn seen(request: Request<Body>) -> Seen {
    let recorder = Recorder::default();
    let _ = app(&recorder).oneshot(request).await.unwrap();
    let seen = recorder.0.lock().unwrap();
    assert_eq!(seen.len(), 1, "one provider call per request");
    seen[0].clone()
}

#[tokio::test]
async fn a_post_is_the_declared_route_and_the_exact_body() {
    let body = br#"{ "clientId" : "c-1" ,"x":1}"#;
    for route in ["register", "push", "pull"] {
        let request = Request::post(format!("/api/v1/sync/{route}?trace=1"))
            .header("content-type", "application/json")
            .body(Body::from(&body[..]))
            .unwrap();
        assert_eq!(
            seen(request).await,
            Seen {
                method: "POST".into(),
                path: format!("/sync/{route}"),
                query: Some("trace=1".into()),
                body: body.to_vec(),
            }
        );
    }
}

#[tokio::test]
async fn the_poke_stream_is_its_route_with_an_empty_body() {
    let request = Request::get("/api/v1/sync/poke?clientId=00000000-0000-4000-8000-000000000001")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        seen(request).await,
        Seen {
            method: "GET".into(),
            path: "/sync/poke".into(),
            query: Some("clientId=00000000-0000-4000-8000-000000000001".into()),
            body: Vec::new(),
        }
    );
}
