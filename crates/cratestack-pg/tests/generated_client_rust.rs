use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use cratestack::{include_client_schema, include_server_schema};
use cratestack_client_rust::{ClientConfig, CratestackClient, JsonCodec};
use cratestack_core::CoolCodec;
use std::net::SocketAddr;

include_server_schema!("tests/fixtures/blog.cstack", db = Postgres);

mod client_only_schema {
    use super::include_client_schema;

    include_client_schema!("tests/fixtures/blog.cstack");
}

#[tokio::test]
async fn generated_rust_client_crud_and_view_surface_round_trips() {
    let (base_url, _server) = spawn_server().await;
    let runtime = CratestackClient::new(
        ClientConfig::new(base_url),
        cratestack_client_rust::CborCodec,
    );
    let client = cratestack_schema::client::Client::new(runtime);

    let listed = client
        .posts()
        .list(&[("limit", "2")], &[])
        .await
        .expect("list should succeed");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].title, "Published Post");

    let selected = client
        .posts()
        .get_view(
            &1,
            &cratestack_schema::post::select()
                .id()
                .include_author_selected(
                    cratestack_schema::user::include_selection()
                        .email()
                        .include_profile_selected(
                            cratestack_schema::profile::include_selection().nickname(),
                        ),
                ),
            &[],
        )
        .await
        .expect("projected get should succeed");
    assert_eq!(selected.id().expect("id should decode"), 1);
    let author = selected
        .author()
        .expect("author should decode")
        .expect("author should be present");
    assert_eq!(
        author.email().expect("email should decode"),
        "owner@example.com"
    );
    let profile = author
        .profile()
        .expect("profile should decode")
        .expect("profile should be present");
    assert_eq!(profile.nickname().expect("nickname should decode"), "Zulu");

    let created = client
        .posts()
        .create(
            &cratestack_schema::CreatePostInput {
                id: 3,
                title: "Created Post".to_owned(),
                subtitle: Some("created".to_owned()),
                published: true,
                authorId: 1,
            },
            &[],
        )
        .await
        .expect("create should succeed");
    assert_eq!(created.id, 3);

    let updated = client
        .posts()
        .update(
            &1,
            &cratestack_schema::UpdatePostInput {
                title: Some("Updated Post".to_owned()),
                subtitle: None,
                published: Some(false),
                authorId: None,
            },
            &[],
        )
        .await
        .expect("update should succeed");
    assert_eq!(updated.title, "Updated Post");
    assert!(!updated.published);

    let deleted = client
        .posts()
        .delete(&1, &[])
        .await
        .expect("delete should succeed");
    assert_eq!(deleted.id, 1);

    let feed = client
        .procedures()
        .get_feed(
            &cratestack_schema::procedures::get_feed::Args { limit: Some(1) },
            &[],
        )
        .await
        .expect("procedure call should succeed");
    assert_eq!(feed.len(), 1);
    assert_eq!(feed[0].title, "Feed Post");

    let feed_page = client
        .procedures()
        .get_feed_page(
            &cratestack_schema::procedures::get_feed_page::Args {
                limit: Some(2),
                offset: Some(1),
            },
            &[],
        )
        .await
        .expect("paged procedure call should succeed");
    assert_eq!(feed_page.items.len(), 1);
    assert_eq!(feed_page.items[0].title, "Feed Page");
    assert_eq!(feed_page.total_count, Some(3));
    assert_eq!(feed_page.page_info.limit, Some(2));
    assert_eq!(feed_page.page_info.offset, Some(1));

    let session_page = client
        .sessions()
        .list(&[("limit", "1")], &[])
        .await
        .expect("paged model list should succeed");
    assert_eq!(session_page.items.len(), 1);
    assert_eq!(session_page.items[0].label, "Primary Session");
    assert_eq!(session_page.total_count, Some(2));
    assert_eq!(session_page.page_info.limit, Some(1));

    let selected_session_page = client
        .sessions()
        .list_view(&cratestack_schema::session::select().id().label(), &[], &[])
        .await
        .expect("paged projected model list should succeed");
    assert_eq!(selected_session_page.items.len(), 1);
    let selected_session = &selected_session_page.items[0];
    assert_eq!(
        selected_session.id().expect("id should decode"),
        "csession1"
    );
    assert_eq!(
        selected_session.label().expect("label should decode"),
        "Primary Session"
    );
}

#[tokio::test]
async fn client_only_macro_generates_reqwest_backed_client_surface() {
    let (base_url, _server) = spawn_server().await;
    let runtime = CratestackClient::new(
        ClientConfig::new(base_url),
        cratestack_client_rust::CborCodec,
    );
    let client = client_only_schema::cratestack_schema::client::Client::new(runtime);

    let listed = client
        .posts()
        .list(&[("limit", "2")], &[])
        .await
        .expect("list should succeed");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].title, "Published Post");

    let selected = client
        .posts()
        .get_view(
            &1,
            &client_only_schema::cratestack_schema::post::select()
                .id()
                .include_author_selected(
                    client_only_schema::cratestack_schema::user::include_selection()
                        .email()
                        .include_profile_selected(
                            client_only_schema::cratestack_schema::profile::include_selection()
                                .nickname(),
                        ),
                ),
            &[],
        )
        .await
        .expect("projected get should succeed");
    assert_eq!(selected.id().expect("id should decode"), 1);
    let author = selected
        .author()
        .expect("author should decode")
        .expect("author should be present");
    assert_eq!(
        author.email().expect("email should decode"),
        "owner@example.com"
    );

    let feed = client
        .procedures()
        .get_feed(
            &client_only_schema::cratestack_schema::procedures::get_feed::Args { limit: Some(1) },
            &[],
        )
        .await
        .expect("procedure call should succeed");
    assert_eq!(feed.len(), 1);
    assert_eq!(feed[0].title, "Feed Post");
}

#[tokio::test]
async fn cbor_client_decodes_json_response_when_server_chooses_json() {
    let (base_url, _server) = spawn_json_server().await;
    let runtime = CratestackClient::new(
        ClientConfig::new(base_url),
        cratestack_client_rust::CborCodec,
    );

    let post: cratestack_schema::Post = runtime
        .get("/posts/1", &[], &[])
        .await
        .expect("json response should decode through cbor-configured client");

    assert_eq!(post.id, 1);
    assert_eq!(post.title, "JSON Post");
}

#[tokio::test]
async fn generated_rust_client_decodes_cbor_sequence_procedure_response() {
    let (base_url, _server) = spawn_cbor_seq_server().await;
    let runtime = CratestackClient::new(
        ClientConfig::new(base_url),
        cratestack_client_rust::CborCodec,
    );
    let client = cratestack_schema::client::Client::new(runtime);

    let feed = client
        .procedures()
        .get_feed(
            &cratestack_schema::procedures::get_feed::Args { limit: Some(2) },
            &[],
        )
        .await
        .expect("procedure cbor-seq call should succeed");

    assert_eq!(feed.len(), 2);
    assert_eq!(feed[0].title, "Feed One");
    assert_eq!(feed[1].title, "Feed Two");
}

#[derive(Clone)]
struct AppState {
    codec: cratestack_client_rust::CborCodec,
}

async fn spawn_server() -> (url::Url, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/$procs/getFeed", post(handle_get_feed))
        .route("/$procs/getFeedPage", post(handle_get_feed_page))
        .route("/posts", get(handle_list_posts).post(handle_create_post))
        .route("/sessions", get(handle_list_sessions))
        .route(
            "/posts/1",
            get(handle_get_post)
                .patch(handle_update_post)
                .delete(handle_delete_post),
        )
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

async fn spawn_json_server() -> (url::Url, tokio::task::JoinHandle<()>) {
    let app = Router::new().route("/posts/1", get(handle_get_post_json));

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

async fn spawn_cbor_seq_server() -> (url::Url, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/$procs/getFeed", post(handle_get_feed_cbor_seq))
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

async fn handle_list_posts(uri: Uri, headers: HeaderMap) -> Response {
    if !accept_header_ok(&headers) || uri.query() != Some("limit=2") {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let payload = vec![
        post_model(1, "Published Post", true),
        post_model(2, "Second Post", false),
    ];
    cbor_response(StatusCode::OK, &payload)
}

async fn handle_list_sessions(uri: Uri, headers: HeaderMap) -> Response {
    if !accept_header_ok(&headers) {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let query = uri.query().unwrap_or_default();
    if query.contains("fields=id%2Clabel") || query.contains("fields=id,label") {
        let payload = cratestack::Page::new(
            vec![cratestack::serde_json::json!({
                "id": "csession1",
                "label": "Primary Session"
            })],
            cratestack::PageInfo {
                limit: None,
                offset: None,
                has_next_page: true,
                has_previous_page: false,
            },
        )
        .with_total_count(Some(2));
        return projected_response(&headers, StatusCode::OK, &payload);
    }

    if query != "limit=1" {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let payload = cratestack::Page::new(
        vec![session_model("csession1", "Primary Session")],
        cratestack::PageInfo {
            limit: Some(1),
            offset: None,
            has_next_page: true,
            has_previous_page: false,
        },
    )
    .with_total_count(Some(2));
    projected_response(&headers, StatusCode::OK, &payload)
}

async fn handle_get_post(uri: Uri, headers: HeaderMap) -> Response {
    let query = uri.query().unwrap_or_default();
    if !accept_header_ok(&headers)
        || !query.contains("fields=id")
        || !query.contains("include=author%2Cauthor.profile")
        || !query.contains("includeFields%5Bauthor%5D=email")
        || !query.contains("includeFields%5Bauthor.profile%5D=nickname")
    {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let payload = cratestack::serde_json::json!({
        "id": 1,
        "author": {
            "email": "owner@example.com",
            "profile": {
                "nickname": "Zulu"
            }
        }
    });
    projected_response(&headers, StatusCode::OK, &payload)
}

async fn handle_get_post_json(headers: HeaderMap) -> Response {
    if !accept_header_ok(&headers) {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    json_response(StatusCode::OK, &post_model(1, "JSON Post", true))
}

async fn handle_create_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !codec_headers_ok(&headers) {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let input: cratestack_schema::CreatePostInput =
        state.codec.decode(&body).expect("request should decode");
    let payload = cratestack_schema::Post {
        id: 3,
        title: input.title,
        subtitle: input.subtitle,
        published: input.published,
        authorId: input.authorId,
    };
    cbor_response(StatusCode::OK, &payload)
}

async fn handle_update_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !codec_headers_ok(&headers) {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let input: cratestack_schema::UpdatePostInput =
        state.codec.decode(&body).expect("request should decode");
    let payload = cratestack_schema::Post {
        id: 1,
        title: input.title.unwrap_or_else(|| "Published Post".to_owned()),
        subtitle: input.subtitle.flatten(),
        published: input.published.unwrap_or(true),
        authorId: input.authorId.unwrap_or(1),
    };
    cbor_response(StatusCode::OK, &payload)
}

async fn handle_delete_post(headers: HeaderMap) -> Response {
    if !accept_header_ok(&headers) {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    cbor_response(StatusCode::OK, &post_model(1, "Published Post", true))
}

async fn handle_get_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !codec_headers_ok(&headers) {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let args: cratestack_schema::procedures::get_feed::Args =
        state.codec.decode(&body).expect("request should decode");
    let payload = vec![post_model(args.limit.unwrap_or(1), "Feed Post", true)];
    cbor_response(StatusCode::OK, &payload)
}

async fn handle_get_feed_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !codec_headers_ok(&headers) {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let args: cratestack_schema::procedures::get_feed_page::Args =
        state.codec.decode(&body).expect("request should decode");
    let limit = args.limit.unwrap_or(1);
    let offset = args.offset.unwrap_or(0);
    let payload = cratestack::Page::new(
        vec![post_model(limit + offset, "Feed Page", true)],
        cratestack::PageInfo {
            limit: Some(limit),
            offset: Some(offset),
            has_next_page: true,
            has_previous_page: offset > 0,
        },
    )
    .with_total_count(Some(3));
    cbor_response(StatusCode::OK, &payload)
}

async fn handle_get_feed_cbor_seq(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !sequence_codec_headers_ok(&headers) {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let args: cratestack_schema::procedures::get_feed::Args =
        state.codec.decode(&body).expect("request should decode");
    let payload = vec![
        post_model(args.limit.unwrap_or(1), "Feed One", true),
        post_model(args.limit.unwrap_or(2), "Feed Two", true),
    ];
    cbor_seq_response(StatusCode::OK, &payload)
}

fn post_model(id: i64, title: &str, published: bool) -> cratestack_schema::Post {
    cratestack_schema::Post {
        id,
        title: title.to_owned(),
        subtitle: Some("subtitle".to_owned()),
        published,
        authorId: 1,
    }
}

fn session_model(id: &str, label: &str) -> cratestack_schema::Session {
    cratestack_schema::Session {
        id: id.to_owned(),
        externalId: cratestack::uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111")
            .expect("uuid should parse"),
        label: label.to_owned(),
        createdAt: cratestack::chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .expect("datetime should parse")
            .with_timezone(&cratestack::chrono::Utc),
        revokedAt: None,
        userId: 1,
    }
}

fn accept_header_ok(headers: &HeaderMap) -> bool {
    let accept = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    accept.contains(cratestack_client_rust::CborCodec::CONTENT_TYPE)
        || accept.contains(JsonCodec::CONTENT_TYPE)
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

fn sequence_codec_headers_ok(headers: &HeaderMap) -> bool {
    let accept = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    accept.contains("application/cbor-seq")
        && accept.contains(cratestack_client_rust::CborCodec::CONTENT_TYPE)
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

fn json_response<T: serde::Serialize>(status: StatusCode, value: &T) -> Response {
    let body = JsonCodec.encode(value).expect("response should encode");
    (
        status,
        [(axum::http::header::CONTENT_TYPE, JsonCodec::CONTENT_TYPE)],
        body,
    )
        .into_response()
}

fn projected_response<T: serde::Serialize>(
    headers: &HeaderMap,
    status: StatusCode,
    value: &T,
) -> Response {
    let accept = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if accept.contains(JsonCodec::CONTENT_TYPE) {
        json_response(status, value)
    } else {
        cbor_response(status, value)
    }
}

fn cbor_seq_response<T: serde::Serialize>(status: StatusCode, values: &[T]) -> Response {
    let mut body = Vec::new();
    for value in values {
        body.extend(
            cratestack_client_rust::CborCodec
                .encode(value)
                .expect("response item should encode"),
        );
    }
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/cbor-seq")],
        body,
    )
        .into_response()
}
