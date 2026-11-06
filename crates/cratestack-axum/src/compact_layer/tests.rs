//! Layer-level tests with hand-built tables (no schema): chunked streams,
//! the error item, the request bound and header handling.

use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use axum::routing::post;
use cratestack_codec_cbor::compact::cbor_to_compact;
use cratestack_core::compact::{
    COMPACT_CBOR, COMPACT_CBOR_SEQ, KeyNode, KeyStruct, KeyTables, OpKeys,
};
use cratestack_core::rpc::RPC_STREAM_ERROR_TAG;
use cratestack_core::{ContractSelector, CratestackCodec};
use futures_util::stream;
use tower::{Layer, ServiceExt};

use super::CompactLayer;

const DIGEST: [u8; 32] = [7; 32];

fn tables() -> KeyTables {
    KeyTables {
        types: vec![KeyStruct {
            fields: vec![
                ("id".to_owned(), KeyNode::Opaque),
                ("label".to_owned(), KeyNode::Opaque),
            ],
        }],
        ops: vec![OpKeys {
            op: "procedure.ticks".to_owned(),
            digest: DIGEST,
            request: KeyNode::Opaque,
            response: KeyNode::Struct(0),
        }],
    }
}

fn item(id: i64) -> Vec<u8> {
    cratestack_codec_cbor::CborCodec
        .encode(&serde_json::json!({"id": id, "label": "x"}))
        .expect("encodes")
}

fn error_item() -> Vec<u8> {
    let mut bytes = Vec::new();
    minicbor::Encoder::new(&mut bytes)
        .tag(minicbor::data::Tag::new(RPC_STREAM_ERROR_TAG))
        .expect("tag");
    bytes.extend([0xa1, 0x64, b'c', b'o', b'd', b'e', 0x61, b'x']);
    bytes
}

fn compacted(named: &[u8]) -> Vec<u8> {
    let tables = tables();
    cbor_to_compact(&tables, &KeyNode::Struct(0), named).expect("compacts")
}

fn app(chunks: Vec<Vec<u8>>, limit: usize) -> Router {
    Router::new()
        .route(
            "/rpc/procedure.ticks",
            post(move || {
                let chunks = chunks.clone();
                async move {
                    let body = Body::from_stream(stream::iter(
                        chunks
                            .into_iter()
                            .map(|c| Ok::<_, std::convert::Infallible>(Bytes::from(c))),
                    ));
                    let mut response = Response::new(body);
                    response.headers_mut().insert(
                        header::CONTENT_TYPE,
                        "application/cbor-seq".parse().unwrap(),
                    );
                    response
                }
            }),
        )
        .layer(CompactLayer::new(tables(), limit))
}

fn request(accept: &str) -> Request<Body> {
    Request::post("/rpc/procedure.ticks")
        .header(header::ACCEPT, accept)
        .header(
            "cratestack-contract",
            ContractSelector::of(&DIGEST).to_header_value(),
        )
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn items_split_across_chunks_are_compacted_and_the_error_item_passes() {
    let whole: Vec<u8> = [item(1), item(2), error_item()].concat();
    // One byte per chunk: every item boundary falls inside a chunk.
    let chunks = whole.iter().map(|b| vec![*b]).collect();
    let answer = app(chunks, 1024)
        .oneshot(request(COMPACT_CBOR_SEQ))
        .await
        .unwrap();
    assert_eq!(answer.headers()[header::CONTENT_TYPE], COMPACT_CBOR_SEQ);
    let body = to_bytes(answer.into_body(), usize::MAX).await.unwrap();
    let expected: Vec<u8> = [compacted(&item(1)), compacted(&item(2)), error_item()].concat();
    assert_eq!(body.as_ref(), expected.as_slice());
}

#[tokio::test]
async fn a_stream_without_the_compact_accept_is_not_touched() {
    let whole: Vec<u8> = [item(1), item(2)].concat();
    let answer = app(vec![whole.clone()], 1024)
        .oneshot(request("application/cbor-seq"))
        .await
        .unwrap();
    assert_eq!(
        answer.headers()[header::CONTENT_TYPE],
        "application/cbor-seq"
    );
    assert!(answer.headers().get(header::VARY).is_none());
    let body = to_bytes(answer.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body.as_ref(), whole.as_slice());
}

#[tokio::test]
async fn a_compact_accept_with_a_zero_weight_is_not_a_request_for_compact() {
    let whole = item(1);
    let accept = format!("{COMPACT_CBOR_SEQ};q=0, application/cbor-seq");
    let answer = app(vec![whole.clone()], 1024)
        .oneshot(request(&accept))
        .await
        .unwrap();
    assert_eq!(
        answer.headers()[header::CONTENT_TYPE],
        "application/cbor-seq"
    );
}

#[tokio::test]
async fn a_compact_request_over_the_limit_is_a_413() {
    let selector = ContractSelector::of(&DIGEST).to_header_value();
    let request = Request::post("/rpc/procedure.ticks")
        .header(header::CONTENT_TYPE, COMPACT_CBOR)
        .header("cratestack-contract", selector)
        .body(Body::from(vec![0xa0; 64]))
        .unwrap();
    let answer = app(vec![item(1)], 16).oneshot(request).await.unwrap();
    assert_eq!(answer.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn the_layer_is_a_plain_tower_layer() {
    let service = CompactLayer::new(KeyTables::default(), 16).layer(tower::service_fn(
        |_: axum::extract::Request| async {
            Ok::<_, std::convert::Infallible>(Response::new(Body::empty()))
        },
    ));
    let answer = service
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(answer.status(), StatusCode::OK);
}
