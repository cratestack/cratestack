//! Server entry point — see `lib.rs` for the schema, procedure, and
//! `build_router`/`ensure_schema`; see `tests/smoke.rs` for the wire-shape
//! demos; see `web/` for the `--preset swr` client consuming this server.

use react_vite_swr_example::{build_router, ensure_schema};
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://cratestack:cratestack@localhost:55432/cratestack_test".to_owned()
    });

    let pool = cratestack::sqlx::PgPool::connect(&database_url)
        .await
        .expect("connect to Postgres — see DATABASE_URL (compose.yml maps port 55432)");
    ensure_schema(&pool)
        .await
        .expect("create boards/tasks tables");

    let db = react_vite_swr_example::schema::Cratestack::builder(pool).build();
    let app = build_router(db);

    let addr: SocketAddr = "127.0.0.1:3210".parse().expect("addr parses");
    println!("react-vite-swr-server listening on http://{addr}");
    println!("routes mounted under /api — try:");
    println!("  curl -H 'x-auth-id: 1' http://{addr}/api/boards");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind 127.0.0.1:3210");
    cratestack::axum::serve(listener, app)
        .await
        .expect("axum serve");
}
