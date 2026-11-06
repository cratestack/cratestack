//! Compact wire keys, server side, over the real generated `transport rpc`
//! router (`docs/design/compact-wire-keys.md` §5.1): negotiation by
//! `Accept` and `Cratestack-Contract`, compact request bodies, the 415
//! refusal, and everything the layer must leave alone.

mod compact_support;

use compact_support::{Anonymous, call, digest_of, order_json, selector, tables};
use cratestack::compact::{COMPACT_CBOR, COMPACT_CBOR_SEQ, COMPACT_JSON, Direction, OpKeys};
use cratestack::{CodecSet, CratestackCodec, CratestackContext, CratestackError};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_cbor::compact::{cbor_from_compact, cbor_to_compact};
use cratestack_codec_json::JsonCodec;
use cratestack_codec_json::compact::json_to_compact;

mod srv {
    cratestack::include_server_schema!("tests/fixtures/compact_rpc.cstack", db = None);
}

use srv::cratestack_schema as schema;

compact_procedures!(srv);

const CBOR: &str = "application/cbor";
const JSON: &str = "application/json";

fn router() -> cratestack::axum::Router {
    schema::axum::rpc_router(
        schema::Cratestack::builder().build(),
        Procedures,
        (),
        CodecSet::new(CborCodec, JsonCodec),
        Anonymous,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

fn keys(op: &str) -> OpKeys {
    let digest = digest_of(schema::OP_CONTRACTS, op);
    tables(schema::KEY_TABLES)
        .find_digest(op, &digest)
        .expect("tables hold the op")
        .clone()
}

fn sel(op: &str) -> String {
    selector(&digest_of(schema::OP_CONTRACTS, op))
}

fn compact(op: &str, dir: Direction, named: &[u8], json: bool) -> Vec<u8> {
    let tables = tables(schema::KEY_TABLES);
    let keys = keys(op);
    let node = tables.node(&keys, dir);
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

const ECHO: &str = "/rpc/procedure.echoOrder";

#[tokio::test]
async fn without_a_compact_accept_the_wire_is_untouched() {
    let router = router();
    let plain = call(
        &router,
        "POST",
        ECHO,
        &[("content-type", CBOR), ("accept", CBOR)],
        order_args(),
    )
    .await;
    assert_eq!(plain.status, 200);
    assert_eq!(plain.header("content-type"), Some(CBOR));
    assert_eq!(plain.header("vary"), None);

    // A compact Accept with no selector, or one that names no known digest,
    // is answered with names, byte for byte as above.
    let other = [("content-type", CBOR), ("accept", COMPACT_CBOR)];
    let no_selector = call(&router, "POST", ECHO, &other, order_args()).await;
    assert_eq!(no_selector.body, plain.body);
    assert_eq!(no_selector.header("content-type"), Some(CBOR));
    assert_eq!(no_selector.header("vary"), None);

    let stale = [
        ("content-type", CBOR),
        ("accept", COMPACT_CBOR),
        ("cratestack-contract", "AAAAAAAAAAA"),
    ];
    let unknown = call(&router, "POST", ECHO, &stale, order_args()).await;
    assert_eq!(unknown.body, plain.body);
    assert_eq!(unknown.header("content-type"), Some(CBOR));
}

#[tokio::test]
async fn a_compact_accept_with_a_selector_gets_integer_keys() {
    let router = router();
    let plain = call(
        &router,
        "POST",
        ECHO,
        &[("content-type", CBOR), ("accept", CBOR)],
        order_args(),
    )
    .await;
    let selector = sel("procedure.echoOrder");
    let headers = [
        ("content-type", CBOR),
        ("accept", &format!("{COMPACT_CBOR}, {CBOR}")),
        ("cratestack-contract", &selector),
    ];
    let answer = call(&router, "POST", ECHO, &headers, order_args()).await;
    assert_eq!(answer.status, 200);
    assert_eq!(answer.header("content-type"), Some(COMPACT_CBOR));
    assert_eq!(answer.header("vary"), Some("Accept, Cratestack-Contract"));
    assert_eq!(
        answer.body,
        compact(
            "procedure.echoOrder",
            Direction::Response,
            &plain.body,
            false
        )
    );
    assert_ne!(answer.body, plain.body);

    // Compact only in `Accept` (no named fallback) still works.
    let only = [
        ("content-type", CBOR),
        ("accept", COMPACT_CBOR),
        ("cratestack-contract", &selector),
    ];
    assert_eq!(
        call(&router, "POST", ECHO, &only, order_args()).await.body,
        answer.body
    );
}

#[tokio::test]
async fn a_compact_request_body_is_opened_and_its_type_rewritten() {
    let router = router();
    let selector = sel("procedure.echoOrder");
    let named = order_args();
    let body = compact("procedure.echoOrder", Direction::Request, &named, false);
    assert_ne!(body, named);

    let headers = [
        ("content-type", COMPACT_CBOR),
        ("accept", CBOR),
        ("cratestack-contract", &selector),
    ];
    let answer = call(&router, "POST", ECHO, &headers, body.clone()).await;
    let plain = call(
        &router,
        "POST",
        ECHO,
        &[("content-type", CBOR), ("accept", CBOR)],
        named,
    )
    .await;
    assert_eq!(answer.status, 200);
    assert_eq!(answer.header("content-type"), Some(CBOR));
    assert_eq!(answer.body, plain.body, "the handler saw the named body");

    // Compact both ways, a text key tolerated beside the integers.
    let both = [
        ("content-type", COMPACT_CBOR),
        ("accept", COMPACT_CBOR),
        ("cratestack-contract", &selector),
    ];
    let answer = call(&router, "POST", ECHO, &both, body).await;
    assert_eq!(answer.header("content-type"), Some(COMPACT_CBOR));
    assert_eq!(
        cbor_from_compact(
            &tables(schema::KEY_TABLES),
            tables(schema::KEY_TABLES).node(&keys("procedure.echoOrder"), Direction::Response),
            &answer.body
        )
        .expect("opens"),
        plain.body
    );
}

#[tokio::test]
async fn a_compact_request_without_a_known_selector_is_a_415() {
    let router = router();
    let body = compact(
        "procedure.echoOrder",
        Direction::Request,
        &order_args(),
        false,
    );
    for selector in [None, Some("AAAAAAAAAAA"), Some("not a selector")] {
        let mut headers = vec![("content-type", COMPACT_CBOR), ("accept", CBOR)];
        if let Some(selector) = selector {
            headers.push(("cratestack-contract", selector));
        }
        let answer = call(&router, "POST", ECHO, &headers, body.clone()).await;
        assert_eq!(answer.status, 415, "{selector:?}");
        let error: serde_json::Value = CborCodec.decode(&answer.body).expect("a named error body");
        assert_eq!(error["code"], "payload_type_unsupported");
    }
}

#[tokio::test]
async fn a_malformed_compact_body_is_a_400() {
    let router = router();
    let selector = sel("procedure.echoOrder");
    let headers = [
        ("content-type", COMPACT_CBOR),
        ("accept", CBOR),
        ("cratestack-contract", &selector),
    ];
    // `{ "order": { 99: 1 } }`: an integer key the table does not number.
    let bad = [
        0xa1, 0x65, b'o', b'r', b'd', b'e', b'r', 0xa1, 0x18, 0x63, 0x01,
    ];
    assert_eq!(
        call(&router, "POST", ECHO, &headers, bad.to_vec())
            .await
            .status,
        400
    );
}

#[tokio::test]
async fn batch_is_never_compacted() {
    let router = router();
    let selector = sel("procedure.echoOrder");
    let frames = CborCodec
        .encode(&serde_json::json!([{"id": 1, "op": "procedure.echoOrder", "input": {"order": order_json()}}]))
        .expect("encodes");
    let plain = call(
        &router,
        "POST",
        "/rpc/batch",
        &[("content-type", CBOR), ("accept", CBOR)],
        frames.clone(),
    )
    .await;
    assert_eq!(plain.status, 200);
    let headers = [
        ("content-type", CBOR),
        ("accept", COMPACT_CBOR),
        ("cratestack-contract", &selector),
    ];
    let asked = call(&router, "POST", "/rpc/batch", &headers, frames.clone()).await;
    assert_eq!(asked.body, plain.body);
    assert_eq!(asked.header("content-type"), plain.header("content-type"));
    assert_eq!(asked.header("vary"), None);

    let sent = [
        ("content-type", COMPACT_CBOR),
        ("accept", CBOR),
        ("cratestack-contract", &selector),
    ];
    assert_eq!(
        call(&router, "POST", "/rpc/batch", &sent, frames)
            .await
            .status,
        415
    );
}

#[tokio::test]
async fn an_error_body_stays_named() {
    let router = router();
    let selector = sel("procedure.boom");
    let plain = call(
        &router,
        "POST",
        "/rpc/procedure.boom",
        &[("content-type", CBOR), ("accept", CBOR)],
        order_args(),
    )
    .await;
    assert_eq!(plain.status, 404);
    let headers = [
        ("content-type", CBOR),
        ("accept", COMPACT_CBOR),
        ("cratestack-contract", &selector),
    ];
    let asked = call(
        &router,
        "POST",
        "/rpc/procedure.boom",
        &headers,
        order_args(),
    )
    .await;
    assert_eq!(asked.status, 404);
    assert_eq!(asked.body, plain.body);
    assert_eq!(asked.header("content-type"), Some(CBOR));
    assert_eq!(asked.header("vary"), None);
}

#[tokio::test]
async fn json_compact_uses_decimal_string_keys() {
    let router = router();
    let selector = sel("procedure.echoOrder");
    let named = serde_json::to_vec(&serde_json::json!({ "order": order_json() })).expect("json");
    let plain = call(
        &router,
        "POST",
        ECHO,
        &[("content-type", JSON), ("accept", JSON)],
        named.clone(),
    )
    .await;
    let headers = [
        ("content-type", JSON),
        ("accept", COMPACT_JSON),
        ("cratestack-contract", &selector),
    ];
    let answer = call(&router, "POST", ECHO, &headers, named.clone()).await;
    assert_eq!(answer.header("content-type"), Some(COMPACT_JSON));
    assert_eq!(
        answer.body,
        compact(
            "procedure.echoOrder",
            Direction::Response,
            &plain.body,
            true
        )
    );
    let value: serde_json::Value = serde_json::from_slice(&answer.body).expect("json");
    assert!(
        value
            .as_object()
            .expect("map")
            .keys()
            .all(|key| key.parse::<u32>().is_ok()),
        "{value}"
    );

    let body = compact("procedure.echoOrder", Direction::Request, &named, true);
    let sent = [
        ("content-type", COMPACT_JSON),
        ("accept", JSON),
        ("cratestack-contract", &selector),
    ];
    let echoed = call(&router, "POST", ECHO, &sent, body).await;
    assert_eq!(echoed.header("content-type"), Some(JSON));
    assert_eq!(echoed.body, plain.body);
}

#[tokio::test]
async fn lists_and_pages_are_compacted_item_by_item() {
    let router = router();
    for (op, path, page) in [
        ("procedure.listItems", "/rpc/procedure.listItems", false),
        ("procedure.pageItems", "/rpc/procedure.pageItems", true),
    ] {
        let selector = sel(op);
        let body = CborCodec
            .encode(&serde_json::json!({ "limit": 3 }))
            .expect("encodes");
        let plain = call(
            &router,
            "POST",
            path,
            &[("content-type", CBOR), ("accept", CBOR)],
            body.clone(),
        )
        .await;
        let headers = [
            ("content-type", CBOR),
            ("accept", COMPACT_CBOR),
            ("cratestack-contract", &selector),
        ];
        let answer = call(&router, "POST", path, &headers, body).await;
        assert_eq!(answer.header("content-type"), Some(COMPACT_CBOR), "{op}");
        assert_eq!(
            answer.body,
            compact(op, Direction::Response, &plain.body, false),
            "{op}"
        );
        assert_ne!(answer.body, plain.body, "{op}");
        if page {
            // The wrapper keeps its names, only the items are numbered.
            let text = String::from_utf8_lossy(&answer.body).into_owned();
            assert!(
                text.contains("items") && text.contains("pageInfo"),
                "{text:?}"
            );
            assert!(!text.contains("label"), "{text:?}");
        }
    }
}

#[tokio::test]
async fn a_stream_is_compacted_item_by_item() {
    let router = router();
    let selector = sel("procedure.ticks");
    let body = CborCodec
        .encode(&serde_json::json!({ "count": 3 }))
        .expect("encodes");
    let plain = call(
        &router,
        "POST",
        "/rpc/procedure.ticks",
        &[("content-type", CBOR), ("accept", "application/cbor-seq")],
        body.clone(),
    )
    .await;
    assert_eq!(plain.header("content-type"), Some("application/cbor-seq"));
    let headers = [
        ("content-type", CBOR),
        (
            "accept",
            &format!("{COMPACT_CBOR_SEQ}, application/cbor-seq"),
        ),
        ("cratestack-contract", &selector),
    ];
    let answer = call(&router, "POST", "/rpc/procedure.ticks", &headers, body).await;
    assert_eq!(answer.header("content-type"), Some(COMPACT_CBOR_SEQ));
    assert_eq!(answer.header("vary"), Some("Accept, Cratestack-Contract"));

    // Three items; each one equals the compacted named item.
    let item_len = plain.body.len() / 3;
    let expected: Vec<u8> = plain
        .body
        .chunks(item_len)
        .flat_map(|item| compact("procedure.ticks", Direction::Response, item, false))
        .collect();
    assert_eq!(plain.body.len() % 3, 0, "three items of equal size");
    assert_eq!(answer.body, expected);
}
