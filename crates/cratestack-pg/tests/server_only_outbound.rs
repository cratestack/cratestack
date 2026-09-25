//! A `@server_only` field never reaches a response, over REST or RPC, on a
//! model that also has a `@computed` field.
//!
//! Procedure outputs of such a model are built field by field by the
//! generated `compose_<owner>_value` helpers
//! (`cratestack-macros/src/computed/compose.rs`), not by serde, so the
//! struct's `#[serde(skip)]` never applies to them. They used to take their
//! field list from `wire_model_fields`, which keeps `@server_only` fields,
//! and so sent `"secret": "HUNTER2"`. The procedure tests below cover every
//! output shape that composes (a model, `T?`, `T[]`, `Page<T>`, and a
//! `type` embedding the model at every arity), on both transports and
//! through `/rpc/batch`, and need no database.
//!
//! The model-read test audits the paths that did *not* leak, so that they
//! keep not leaking: get and list with computed fields, `?fields=` naming
//! a `@server_only` field (refused), a `?fields=` keeping only the computed
//! value derived from it, `?include=` of a related model that has both
//! attributes (to-many and to-one), and `includeFields[<relation>]` naming
//! a `@server_only` field (refused). Postgres-backed: run it with
//! `CRATESTACK_REQUIRE_DB=1`, or a missing database is a skip that still
//! prints `ok` (CLAUDE.md, "Critical test gotcha"). MCP tools are
//! `tests/server_only_outbound_mcp.rs`.

#[macro_use]
mod server_only_outbound_support;
mod support;

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::serde_json::{self, Value as Json, json};
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext, Value,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use server_only_outbound_support::{
    LABEL, PART_TOKEN, WIDGET_RECOVERY, WIDGET_SECRET, assert_no_server_only, expected_widget,
    hint_for, procedure_cases,
};
use support::pg;
use tower::util::ServiceExt;

#[derive(Clone)]
struct AllowAllAuth;

impl AuthProvider for AllowAllAuth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

/// For the procedure tests: they never touch Postgres.
fn lazy_pool() -> cratestack::sqlx::PgPool {
    PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse")
}

async fn send(
    router: cratestack::axum::Router,
    request: Request<Body>,
    content_type: &str,
) -> (StatusCode, Vec<u8>) {
    let request = {
        let (mut parts, body) = request.into_parts();
        for header in ["accept", "content-type"] {
            parts
                .headers
                .insert(header, content_type.parse().expect("header value"));
        }
        Request::from_parts(parts, body)
    };
    let response = router.oneshot(request).await.expect("request should run");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    (status, bytes.to_vec())
}

async fn post_json(router: cratestack::axum::Router, path: &str, body: &Json) -> Json {
    let request = Request::post(path)
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .expect("request should build");
    let (status, bytes) = send(router, request, JsonCodec::CONTENT_TYPE).await;
    assert_no_server_only(path, &bytes);
    let text = String::from_utf8_lossy(&bytes);
    assert_eq!(status, StatusCode::OK, "{path}: {text}");
    serde_json::from_slice(&bytes).unwrap_or_else(|_| panic!("{path}: not JSON: {text}"))
}

/// Every composing procedure output, exactly: no `secret`, both computed
/// fields present.
fn assert_procedure_output(name: &str, pointer: &str, expected: &Json, output: &Json) {
    let compared = output
        .pointer(pointer)
        .unwrap_or_else(|| panic!("{name}: no `{pointer}` in {output}"));
    assert_eq!(compared, expected, "{name}");
}

// The row the model-read test seeds: one widget, one part. The raw
// `@server_only` values are the ones `assert_no_server_only` searches for.
const SEED: [&str; 5] = [
    "DROP TABLE IF EXISTS so_out_parts, so_out_widgets",
    "CREATE TABLE so_out_widgets (id BIGINT PRIMARY KEY, label TEXT NOT NULL, \
     secret TEXT NOT NULL, recovery TEXT)",
    "CREATE TABLE so_out_parts (id BIGINT PRIMARY KEY, widget_id BIGINT NOT NULL, \
     name TEXT NOT NULL, token TEXT NOT NULL)",
    "INSERT INTO so_out_widgets (id, label, secret, recovery) VALUES (1, $1, $2, $3)",
    "INSERT INTO so_out_parts (id, widget_id, name, token) VALUES (10, 1, 'bolt', $1)",
];

async fn seed(pool: &cratestack::sqlx::PgPool) {
    for statement in SEED[..3].iter().copied() {
        cratestack::sqlx::query(statement)
            .execute(pool)
            .await
            .expect(statement);
    }
    cratestack::sqlx::query(SEED[3])
        .bind(LABEL)
        .bind(WIDGET_SECRET)
        .bind(WIDGET_RECOVERY)
        .execute(pool)
        .await
        .expect(SEED[3]);
    cratestack::sqlx::query(SEED[4])
        .bind(PART_TOKEN)
        .execute(pool)
        .await
        .expect(SEED[4]);
}

fn expected_part() -> Json {
    json!({ "id": 10, "widgetId": 1, "name": "bolt", "loud": "BOLT" })
}

fn expected_widget_with_parts() -> Json {
    let mut widget = expected_widget(1);
    widget["parts"] = json!([expected_part()]);
    widget
}

fn expected_part_with_widget() -> Json {
    let mut part = expected_part();
    part["widget"] = expected_widget(1);
    part
}

/// A model read that must answer `200` with exactly `expected`, or (for
/// `None`) be refused as a client error; the body is leak-checked either way.
fn check_read(what: &str, status: StatusCode, bytes: &[u8], expected: Option<&Json>) {
    assert_no_server_only(what, bytes);
    let text = String::from_utf8_lossy(bytes);
    match expected {
        Some(expected) => {
            assert_eq!(status, StatusCode::OK, "{what}: {text}");
            let body: Json = serde_json::from_slice(bytes).expect("JSON body");
            assert_eq!(&body, expected, "{what}");
        }
        None => assert!(
            status.is_client_error(),
            "{what}: a selection naming a @server_only field must be refused, got {status}: {text}"
        ),
    }
}

mod rest {
    use super::*;

    include_server_schema!("tests/fixtures/server_only_outbound.cstack", db = Postgres);
    so_out_impls!();

    fn procedure_router() -> cratestack::axum::Router {
        let db = cratestack_schema::Cratestack::builder(lazy_pool()).build();
        cratestack_schema::axum::procedure_router(
            db,
            Procedures,
            Resolvers,
            JsonCodec,
            AllowAllAuth,
        )
    }

    #[tokio::test]
    async fn a_procedure_output_never_carries_a_server_only_field() {
        for (name, pointer, expected) in procedure_cases() {
            let path = format!("/$procs/{name}");
            let output = post_json(procedure_router(), &path, &json!({ "label": LABEL })).await;
            assert_procedure_output(name, pointer, &expected, &output);
        }
    }

    /// The leak is in what is composed, not in how it is encoded: CBOR is
    /// checked on its raw bytes, where a leaked string is still verbatim.
    #[tokio::test]
    async fn a_procedure_output_never_carries_a_server_only_field_over_cbor() {
        let db = cratestack_schema::Cratestack::builder(lazy_pool()).build();
        let router = cratestack_schema::axum::procedure_router(
            db,
            Procedures,
            Resolvers,
            CborCodec,
            AllowAllAuth,
        );
        let body = CborCodec
            .encode(&json!({ "label": LABEL }))
            .expect("CBOR body");
        let request = Request::post("/$procs/soOutWidget")
            .body(Body::from(body))
            .expect("request should build");
        let (status, bytes) = send(router, request, CborCodec::CONTENT_TYPE).await;
        assert_eq!(status, StatusCode::OK);
        assert_no_server_only("CBOR soOutWidget", &bytes);
        let output: Json = CborCodec.decode(&bytes).expect("CBOR output");
        assert_eq!(output, expected_widget(1));
    }

    pub(super) async fn model_reads(pool: &cratestack::sqlx::PgPool) {
        let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
        let router = cratestack_schema::axum::model_router(db, Resolvers, JsonCodec, AllowAllAuth);
        let derived_only = json!({ "id": 1, "hint": hint_for(WIDGET_SECRET) });
        let cases: [(&str, Option<Json>); 10] = [
            ("/so_out_widgets/1", Some(expected_widget(1))),
            ("/so_out_widgets", Some(json!([expected_widget(1)]))),
            (
                "/so_out_widgets/1?include=parts",
                Some(expected_widget_with_parts()),
            ),
            (
                "/so_out_widgets?include=parts",
                Some(json!([expected_widget_with_parts()])),
            ),
            (
                "/so_out_parts/10?include=widget",
                Some(expected_part_with_widget()),
            ),
            ("/so_out_widgets/1?fields=id,hint", Some(derived_only)),
            ("/so_out_widgets/1?fields=id,secret", None),
            ("/so_out_widgets/1?fields=id,recovery", None),
            ("/so_out_widgets?fields=secret", None),
            (
                "/so_out_widgets/1?include=parts&includeFields%5Bparts%5D=token",
                None,
            ),
        ];
        for (path, expected) in cases {
            let request = Request::get(path)
                .body(Body::empty())
                .expect("request should build");
            let (status, bytes) = send(router.clone(), request, JsonCodec::CONTENT_TYPE).await;
            check_read(
                &format!("REST GET {path}"),
                status,
                &bytes,
                expected.as_ref(),
            );
        }
    }
}

mod rpc {
    use super::*;

    include_server_schema!(
        "tests/fixtures/server_only_outbound_rpc.cstack",
        db = Postgres
    );
    so_out_impls!();

    fn router(pool: cratestack::sqlx::PgPool) -> cratestack::axum::Router {
        let db = cratestack_schema::Cratestack::builder(pool).build();
        cratestack_schema::axum::rpc_router(
            db,
            Procedures,
            Resolvers,
            JsonCodec,
            AllowAllAuth,
            cratestack::DEFAULT_BODY_LIMIT_BYTES,
        )
    }

    #[tokio::test]
    async fn a_procedure_output_never_carries_a_server_only_field() {
        for (name, pointer, expected) in procedure_cases() {
            let path = format!("/rpc/procedure.{name}");
            let output = post_json(router(lazy_pool()), &path, &json!({ "label": LABEL })).await;
            assert_procedure_output(name, pointer, &expected, &output);
        }
    }

    /// `/rpc/batch` re-dispatches each frame through the unary path; this
    /// pins that it keeps doing so for a composing output.
    #[tokio::test]
    async fn a_batched_procedure_output_never_carries_a_server_only_field() {
        let frames: Vec<Json> = procedure_cases()
            .iter()
            .enumerate()
            .map(|(id, (name, _, _))| {
                json!({ "id": id, "op": format!("procedure.{name}"), "input": { "label": LABEL } })
            })
            .collect();
        let responses = post_json(router(lazy_pool()), "/rpc/batch", &json!(frames)).await;
        for (id, (name, pointer, expected)) in procedure_cases().iter().enumerate() {
            let frame = &responses[id];
            assert_eq!(frame["id"], json!(id), "{name}: {frame}");
            assert_procedure_output(name, pointer, expected, &frame["output"]);
        }
    }

    /// `@@subscribe` SSE never composes: the generated
    /// `model.<X>.subscribe` arm hands `ModelEvent<Model>` (the server
    /// struct, no computed fields) to this encoder, which goes through
    /// serde, so `#[serde(skip)]` holds. Driven here with `secret` set,
    /// which a real event never has (its data is decoded from a serde-built
    /// envelope), so the encoder itself is what is checked. The route end
    /// to end is `tests/rpc_subscribe_sse.rs`.
    #[tokio::test]
    async fn a_subscribed_model_event_never_carries_a_server_only_field() {
        let event = cratestack::ModelEvent {
            event_id: cratestack::uuid::Uuid::nil(),
            model: "SoOutWidget".to_owned(),
            operation: cratestack::ModelEventKind::Created,
            occurred_at: cratestack::chrono::DateTime::UNIX_EPOCH,
            data: widget(1, LABEL),
        };
        let response = cratestack::__private::encode_model_event_sse_response(
            cratestack::futures::stream::iter([event]),
        );
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the stream ends after one event");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains(LABEL), "the event was sent: {text}");
        assert_no_server_only("SSE model event", &bytes);
    }

    pub(super) async fn model_reads(pool: &cratestack::sqlx::PgPool) {
        let derived_only = json!({ "id": 1, "hint": hint_for(WIDGET_SECRET) });
        let cases: [(&str, Json, Option<Json>); 10] = [
            (
                "model.SoOutWidget.get",
                json!({ "id": 1 }),
                Some(expected_widget(1)),
            ),
            (
                "model.SoOutWidget.list",
                json!({}),
                Some(json!([expected_widget(1)])),
            ),
            (
                "model.SoOutWidget.get",
                json!({ "id": 1, "include": ["parts"] }),
                Some(expected_widget_with_parts()),
            ),
            (
                "model.SoOutWidget.list",
                json!({ "include": ["parts"] }),
                Some(json!([expected_widget_with_parts()])),
            ),
            (
                "model.SoOutPart.get",
                json!({ "id": 10, "include": ["widget"] }),
                Some(expected_part_with_widget()),
            ),
            (
                "model.SoOutWidget.get",
                json!({ "id": 1, "fields": ["id", "hint"] }),
                Some(derived_only),
            ),
            (
                "model.SoOutWidget.get",
                json!({ "id": 1, "fields": ["id", "secret"] }),
                None,
            ),
            (
                "model.SoOutWidget.get",
                json!({ "id": 1, "fields": ["id", "recovery"] }),
                None,
            ),
            (
                "model.SoOutWidget.list",
                json!({ "fields": ["secret"] }),
                None,
            ),
            (
                "model.SoOutWidget.get",
                json!({ "id": 1, "include": ["parts"], "include_fields": { "parts": ["token"] } }),
                None,
            ),
        ];
        for (op, input, expected) in cases {
            let request = Request::post(format!("/rpc/{op}"))
                .body(Body::from(serde_json::to_vec(&input).unwrap()))
                .expect("request should build");
            let (status, bytes) =
                send(router(pool.clone()), request, JsonCodec::CONTENT_TYPE).await;
            check_read(
                &format!("RPC {op} {input}"),
                status,
                &bytes,
                expected.as_ref(),
            );
        }
    }
}

/// One test, one container: a second container start in one binary races
/// rootless Docker's port manager (`tests/mcp_policy_pg.rs`).
#[tokio::test]
async fn a_model_read_never_carries_a_server_only_field() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    seed(&test_pg.pool).await;
    rest::model_reads(&test_pg.pool).await;
    rpc::model_reads(&test_pg.pool).await;
}
