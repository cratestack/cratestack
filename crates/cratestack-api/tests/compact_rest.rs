//! Compact wire keys, server side, over the real generated REST router: the
//! op key is `"<METHOD> <route template>"`, found from axum's matched path,
//! also under a nested mount. The negotiation rules are those of
//! `compact_rpc.rs`; this file checks they hold on the REST surface.

mod compact_support;

use compact_support::{Anonymous, call, digest_of, order_json, selector, tables};
use cratestack::compact::{COMPACT_CBOR, COMPACT_CBOR_SEQ, COMPACT_JSON, Direction};
use cratestack::{CodecSet, CratestackCodec, CratestackContext, CratestackError};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_cbor::compact::cbor_to_compact;
use cratestack_codec_json::JsonCodec;
use cratestack_codec_json::compact::json_to_compact;

mod srv {
    cratestack::include_server_schema!("tests/fixtures/compact_rest.cstack", db = None);
}

use srv::cratestack_schema as schema;

compact_procedures!(srv);

const CBOR: &str = "application/cbor";
const JSON: &str = "application/json";

fn router() -> cratestack::axum::Router {
    schema::axum::router(
        schema::Cratestack::builder().build(),
        Procedures,
        (),
        CodecSet::new(CborCodec, JsonCodec),
        Anonymous,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

fn sel(op: &str) -> String {
    selector(&digest_of(schema::OP_CONTRACTS, op))
}

fn compact(op: &str, dir: Direction, named: &[u8], json: bool) -> Vec<u8> {
    let tables = tables(schema::KEY_TABLES);
    let digest = digest_of(schema::OP_CONTRACTS, op);
    let keys = tables.find_digest(op, &digest).expect("tables hold the op");
    let node = tables.node(keys, dir);
    if json {
        json_to_compact(&tables, node, named).expect("compacts")
    } else {
        cbor_to_compact(&tables, node, named).expect("compacts")
    }
}

fn order_args() -> Vec<u8> {
    CborCodec
        .encode(&serde_json::json!({ "order": order_json() }))
        .expect("encodes")
}

const ECHO_OP: &str = "POST /$procs/echoOrder";

#[tokio::test]
async fn the_op_keys_are_the_methods_and_templates_of_the_routes() {
    assert!(schema::OP_CONTRACTS.iter().any(|(key, _)| *key == ECHO_OP));
    assert!(
        schema::axum::ROUTE_TRANSPORTS
            .iter()
            .any(|route| format!("{} {}", route.method, route.path) == ECHO_OP)
    );
}

#[tokio::test]
async fn a_compact_accept_with_a_selector_gets_integer_keys_on_rest() {
    let router = router();
    let path = "/$procs/echoOrder";
    let plain = call(
        &router,
        "POST",
        path,
        &[("content-type", CBOR), ("accept", CBOR)],
        order_args(),
    )
    .await;
    assert_eq!(plain.status, 200);
    assert_eq!(plain.header("vary"), None);

    let selector = sel(ECHO_OP);
    let accept = format!("{COMPACT_CBOR}, {CBOR}");
    let headers = [
        ("content-type", CBOR),
        ("accept", &accept),
        ("cratestack-contract", &selector),
    ];
    let answer = call(&router, "POST", path, &headers, order_args()).await;
    assert_eq!(answer.header("content-type"), Some(COMPACT_CBOR));
    assert_eq!(answer.header("vary"), Some("Accept, Cratestack-Contract"));
    assert_eq!(
        answer.body,
        compact(ECHO_OP, Direction::Response, &plain.body, false)
    );

    // Without the selector: names, as today.
    let bare = call(
        &router,
        "POST",
        path,
        &[("content-type", CBOR), ("accept", &accept)],
        order_args(),
    )
    .await;
    assert_eq!(bare.body, plain.body);
    assert_eq!(bare.header("content-type"), Some(CBOR));
}

#[tokio::test]
async fn compact_requests_are_opened_and_unknown_selectors_are_a_415() {
    let router = router();
    let path = "/$procs/echoOrder";
    let selector = sel(ECHO_OP);
    let body = compact(ECHO_OP, Direction::Request, &order_args(), false);
    let plain = call(
        &router,
        "POST",
        path,
        &[("content-type", CBOR), ("accept", CBOR)],
        order_args(),
    )
    .await;

    let headers = [
        ("content-type", COMPACT_CBOR),
        ("accept", CBOR),
        ("cratestack-contract", &selector),
    ];
    let answer = call(&router, "POST", path, &headers, body.clone()).await;
    assert_eq!(answer.status, 200);
    assert_eq!(answer.body, plain.body);

    let stale = [
        ("content-type", COMPACT_CBOR),
        ("accept", CBOR),
        ("cratestack-contract", "AAAAAAAAAAA"),
    ];
    let refused = call(&router, "POST", path, &stale, body).await;
    assert_eq!(refused.status, 415);
    let error: serde_json::Value = CborCodec.decode(&refused.body).expect("a named error body");
    assert_eq!(error["code"], "PAYLOAD_TYPE_UNSUPPORTED");
}

#[tokio::test]
async fn errors_and_json_and_streams_on_rest() {
    let router = router();

    let boom = "POST /$procs/boom";
    let selector = sel(boom);
    let plain = call(
        &router,
        "POST",
        "/$procs/boom",
        &[("content-type", CBOR), ("accept", CBOR)],
        order_args(),
    )
    .await;
    let asked = call(
        &router,
        "POST",
        "/$procs/boom",
        &[
            ("content-type", CBOR),
            ("accept", COMPACT_CBOR),
            ("cratestack-contract", &selector),
        ],
        order_args(),
    )
    .await;
    assert_eq!((asked.status, &asked.body), (plain.status, &plain.body));
    assert_eq!(asked.header("content-type"), Some(CBOR));

    let named = serde_json::to_vec(&serde_json::json!({ "order": order_json() })).expect("json");
    let selector = sel(ECHO_OP);
    let plain = call(
        &router,
        "POST",
        "/$procs/echoOrder",
        &[("content-type", JSON), ("accept", JSON)],
        named.clone(),
    )
    .await;
    let headers = [
        ("content-type", JSON),
        ("accept", COMPACT_JSON),
        ("cratestack-contract", &selector),
    ];
    let answer = call(&router, "POST", "/$procs/echoOrder", &headers, named).await;
    assert_eq!(answer.header("content-type"), Some(COMPACT_JSON));
    assert_eq!(
        answer.body,
        compact(ECHO_OP, Direction::Response, &plain.body, true)
    );

    let ticks = "POST /$procs/ticks";
    let selector = sel(ticks);
    let body = CborCodec
        .encode(&serde_json::json!({ "count": 2 }))
        .expect("encodes");
    let plain = call(
        &router,
        "POST",
        "/$procs/ticks",
        &[("content-type", CBOR), ("accept", "application/cbor-seq")],
        body.clone(),
    )
    .await;
    let headers = [
        ("content-type", CBOR),
        ("accept", COMPACT_CBOR_SEQ),
        ("cratestack-contract", &selector),
    ];
    let answer = call(&router, "POST", "/$procs/ticks", &headers, body).await;
    assert_eq!(answer.header("content-type"), Some(COMPACT_CBOR_SEQ));
    let half = plain.body.len() / 2;
    let expected: Vec<u8> = plain
        .body
        .chunks(half)
        .flat_map(|item| compact(ticks, Direction::Response, item, false))
        .collect();
    assert_eq!(answer.body, expected);
}

#[tokio::test]
async fn a_nested_mount_needs_no_prefix_configuration() {
    let nested = cratestack::axum::Router::new().nest("/api/v2", router());
    let selector = sel(ECHO_OP);
    let plain = call(
        &nested,
        "POST",
        "/api/v2/$procs/echoOrder",
        &[("content-type", CBOR), ("accept", CBOR)],
        order_args(),
    )
    .await;
    let headers = [
        ("content-type", CBOR),
        ("accept", COMPACT_CBOR),
        ("cratestack-contract", &selector),
    ];
    let answer = call(
        &nested,
        "POST",
        "/api/v2/$procs/echoOrder",
        &headers,
        order_args(),
    )
    .await;
    assert_eq!(answer.header("content-type"), Some(COMPACT_CBOR));
    assert_eq!(
        answer.body,
        compact(ECHO_OP, Direction::Response, &plain.body, false)
    );
}
