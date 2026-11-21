//! cratestack#1425: the generated RPC client's `get_view`
//! (`cratestack-macros/src/client/rpc/model/view.rs`) reads a `Bytes` field
//! that arrives as a CBOR byte string, the form the server now writes, and
//! still reads the integer array an older server wrote. The server is a
//! hand-rolled axum route returning canned CBOR, so what is under test is
//! the client's decode, not the server's encode.

use std::collections::BTreeMap;

use cratestack::axum::Router;
use cratestack::axum::http::{StatusCode, header};
use cratestack::axum::response::IntoResponse;
use cratestack::axum::routing::post;
use cratestack::rpc::RpcFrameValue;
use cratestack::{CratestackCodec, SelectionQuery};
use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};
use serde_json::json;

cratestack::include_client_schema!("tests/fixtures/view_bytes_rpc.cstack");

fn reply(data: RpcFrameValue) -> impl IntoResponse {
    let doc = RpcFrameValue::Object(
        [
            ("id".to_owned(), RpcFrameValue::from(json!(1))),
            ("data".to_owned(), data),
        ]
        .into(),
    );
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, CborCodec::CONTENT_TYPE)],
        CborCodec.encode(&doc).expect("encode"),
    )
}

async fn get_view(data: RpcFrameValue) -> serde_json::Value {
    let app = Router::new().route(
        "/rpc/model.Doc.get",
        post(move || async move { reply(data.clone()) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    let server =
        tokio::spawn(async move { cratestack::axum::serve(listener, app).await.expect("serve") });
    let _ = rustls::crypto::ring::default_provider().install_default();
    let base_url = reqwest::Url::parse(&format!("http://{address}")).expect("url");
    let client = cratestack_schema::client::Client::new(CratestackClient::new(
        ClientConfig::new(base_url),
        CborCodec,
    ));
    let selection = SelectionQuery {
        fields: vec!["id".to_owned(), "data".to_owned()],
        includes: Vec::new(),
        include_fields: BTreeMap::new(),
    };
    let view = client.docs().get_view(&1, &selection).await;
    server.abort();
    view.expect("get_view reads the reply")
}

#[tokio::test]
async fn get_view_reads_a_byte_string_field() {
    let view = get_view(RpcFrameValue::Bytes(vec![1, 2, 255])).await;
    assert_eq!(view, json!({ "id": 1, "data": [1, 2, 255] }));
}

#[tokio::test]
async fn get_view_still_reads_the_array_form() {
    let view = get_view(RpcFrameValue::from(json!([1, 2, 255]))).await;
    assert_eq!(view, json!({ "id": 1, "data": [1, 2, 255] }));
}
