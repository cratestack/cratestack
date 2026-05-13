//! `cratestack-daemon` — a long-running process that watches a directory and
//! persists filesystem events through `include_embedded_schema!`. This is the
//! reference shape for "tokio async I/O on the outside, sync ModelDelegate on
//! the inside": the persistence call goes through `tokio::task::spawn_blocking`
//! so the rusqlite mutex never starves the tokio worker pool.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use chrono::Utc;
use clap::Parser;
use cratestack_rusqlite::RusqliteRuntime;
use embedded_daemon_example::{Debouncer, ReadyEvent, bootstrap, persist_event};
use notify::event::{EventKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::signal;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "cratestack-daemon",
    about = "Filesystem-watcher daemon that persists debounced events through cratestack-rusqlite",
    version
)]
struct Cli {
    /// Directory to watch (recursive).
    #[arg(long)]
    watch: PathBuf,

    /// SQLite database file. Created on first run.
    #[arg(long, default_value = "events.db")]
    db: PathBuf,

    /// Quiet window in milliseconds — events on the same path within this
    /// gap are collapsed into one row.
    #[arg(long, default_value_t = 250)]
    window_ms: u64,

    /// How often the debouncer task wakes up to flush ready entries.
    #[arg(long, default_value_t = 100)]
    tick_ms: u64,
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
        RusqliteRuntime::open(&cli.db)
            .with_context(|| format!("opening {}", cli.db.display()))?,
    );
    bootstrap(&runtime).context("bootstrap schema")?;

    let (raw_tx, raw_rx) = mpsc::unbounded_channel::<RawEvent>();
    let watch_path = cli.watch.clone();
    let mut watcher = build_watcher(raw_tx)?;
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .with_context(|| format!("watching {}", watch_path.display()))?;
    info!(path = %watch_path.display(), db = %cli.db.display(), "daemon started");

    let window = Duration::from_millis(cli.window_ms);
    let tick = Duration::from_millis(cli.tick_ms);
    run(runtime, raw_rx, window, tick).await;

    // dropping the watcher stops the underlying OS notifier
    drop(watcher);
    info!("daemon stopped");
    Ok(())
}

#[derive(Debug)]
struct RawEvent {
    path: PathBuf,
    kind: &'static str,
    observed: Instant,
}

fn build_watcher(tx: mpsc::UnboundedSender<RawEvent>) -> anyhow::Result<RecommendedWatcher> {
    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        match res {
            Ok(event) => {
                let kind = match event.kind {
                    EventKind::Create(_) => Some("created"),
                    EventKind::Modify(ModifyKind::Name(_)) => Some("renamed"),
                    EventKind::Modify(_) => Some("modified"),
                    EventKind::Remove(_) => Some("deleted"),
                    _ => None,
                };
                let Some(kind) = kind else {
                    return;
                };
                for path in event.paths {
                    let _ = tx.send(RawEvent {
                        path,
                        kind,
                        observed: Instant::now(),
                    });
                }
            }
            Err(error) => {
                warn!(%error, "notify watcher reported an error");
            }
        }
    })?;
    Ok(watcher)
}

async fn run(
    runtime: Arc<RusqliteRuntime>,
    mut raw_rx: mpsc::UnboundedReceiver<RawEvent>,
    window: Duration,
    tick: Duration,
) {
    let mut debouncer = Debouncer::new(window);
    let mut ticker = interval(tick);

    loop {
        tokio::select! {
            biased;
            _ = signal::ctrl_c() => {
                info!("ctrl-c received, draining {} pending entries", count_pending(&debouncer));
                let ready = debouncer.drain_all(Utc::now());
                flush_batch(&runtime, ready).await;
                break;
            }
            maybe = raw_rx.recv() => {
                match maybe {
                    Some(event) => {
                        debug!(path = %event.path.display(), kind = event.kind, "raw event");
                        debouncer.observe(event.path, event.kind, event.observed);
                    }
                    None => {
                        info!("event channel closed, draining and exiting");
                        let ready = debouncer.drain_all(Utc::now());
                        flush_batch(&runtime, ready).await;
                        break;
                    }
                }
            }
            _ = ticker.tick() => {
                let ready = debouncer.drain_ready(Instant::now(), Utc::now());
                if !ready.is_empty() {
                    flush_batch(&runtime, ready).await;
                }
            }
        }
    }
}

fn count_pending(deb: &Debouncer) -> &'static str {
    // Avoid exposing the internals — show "empty" vs "non-empty".
    if deb.is_empty() { "0" } else { "≥1" }
}

async fn flush_batch(runtime: &Arc<RusqliteRuntime>, batch: Vec<ReadyEvent>) {
    if batch.is_empty() {
        return;
    }
    let runtime = Arc::clone(runtime);
    let count = batch.len();
    let result = tokio::task::spawn_blocking(move || {
        for event in batch {
            persist_event(&runtime, event)?;
        }
        Ok::<_, cratestack_rusqlite::RusqliteError>(())
    })
    .await;
    match result {
        Ok(Ok(())) => info!(count, "persisted batch"),
        Ok(Err(error)) => error!(%error, "rusqlite error while persisting batch"),
        Err(join_error) => error!(%join_error, "spawn_blocking task panicked"),
    }
}

#[allow(dead_code)]
fn _ensure_path_is_dir(path: &Path) -> anyhow::Result<()> {
    if !path.is_dir() {
        anyhow::bail!("{} is not a directory", path.display());
    }
    Ok(())
}
