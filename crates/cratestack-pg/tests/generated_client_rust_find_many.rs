//! Regression test for a real bug in the initial `FindMany<Model>`
//! redesign (issue #371): `include_client_schema!` failed to compile
//! for any schema declaring a `FindMany<Model>` procedure argument —
//! `procedure/types.rs` resolves the arg type to `PostFindManyInput`
//! unconditionally, but only the *server* composer
//! (`include/server/collect/models.rs`) generated that struct;
//! `include/client.rs` never did. Fixed by splitting
//! `find_many_input.rs`'s `generate_find_many_input` into a shared
//! `generate_find_many_types` (the `<Model>Where`/`<Model>SortField`/
//! `<Model>OrderByClause`/`<Model>FindManyInput` *types*, usable by
//! both composers) and a server-only wrapper that adds the
//! `build_<model>_query_from_find_many` entry point on top (which needs
//! a live `Cratestack` DB handle a pure HTTP client doesn't have).
//!
//! This mirrors `generated_client_rust.rs`'s pattern: a fake in-process
//! axum server decodes the request body using the *client*-generated
//! `Args`/`PostFindManyInput` types, proving the client's serialized
//! wire format round-trips through the exact shape the server expects
//! — not just that the macro compiles.
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use cratestack::{include_client_schema, include_server_schema};
use cratestack_client_rust::{ClientConfig, CratestackClient};
use cratestack_core::CoolCodec;
use std::net::SocketAddr;

include_server_schema!("tests/fixtures/find_many_procedure.cstack", db = Postgres);

mod client_only_schema {
    use super::include_client_schema;

    include_client_schema!("tests/fixtures/find_many_procedure.cstack");
}

#[tokio::test]
async fn generated_rust_client_sends_a_structured_find_many_argument() {
    let (base_url, _server) = spawn_server().await;
    let runtime = CratestackClient::new(
        ClientConfig::new(base_url),
        cratestack_client_rust::CborCodec,
    );
    let client = cratestack_schema::client::Client::new(runtime);

    let results = client
        .procedures()
        .search_posts(
            &cratestack_schema::procedures::search_posts::Args {
                query: cratestack_schema::PostFindManyInput {
                    r#where: Some(cratestack_schema::PostWhere {
                        id: None,
                        title: None,
                        subtitle: None,
                        published: Some(cratestack::FieldFilterInput {
                            eq: Some(true),
                            ..Default::default()
                        }),
                        authorId: None,
                    }),
                    order_by: Some(vec![cratestack_schema::PostOrderByClause {
                        field: cratestack_schema::PostSortField::Title,
                        direction: cratestack::SortDirection::Asc,
                    }]),
                },
            },
            &[],
        )
        .await
        .expect("searchPosts should succeed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Matched Post");
}

#[derive(Clone)]
struct AppState {
    codec: cratestack_client_rust::CborCodec,
}

async fn spawn_server() -> (url::Url, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/$procs/searchPosts", post(handle_search_posts))
        .with_state(AppState {
            codec: cratestack_client_rust::CborCodec,
        });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr: SocketAddr = listener.local_addr().expect("listener should have addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server should run");
    });

    (
        url::Url::parse(&format!("http://{}", addr)).expect("base url should parse"),
        handle,
    )
}

async fn handle_search_posts(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !codec_headers_ok(&headers) {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    // Decoded with the *client*-generated `Args`/`PostFindManyInput`
    // types (via `client_only_schema`), not the server's — this is what
    // proves the client's own generated types actually exist and
    // serialize into the shape a real server-side decode expects.
    let args: client_only_schema::cratestack_schema::procedures::search_posts::Args =
        state.codec.decode(&body).expect("request should decode");
    let query = args.query;
    let where_ = query.r#where.expect("where should round-trip");
    assert_eq!(where_.published.expect("published filter").eq, Some(true));
    let order_by = query.order_by.expect("orderBy should round-trip");
    assert_eq!(order_by.len(), 1);
    assert_eq!(
        order_by[0].field,
        client_only_schema::cratestack_schema::PostSortField::Title
    );

    let payload = vec![cratestack_schema::Post {
        id: 1,
        title: "Matched Post".to_owned(),
        subtitle: None,
        published: true,
        authorId: 1,
    }];
    cbor_response(StatusCode::OK, &payload)
}

fn codec_headers_ok(headers: &HeaderMap) -> bool {
    let accept = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    accept.contains(cratestack_client_rust::CborCodec::CONTENT_TYPE)
        && content_type == cratestack_client_rust::CborCodec::CONTENT_TYPE
}

fn cbor_response<T: serde::Serialize>(status: StatusCode, value: &T) -> Response {
    let body = cratestack_client_rust::CborCodec
        .encode(value)
        .expect("response should encode");
    (
        status,
        [(
            axum::http::header::CONTENT_TYPE,
            cratestack_client_rust::CborCodec::CONTENT_TYPE,
        )],
        body,
    )
        .into_response()
}
