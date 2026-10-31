//! `clearDoneTasks`'s `@authn(maxAge: 300, amr: ["hwk"])` (cratestack#1245)
//! over the routes the web and Flutter apps call. The refusals need no
//! database: the generated dispatch refuses before the procedure runs, so a
//! lazy pool that never connects is enough. The successful call deletes rows,
//! so it is gated on `CRATESTACK_TEST_DATABASE_URL` like `smoke.rs`.

use cratestack::axum::Router;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode, header::WWW_AUTHENTICATE};
use cratestack::sqlx::PgPool;
use react_vite_swr_example::{build_router, ensure_schema};
use tower::ServiceExt;

const PATH: &str = "/api/$procs/clearDoneTasks";

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after the epoch")
        .as_secs() as i64
}

fn offline_router() -> Router {
    let pool = cratestack::sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://x:x@127.0.0.1/none")
        .expect("lazy pool should parse");
    build_router(react_vite_swr_example::schema::Cratestack::builder(pool).build())
}

/// `(x-auth-time, x-auth-amr)`: the demo session the apps start with, a
/// password sign-in twenty minutes ago.
fn stale_password() -> (String, &'static str) {
    ((now() - 1200).to_string(), "pwd")
}

/// What the apps' step-up screen leaves: the hardware key, just now.
fn fresh_hardware_key() -> (String, &'static str) {
    (now().to_string(), "hwk user")
}

async fn clear(
    app: Router,
    session: Option<(String, &str)>,
) -> (StatusCode, Vec<String>, serde_json::Value) {
    let mut request = Request::post(PATH)
        .header("content-type", "application/json")
        .header("accept", "application/json");
    if let Some((auth_time, amr)) = session {
        request = request
            .header("x-auth-id", "1")
            .header("x-auth-time", auth_time)
            .header("x-auth-amr", amr);
    }
    let body = Body::from(r#"{"args":{"boardId":1}}"#);
    let response = app.oneshot(request.body(body).unwrap()).await.unwrap();
    let status = response.status();
    let challenges = response
        .headers()
        .get_all(WWW_AUTHENTICATE)
        .iter()
        .map(|value| value.to_str().unwrap().to_owned())
        .collect();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, challenges, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn a_stale_password_session_gets_the_step_up_401() {
    let (status, challenges, body) = clear(offline_router(), Some(stale_password())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(
        challenges,
        [
            r#"Bearer error="insufficient_user_authentication", error_description="step-up authentication required", max_age="300""#
        ]
    );
    assert_eq!(body["error"], "insufficient_user_authentication");
    assert_eq!(
        body["details"],
        serde_json::json!({ "max_age": 300, "amr": ["hwk"], "unmet": ["max_age", "amr"] })
    );
}

#[tokio::test]
async fn an_anonymous_caller_is_refused_by_allow_not_asked_to_step_up() {
    let (status, challenges, body) = clear(offline_router(), None).await;
    assert_ne!(status, StatusCode::OK, "{body}");
    assert!(body.get("error").is_none(), "no step-up for {body}");
    assert!(
        challenges
            .iter()
            .all(|c| !c.contains("insufficient_user_authentication"))
    );
}

#[tokio::test]
async fn a_fresh_hardware_key_session_clears_the_done_tasks() {
    let Some(pool) = connect_or_skip().await else {
        eprintln!("CRATESTACK_TEST_DATABASE_URL not set — skipping");
        return;
    };
    cratestack::sqlx::query("DROP TABLE IF EXISTS tasks, boards")
        .execute(&pool)
        .await
        .expect("reset tables");
    ensure_schema(&pool).await.expect("create tables");
    for statement in [
        "INSERT INTO boards (id, name) VALUES (1, 'Launch')",
        "INSERT INTO tasks (id, title, done, board_id) VALUES (1, 'Done', true, 1), (2, 'Open', false, 1)",
    ] {
        cratestack::sqlx::query(statement)
            .execute(&pool)
            .await
            .expect("seed");
    }
    let app = build_router(react_vite_swr_example::schema::Cratestack::builder(pool).build());

    let (status, _, body) = clear(app.clone(), Some(stale_password())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, _, body) = clear(app, Some(fresh_hardware_key())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removed"], 1, "only the done task, and only once");
}

async fn connect_or_skip() -> Option<PgPool> {
    let url = std::env::var("CRATESTACK_TEST_DATABASE_URL").ok()?;
    PgPool::connect(&url).await.ok()
}
