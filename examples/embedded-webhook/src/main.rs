//! `cratestack-webhook` — a single-binary HTTP webhook receiver that owns
//! its own SQLite. Useful for the "edge service with state" deployment where
//! the operational cost of a Postgres pair would dwarf the workload.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use cratestack_rusqlite::RusqliteRuntime;
use embedded_webhook_example::{AppState, bootstrap, build_router};
use tokio::net::TcpListener;
use tokio::signal;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "cratestack-webhook",
    about = "Embedded-SQLite-backed HTTP webhook receiver",
    version
)]
struct Cli {
    /// Address to bind the HTTP server.
    #[arg(long, default_value = "127.0.0.1:8080")]
    bind: SocketAddr,

    /// SQLite database file. Created on first run.
    #[arg(long, default_value = "webhooks.db")]
    db: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let runtime = Arc::new(
        RusqliteRuntime::open(&cli.db).with_context(|| format!("opening {}", cli.db.display()))?,
    );
    bootstrap(&runtime).context("bootstrap schema")?;
    let app = build_router(AppState { runtime });

    let listener = TcpListener::bind(cli.bind)
        .await
        .with_context(|| format!("binding {}", cli.bind))?;
    info!(bind = %cli.bind, db = %cli.db.display(), "webhook server started");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = signal::ctrl_c().await;
            info!("ctrl-c received, shutting down");
        })
        .await?;
    Ok(())
}
