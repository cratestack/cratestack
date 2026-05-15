//! Generated client integration tests for `transport rpc` schemas.
//!
//! Verifies that `include_client_schema!` against an RPC schema emits a
//! typed `cratestack_schema::client::Client` whose model and procedure
//! methods speak the `/rpc/{op_id}` wire shape correctly: CRUD verbs
//! wrap inputs in the right `RpcListInput` / `RpcPkInput` /
//! `RpcUpdateInput` envelopes, procedures hit `RpcClient::call` for
//! unary and `RpcClient::call_streaming` for sequence-returning
//! procedures.
//!
//! The server is a hand-rolled axum router with canned responses for
//! each `/rpc/...` route, so the test exercises *what the typed client
//! puts on the wire* — not the full server runtime. End-to-end coverage
//! through the real RPC dispatcher lives in the example crates
//! (`rpc-streaming-client-rust-example`, etc.).

use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode, header};
use axum::routing::post;
use axum::Router;
use cratestack::include_client_schema;
use cratestack_client_rust::{ClientConfig, CborCodec, CratestackClient};
use cratestack_core::CoolCodec;

include_client_schema!("../cratestack/tests/fixtures/transport_rpc.cstack");

#[tokio::test]
async fn rpc_client_widget_list_get_create_update_delete_round_trip() {
    let (base_url, _server) = spawn_server().await;
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    // list — server decodes RpcListInput, returns Vec<Widget>.
    let listed = client
        .widgets()
        .list(&cratestack::rpc::RpcListInput::default())
        .await
        .expect("list should succeed");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, 1);
    assert_eq!(listed[0].name, "Alpha");
    assert_eq!(listed[1].id, 2);
    assert_eq!(listed[1].name, "Beta");

    // get — input wraps `id` in RpcPkInput { id }.
    let widget = client
        .widgets()
        .get(&1)
        .await
        .expect("get should succeed");
    assert_eq!(widget.id, 1);
    assert_eq!(widget.name, "Alpha");

    // create — body is CreateWidgetInput directly, no envelope.
    let created = client
        .widgets()
        .create(&cratestack_schema::CreateWidgetInput {
            id: 99,
            name: "Gamma".into(),
        })
        .await
        .expect("create should succeed");
    assert_eq!(created.id, 99);
    assert_eq!(created.name, "Gamma");

    // update — input wraps `id` + `patch` in RpcUpdateInput { id, patch }.
    let updated = client
        .widgets()
        .update(
            &1,
            &cratestack_schema::UpdateWidgetInput {
                name: Some("AlphaPrime".into()),
            },
        )
        .await
        .expect("update should succeed");
    assert_eq!(updated.id, 1);
    assert_eq!(updated.name, "AlphaPrime");

    // delete — input wraps `id` in RpcPkInput { id }; server returns the
    // deleted record.
    let deleted = client
        .widgets()
        .delete(&1)
        .await
        .expect("delete should succeed");
    assert_eq!(deleted.id, 1);
}

#[tokio::test]
async fn rpc_client_unary_procedure_round_trip() {
    let (base_url, _server) = spawn_server().await;
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    let echoed = client
        .procedures()
        .ping(&cratestack_schema::procedures::ping::Args {
            args: cratestack_schema::PingArgs {
                nonce: "hello".into(),
            },
        })
        .await
        .expect("ping should succeed");
    assert_eq!(echoed.nonce, "hello");

    let bumped = client
        .procedures()
        .bump(&cratestack_schema::procedures::bump::Args {
            args: cratestack_schema::PingArgs {
                nonce: "world".into(),
            },
        })
        .await
        .expect("bump should succeed");
    // The mock server appends "-bumped" so we can tell ping from bump.
    assert_eq!(bumped.nonce, "world-bumped");
}

#[tokio::test]
async fn rpc_client_sequence_procedure_streams_each_item() {
    let (base_url, _server) = spawn_server().await;
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    let mut rx = client
        .procedures()
        .many_pings(&cratestack_schema::procedures::many_pings::Args {
            args: cratestack_schema::PingArgs {
                nonce: "tick".into(),
            },
        })
        .await
        .expect("many_pings should open the stream");

    let mut collected = Vec::<String>::new();
    while let Some(item) = rx.recv().await {
        collected.push(item.expect("per-item should not error").nonce);
    }

    assert_eq!(collected, vec!["tick-0", "tick-1", "tick-2"]);
}

// -----------------------------------------------------------------------------
// Mock server — canned `/rpc/...` responses for each op.
// -----------------------------------------------------------------------------

const CBOR_SEQ: &str = "application/cbor-seq";

async fn spawn_server() -> (url::Url, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/rpc/model.Widget.list", post(handle_widget_list))
        .route("/rpc/model.Widget.get", post(handle_widget_get))
        .route("/rpc/model.Widget.create", post(handle_widget_create))
        .route("/rpc/model.Widget.update", post(handle_widget_update))
        .route("/rpc/model.Widget.delete", post(handle_widget_delete))
        .route("/rpc/procedure.ping", post(handle_proc_ping))
        .route("/rpc/procedure.bump", post(handle_proc_bump))
        .route("/rpc/procedure.many_pings", post(handle_proc_many_pings));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr: SocketAddr = listener.local_addr().expect("listener has addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server should run");
    });
    (
        url::Url::parse(&format!("http://{addr}")).expect("base url parses"),
        handle,
    )
}

fn widget(id: i64, name: &str) -> cratestack_schema::Widget {
    cratestack_schema::Widget {
        id,
        name: name.to_owned(),
    }
}

fn cbor_response<T: serde::Serialize>(status: StatusCode, body: &T) -> Response<Body> {
    let bytes = CborCodec.encode(body).expect("encode body");
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("application/cbor"))
        .body(Body::from(bytes))
        .expect("response builds")
}

async fn handle_widget_list(_body: Bytes) -> Response<Body> {
    // Server-side: would decode the body as RpcListInput. For the mock we
    // just return a canned list.
    
    cbor_response(StatusCode::OK, &vec![widget(1, "Alpha"), widget(2, "Beta")])
}

async fn handle_widget_get(body: Bytes) -> Response<Body> {
    // Decode the RpcPkInput wrapper to verify the client sent the right shape.
    let input: cratestack::rpc::RpcPkInput<i64> =
        CborCodec.decode(&body).expect("decode RpcPkInput");
    assert_eq!(input.id, 1, "client should have wrapped id in RpcPkInput");
    cbor_response(StatusCode::OK, &widget(1, "Alpha"))
}

async fn handle_widget_create(body: Bytes) -> Response<Body> {
    // No envelope on create — body should decode straight into the
    // generated client-side CreateWidgetInput.
    let input: cratestack_schema::CreateWidgetInput =
        CborCodec.decode(&body).expect("decode CreateWidgetInput");
    assert_eq!(input.id, 99);
    assert_eq!(input.name, "Gamma");
    cbor_response(StatusCode::OK, &widget(input.id, &input.name))
}

async fn handle_widget_update(body: Bytes) -> Response<Body> {
    let input: cratestack::rpc::RpcUpdateInput<i64, cratestack_schema::UpdateWidgetInput> =
        CborCodec.decode(&body).expect("decode RpcUpdateInput");
    assert_eq!(input.id, 1, "client should wrap id in RpcUpdateInput");
    let new_name = input
        .patch
        .name
        .clone()
        .expect("patch.name should be Some");
    cbor_response(StatusCode::OK, &widget(input.id, &new_name))
}

async fn handle_widget_delete(body: Bytes) -> Response<Body> {
    let input: cratestack::rpc::RpcPkInput<i64> =
        CborCodec.decode(&body).expect("decode RpcPkInput");
    assert_eq!(input.id, 1, "client should wrap id in RpcPkInput");
    cbor_response(StatusCode::OK, &widget(input.id, "Alpha"))
}

async fn handle_proc_ping(body: Bytes) -> Response<Body> {
    // Procedures use the generated `<proc>::Args` envelope `{ args: ... }`.
    let input: cratestack_schema::procedures::ping::Args =
        CborCodec.decode(&body).expect("decode ping::Args");
    cbor_response(StatusCode::OK, &input.args)
}

async fn handle_proc_bump(body: Bytes) -> Response<Body> {
    let input: cratestack_schema::procedures::bump::Args =
        CborCodec.decode(&body).expect("decode bump::Args");
    let mut echoed = input.args;
    echoed.nonce = format!("{}-bumped", echoed.nonce);
    cbor_response(StatusCode::OK, &echoed)
}

async fn handle_proc_many_pings(
    
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    // The generated streaming method sets `Accept: application/cbor-seq`
    // — assert that so a future regression that drops the Accept header
    // shows up here as a test failure.
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        accept.contains(CBOR_SEQ),
        "streaming call should advertise application/cbor-seq, got `{accept}`",
    );

    let input: cratestack_schema::procedures::many_pings::Args =
        CborCodec.decode(&body).expect("decode many_pings::Args");
    let prefix = input.args.nonce;

    let pre_encoded: Vec<Vec<u8>> = (0..3)
        .map(|i| {
            CborCodec
                .encode(&cratestack_schema::PingArgs {
                    nonce: format!("{prefix}-{i}"),
                })
                .expect("encode item")
        })
        .collect();

    let stream = async_stream::stream! {
        for bytes in pre_encoded {
            yield Ok::<_, Infallible>(Bytes::from(bytes));
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(CBOR_SEQ))
        .body(Body::from_stream(stream))
        .expect("response builds")
}
