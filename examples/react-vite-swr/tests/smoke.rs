//! End-to-end demo of the example server. `router_builds_offline` needs
//! no database (proves the macro wiring compiles + assembles before you
//! go hunting a Postgres instance). `crud_and_procedure_round_trip_over_http`
//! is the real thing — a live Postgres connection, gated on
//! `CRATESTACK_TEST_DATABASE_URL` (skips silently when unset, the same
//! convention every other PG-backed test in this workspace uses; `just
//! test-pg` sets it) — driven in-process via `tower::ServiceExt`, proving
//! the exact routes (`/api/boards`, `/api/tasks`, `/api/$procs/...`) the
//! generated TypeScript client in `web/` calls over real HTTP.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::sqlx::PgPool;
use react_vite_swr_example::{build_router, ensure_schema};
use tower::ServiceExt;

#[tokio::test]
async fn router_builds_offline() {
    let pool = cratestack::sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://x:x@127.0.0.1/none")
        .expect("lazy pool should parse");
    let db = react_vite_swr_example::schema::Cratestack::builder(pool).build();
    let _router = build_router(db);
}

async fn connect_or_skip() -> Option<PgPool> {
    let url = std::env::var("CRATESTACK_TEST_DATABASE_URL").ok()?;
    PgPool::connect(&url).await.ok()
}

#[tokio::test]
async fn crud_and_procedure_round_trip_over_http() {
    let Some(pool) = connect_or_skip().await else {
        eprintln!("CRATESTACK_TEST_DATABASE_URL not set — skipping");
        return;
    };

    cratestack::sqlx::query("DROP TABLE IF EXISTS tasks, boards")
        .execute(&pool)
        .await
        .expect("reset tables");
    ensure_schema(&pool).await.expect("create tables");

    let db = react_vite_swr_example::schema::Cratestack::builder(pool).build();
    let app = build_router(db);

    let call = |method: &'static str, path: String, body: serde_json::Value| {
        let app = app.clone();
        async move {
            let response = app
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header("content-type", "application/json")
                        .header("accept", "application/json")
                        .header("x-auth-id", "1")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let payload: serde_json::Value = if bytes.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::from_slice(&bytes).unwrap()
            };
            (status, payload)
        }
    };

    // create a board
    let (status, board) = call(
        "POST",
        "/api/boards".to_owned(),
        serde_json::json!({ "id": 1, "name": "Launch" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{board:?}");
    assert_eq!(board["name"], "Launch");

    // create two tasks on that board
    let (status, task_one) = call(
        "POST",
        "/api/tasks".to_owned(),
        serde_json::json!({ "id": 1, "title": "Write README", "done": false, "boardId": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{task_one:?}");

    let (status, _task_two) = call(
        "POST",
        "/api/tasks".to_owned(),
        serde_json::json!({ "id": 2, "title": "Ship it", "done": false, "boardId": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // list reflects both tasks — this is the state the generated
    // `useTasks()` hook reads, and what a create-mutation's invalidation
    // must cause to refetch.
    let (status, list) = call("GET", "/api/tasks".to_owned(), serde_json::Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 2, "{list:?}");

    // mark the first task done via update
    let (status, updated) = call(
        "PATCH",
        "/api/tasks/1".to_owned(),
        serde_json::json!({ "done": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["done"], true);

    // delete the second task
    let (status, _) = call("DELETE", "/api/tasks/2".to_owned(), serde_json::Value::Null).await;
    assert_eq!(status, StatusCode::OK);

    let (status, list_after_delete) =
        call("GET", "/api/tasks".to_owned(), serde_json::Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list_after_delete.as_array().unwrap().len(), 1);

    // the stateless procedure, over the same router. Body is wrapped in
    // `{ "args": { ... } }` — the procedure is declared as
    // `estimateFocusMinutes(args: FocusEstimateArgs)`, one named arg
    // called `args`, so the generated `EstimateFocusMinutesArgs` wire
    // type (and the Rust `Args` struct) both nest the payload one level
    // under that field name. This is the exact shape
    // `web/src/procedures.tsx` sends via the generated
    // `estimateFocusMinutes(runtime, { args: {...} })` plain function.
    let (status, estimate) = call(
        "POST",
        "/api/$procs/estimateFocusMinutes".to_owned(),
        serde_json::json!({ "args": { "taskCount": 4, "minutesPerTask": 25 } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{estimate:?}");
    assert_eq!(estimate["totalMinutes"], 100);
}
