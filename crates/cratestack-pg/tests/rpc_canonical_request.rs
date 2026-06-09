//! Pins the canonical signed request emitted on `transport rpc`.
//!
//! On `transport rpc` the canonical request the server feeds into
//! `AuthProvider::authenticate` MUST be the actual rpc request:
//! `POST /rpc/<op_id>` with the raw frame bytes (which carry the
//! primary key / patch / args). This must match what the rpc client
//! signs byte-for-byte — see `cratestack-client-rust/src/rpc/client.rs`
//! (`path = format!("/rpc/{}", op_id)`, method POST, frame body).
//!
//! This test caught two regressions in the first draft of #102:
//!   1. the canonical path was the bare op id (`procedure.ping`) instead
//!      of the concrete `/rpc/procedure.ping` URL, and
//!   2. for model `get`/`update`/`delete` the canonical body reused the
//!      REST body (empty / re-encoded patch) instead of the raw frame,
//!      so the id was not bound to the signature.
//!
//! It drives the REAL generated `rpc_router` over axum via `oneshot`
//! (no network) with a mock `AuthProvider` that records the
//! `RequestContext` it is handed.

use std::sync::{Arc, Mutex};

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{AuthProvider, CoolCodec, CoolContext, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/transport_rpc.cstack", db = Postgres);

mod support;

use support::pg;

/// What the mock provider observed about the canonical request.
#[derive(Clone, Debug)]
struct CapturedRequest {
    method: String,
    path: String,
    query: Option<String>,
    body: Vec<u8>,
}

/// Records the `RequestContext` it is handed, then returns an
/// authenticated context so the op proceeds past the `auth() != null`
/// policy.
#[derive(Clone)]
struct RecordingAuthProvider {
    captured: Arc<Mutex<Vec<CapturedRequest>>>,
}

impl AuthProvider for RecordingAuthProvider {
    type Error = cratestack::CoolError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        self.captured
            .lock()
            .expect("capture lock")
            .push(CapturedRequest {
                method: request.method.to_owned(),
                path: request.path.to_owned(),
                query: request.query.map(|q| q.to_owned()),
                body: request.body.to_vec(),
            });
        // Authenticated so the Widget `auth() != null` policy and the
        // procedure `@allow(auth() != null)` pass and we exercise the
        // full dispatch path.
        core::future::ready(Ok(CoolContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

#[derive(Clone)]
struct RpcProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for RpcProcedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::ping::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, cratestack::CoolError>,
    > + Send {
        core::future::ready(Ok(args.args))
    }

    fn bump(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::bump::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::bump::Output, cratestack::CoolError>,
    > + Send {
        core::future::ready(Ok(args.args))
    }

    fn many_pings(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::many_pings::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::many_pings::Output, cratestack::CoolError>,
    > + Send {
        core::future::ready(Ok(vec![args.args]))
    }
}

/// Drives the REAL `rpc_router` and asserts that the canonical request
/// the `AuthProvider` is handed is the concrete `/rpc/<op_id>` URL with
/// the raw frame bytes — pinning both #102 P1s.
#[tokio::test]
async fn rpc_canonical_is_concrete_rpc_url_with_raw_frame_body() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;

    cratestack::sqlx::query("DROP TABLE IF EXISTS widgets")
        .execute(pool)
        .await
        .expect("widgets reset");
    cratestack::sqlx::query("CREATE TABLE widgets (id BIGINT PRIMARY KEY, name TEXT NOT NULL)")
        .execute(pool)
        .await
        .expect("widgets table");
    cratestack::sqlx::query("INSERT INTO widgets (id, name) VALUES (7, 'Seven')")
        .execute(pool)
        .await
        .expect("seed widget");

    let captured = Arc::new(Mutex::new(Vec::<CapturedRequest>::new()));
    let codec = CborCodec;
    let router = cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        RpcProcedures,
        codec.clone(),
        RecordingAuthProvider {
            captured: captured.clone(),
        },
    );

    // --- procedure unary -----------------------------------------------
    let ping_frame = codec
        .encode(&cratestack_schema::procedures::ping::Args {
            args: cratestack_schema::PingArgs {
                nonce: "hello".into(),
            },
        })
        .expect("encode ping frame");
    let ping_resp = router
        .clone()
        .oneshot(
            Request::post("/rpc/procedure.ping")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(ping_frame.clone()))
                .expect("ping request builds"),
        )
        .await
        .expect("ping dispatch completes");
    assert_eq!(ping_resp.status(), StatusCode::OK);

    let ping_canonical = {
        let guard = captured.lock().expect("capture lock");
        guard.last().expect("procedure recorded a request").clone()
    };
    assert_eq!(
        ping_canonical.method, "POST",
        "rpc canonical method is POST"
    );
    assert_eq!(
        ping_canonical.path, "/rpc/procedure.ping",
        "rpc canonical path is the concrete /rpc/<op_id> URL, not the bare op id",
    );
    assert_eq!(ping_canonical.query, None, "rpc canonical query is None");
    assert_eq!(
        ping_canonical.body, ping_frame,
        "rpc canonical body is the exact raw frame bytes the client sent",
    );

    // --- model get -----------------------------------------------------
    // The id rides in the frame body, so it MUST appear in the signed
    // material. Two different ids => two different captured bodies.
    let get_frame = codec
        .encode(&cratestack::rpc::RpcPkInput::<i64> { id: 7 })
        .expect("encode get frame");
    let get_resp = router
        .clone()
        .oneshot(
            Request::post("/rpc/model.Widget.get")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(get_frame.clone()))
                .expect("get request builds"),
        )
        .await
        .expect("get dispatch completes");
    assert_eq!(get_resp.status(), StatusCode::OK);

    let get_canonical = {
        let guard = captured.lock().expect("capture lock");
        guard.last().expect("get recorded a request").clone()
    };
    assert_eq!(get_canonical.method, "POST");
    assert_eq!(
        get_canonical.path, "/rpc/model.Widget.get",
        "model get canonical path is the concrete /rpc/<op_id> URL",
    );
    assert_eq!(get_canonical.query, None);
    assert_eq!(
        get_canonical.body, get_frame,
        "model get canonical body is the `{{id}}` frame, so the id is signed material",
    );

    // Sanity: a different id yields a different captured body — proves the
    // id is bound to the signature, not stripped (the draft P1).
    let get_frame_other = codec
        .encode(&cratestack::rpc::RpcPkInput::<i64> { id: 999 })
        .expect("encode other get frame");
    let _ = router
        .clone()
        .oneshot(
            Request::post("/rpc/model.Widget.get")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(get_frame_other.clone()))
                .expect("other get request builds"),
        )
        .await
        .expect("other get dispatch completes");
    let get_canonical_other = {
        let guard = captured.lock().expect("capture lock");
        guard.last().expect("other get recorded a request").clone()
    };
    assert_eq!(
        get_canonical_other.body, get_frame_other,
        "changing the id changes the signed body",
    );
    assert_ne!(
        get_canonical.body, get_canonical_other.body,
        "two ids must produce two distinct signed bodies",
    );
}
