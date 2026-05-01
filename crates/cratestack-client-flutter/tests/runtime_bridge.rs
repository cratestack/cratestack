use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Router, body::Bytes};
use cratestack_client_flutter::{
    FlutterHeader, FlutterRequest, FlutterRuntime, FlutterRuntimeCodec, FlutterRuntimeConfig,
    FlutterRuntimeEnvelope, FlutterRuntimeTransportConfig, FlutterStateStoreConfig,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::CoolCodec;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FeedArgs {
    limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FeedItem {
    id: i64,
    title: String,
}

#[derive(Clone)]
struct AppState {
    codec: CborCodec,
}

#[test]
fn flutter_runtime_bridge_executes_cbor_request() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let (base_url, _server) = rt.block_on(spawn_server());
    let runtime = FlutterRuntime::new(FlutterRuntimeConfig {
        base_url: base_url.to_string(),
        state_store: FlutterStateStoreConfig::InMemory,
        transport: FlutterRuntimeTransportConfig {
            codec: FlutterRuntimeCodec::Cbor,
            envelope: FlutterRuntimeEnvelope::None,
        },
    })
    .expect("flutter runtime should build");
    let body = serde_json::to_vec(&FeedArgs { limit: Some(3) }).expect("json body should encode");

    let response = runtime
        .execute(FlutterRequest {
            method: "POST".to_owned(),
            path: "/$procs/getFeed".to_owned(),
            canonical_query: None,
            headers: vec![FlutterHeader {
                name: "x-auth-id".to_owned(),
                value: "5".to_owned(),
            }],
            body,
        })
        .expect("request should succeed");

    let posts: Vec<FeedItem> =
        serde_json::from_slice(&response.body).expect("response body should decode");
    assert_eq!(posts[0].id, 3);
}

#[test]
fn flutter_runtime_bridge_rejects_invalid_base_url() {
    let result = FlutterRuntime::new(FlutterRuntimeConfig {
        base_url: "not a url".to_owned(),
        state_store: FlutterStateStoreConfig::InMemory,
        transport: FlutterRuntimeTransportConfig {
            codec: FlutterRuntimeCodec::Cbor,
            envelope: FlutterRuntimeEnvelope::None,
        },
    });

    let error = match result {
        Ok(_) => panic!("invalid URL should fail"),
        Err(error) => error,
    };

    assert_eq!(
        error.code,
        cratestack_client_rust::RuntimeErrorCode::BadInput as u32
    );
}

async fn spawn_server() -> (Url, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/$procs/getFeed", post(handle_get_feed))
        .with_state(AppState { codec: CborCodec });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should have an address");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server should run");
    });
    let base_url = Url::parse(&format!("http://{address}/")).expect("base URL should parse");
    (base_url, handle)
}

async fn handle_get_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let accept = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !accept.contains(CborCodec::CONTENT_TYPE) || content_type != CborCodec::CONTENT_TYPE {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let args: FeedArgs = state.codec.decode(&body).expect("request should decode");
    let payload = vec![FeedItem {
        id: args.limit.unwrap_or(1),
        title: "Feed".to_owned(),
    }];
    let response_body = state
        .codec
        .encode(&payload)
        .expect("response payload should encode");
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, CborCodec::CONTENT_TYPE)],
        response_body,
    )
        .into_response()
}
