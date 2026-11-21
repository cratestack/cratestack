//! cratestack#1425: the REST projection-view calls (`get_view`, `list_view`,
//! `list_view_paged`) read a response whose `Bytes` field is a CBOR byte
//! string, the form the server now writes, as well as the integer array an
//! older server wrote. Each call site is exercised, not just the shared
//! decode helper, so a call site that goes back to decoding into
//! `serde_json::Value` fails here.

use std::collections::BTreeMap;

use axum::Router;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use cratestack_client_rust::{ClientConfig, CratestackClient};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::rpc::RpcFrameValue;
use cratestack_core::{CratestackCodec, SelectionQuery};
use serde_json::json;
use url::Url;

fn doc(data: RpcFrameValue) -> RpcFrameValue {
    RpcFrameValue::Object(
        [
            ("id".to_owned(), RpcFrameValue::from(json!(1))),
            ("data".to_owned(), data),
        ]
        .into(),
    )
}

fn page(data: RpcFrameValue) -> RpcFrameValue {
    RpcFrameValue::from(json!({
        "items": [],
        "totalCount": 1,
        "pageInfo": { "limit": 10, "offset": 0, "hasNextPage": false, "hasPreviousPage": false },
    }))
    .with_items(vec![doc(data)])
}

trait WithItems {
    fn with_items(self, items: Vec<RpcFrameValue>) -> Self;
}

impl WithItems for RpcFrameValue {
    fn with_items(self, items: Vec<RpcFrameValue>) -> Self {
        let Self::Object(mut fields) = self else {
            unreachable!("a page is an object")
        };
        fields.insert("items".to_owned(), Self::Array(items));
        Self::Object(fields)
    }
}

fn cbor(value: &RpcFrameValue) -> Vec<u8> {
    CborCodec.encode(value).expect("encode")
}

fn reply(body: Vec<u8>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, CborCodec::CONTENT_TYPE)],
        body,
    )
}

async fn spawn(data: RpcFrameValue) -> Url {
    let one = cbor(&doc(data.clone()));
    let many = cbor(&RpcFrameValue::Array(vec![doc(data.clone())]));
    let paged = cbor(&page(data));
    let app = Router::new()
        .route("/docs/1", get(move || async move { reply(one.clone()) }))
        .route("/docs", get(move || async move { reply(many.clone()) }))
        .route(
            "/docs-paged",
            get(move || async move { reply(paged.clone()) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    Url::parse(&format!("http://{address}/")).expect("url")
}

fn selection() -> SelectionQuery {
    SelectionQuery {
        fields: vec!["id".to_owned(), "data".to_owned()],
        includes: Vec::new(),
        include_fields: BTreeMap::new(),
    }
}

async fn assert_every_view_call_reads(data: RpcFrameValue) {
    let client = CratestackClient::new(ClientConfig::new(spawn(data).await), CborCodec);
    let expected = json!({ "id": 1, "data": [1, 2, 255] });

    let one = client
        .get_view("/docs/1", &selection(), &[])
        .await
        .expect("get_view reads the body");
    assert_eq!(one, expected, "get_view");

    let many = client
        .list_view("/docs", &selection(), &[], &[])
        .await
        .expect("list_view reads the body");
    assert_eq!(many, vec![expected.clone()], "list_view");

    let paged = client
        .list_view_paged("/docs-paged", &selection(), &[], &[])
        .await
        .expect("list_view_paged reads the body");
    assert_eq!(paged.items, vec![expected], "list_view_paged");
}

#[tokio::test]
async fn every_view_call_reads_a_byte_string_field() {
    assert_every_view_call_reads(RpcFrameValue::Bytes(vec![1, 2, 255])).await;
}

#[tokio::test]
async fn every_view_call_still_reads_the_array_form() {
    assert_every_view_call_reads(RpcFrameValue::from(json!([1, 2, 255]))).await;
}
