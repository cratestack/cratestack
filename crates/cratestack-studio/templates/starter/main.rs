//! Minimal CrateStack Studio server. Loads `studio.toml`, mounts the
//! admin API + bundled Leptos UI at `/`, and serves until Ctrl-C.
//!
//! Customize freely — `cratestack_studio::server::build_router`
//! returns the underlying `axum::Router` if you want to layer in your
//! own routes, middleware, or auth.

use std::net::SocketAddr;
use std::path::PathBuf;

use cratestack_studio::{ServerOptions, run};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    run(ServerOptions {
        config_path: PathBuf::from("studio.toml"),
        bind: "127.0.0.1:7878".parse::<SocketAddr>()?,
    })
    .await?;
    Ok(())
}
