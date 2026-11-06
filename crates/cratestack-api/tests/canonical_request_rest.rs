//! ADR 0021 §3 (cratestack#1243) on the REST binding: the provider sees the
//! declared route relative to the router's root, never the prefix it is
//! mounted under, with no query for a procedure and the exact body bytes.

mod canonical_support;

use canonical_support::{Recorder, Seen};
use cratestack::CratestackCodec;
use cratestack::axum::Router;
use cratestack::axum::body::Body;
use cratestack::axum::http::Request;
use cratestack::{CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/trusted_proxy_client_ip.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn who_am_i(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: cratestack_schema::procedures::who_am_i::Args,
        _authorized: cratestack_schema::procedures::who_am_i::Authorized,
    ) -> Result<cratestack_schema::procedures::who_am_i::Output, CratestackError> {
        Ok("unreachable".to_owned())
    }
}

#[tokio::test]
async fn a_procedure_is_its_declared_route_without_the_mount_prefix() {
    let recorder = Recorder::default();
    let router = cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder().build(),
        Procedures,
        (),
        JsonCodec,
        recorder.clone(),
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let app = Router::new().nest("/api/v1", router);
    let body = br#"{ "args" : {} }"#.to_vec();
    let request = Request::post("/api/v1/$procs/whoAmI?trace=1")
        .header("content-type", JsonCodec::CONTENT_TYPE)
        .header("accept", JsonCodec::CONTENT_TYPE)
        .body(Body::from(body.clone()))
        .unwrap();
    let _ = app.oneshot(request).await.unwrap();
    assert_eq!(
        recorder.only(),
        Seen {
            method: "POST".into(),
            path: "/$procs/whoAmI".into(),
            query: None,
            body,
        }
    );
}
