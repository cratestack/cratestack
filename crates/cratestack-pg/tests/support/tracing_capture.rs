//! Shared tracing-event capture harness for tests that need to assert
//! on the exact SQL text a query emits — e.g. proving a route issues a
//! `COUNT(*)` aggregate rather than materialising rows
//! (cratestack#570). sqlx logs every statement via `tracing` at target
//! `"sqlx::query"` with a `db.statement` (or, for short queries, plain
//! `summary`) field carrying the full SQL text (see
//! `sqlx-core::logger::QueryLogger::finish`) — this captures that
//! (and any other tracing event) without touching production code.
//!
//! Mirrors the capture layer `include_schema.rs`'s
//! `generated_routes_emit_tracing_events` test already uses for
//! `cratestack`-target events; factored out here so a second test
//! binary doesn't have to reinvent it. Each integration test file
//! compiles to its own binary, so this module's process-wide
//! `TRACING_INIT`/`TEST_CAPTURE` statics are independent per binary —
//! duplicating the *harness* across binaries is fine and unavoidable;
//! what this factors out is having to reimplement it.

#![allow(dead_code)] // only the test binaries that opt in via `mod support;` use this

use std::future::Future;
use std::sync::{Arc, Mutex, Once};

use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;

tokio::task_local! {
    static TEST_CAPTURE: EventCaptureLayer;
}

static TRACING_INIT: Once = Once::new();

/// Installs the process-wide capture-dispatch subscriber exactly once.
/// Uses `set_global_default` (not `set_default`) so every worker thread
/// of the multi-threaded test runtime shares it — see cratestack#417
/// for why a thread-local default would poison tracing's one-time
/// callsite `Interest` cache for the other threads.
pub fn init_tracing() {
    TRACING_INIT.call_once(|| {
        let subscriber = tracing_subscriber::registry().with(GlobalCaptureLayer);
        tracing::subscriber::set_global_default(subscriber)
            .expect("global tracing subscriber should only be installed once");
    });
}

/// Runs `fut` with a fresh, task-local capture layer active, and
/// returns `fut`'s output alongside every `"<event name> field=value
/// ..."` line recorded for any tracing event fired on this task while
/// it ran — regardless of target or level. Call [`init_tracing`] once
/// beforehand.
pub async fn capture_events<F: Future>(fut: F) -> (F::Output, Vec<String>) {
    let capture = EventCaptureLayer::default();
    let output = TEST_CAPTURE.scope(capture.clone(), fut).await;
    (output, capture.snapshot())
}

#[derive(Clone, Copy)]
struct GlobalCaptureLayer;

impl<S: Subscriber> Layer<S> for GlobalCaptureLayer {
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let _ = TEST_CAPTURE.try_with(|capture| capture.on_event(event, ctx));
    }
}

#[derive(Clone, Default)]
struct EventCaptureLayer {
    events: Arc<Mutex<Vec<String>>>,
}

impl EventCaptureLayer {
    fn snapshot(&self) -> Vec<String> {
        self.events
            .lock()
            .expect("event capture mutex should not be poisoned")
            .clone()
    }
}

impl<S: Subscriber> Layer<S> for EventCaptureLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = TraceFieldVisitor::default();
        event.record(&mut visitor);
        self.events
            .lock()
            .expect("event capture mutex should not be poisoned")
            .push(format!(
                "{} {}",
                event.metadata().name(),
                visitor.fields.join(" ")
            ));
    }
}

#[derive(Default)]
struct TraceFieldVisitor {
    fields: Vec<String>,
}

impl Visit for TraceFieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields.push(format!("{}={value:?}", field.name()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.push(format!("{}={value}", field.name()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.push(format!("{}={value}", field.name()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields.push(format!("{}={value}", field.name()));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields.push(format!("{}={value}", field.name()));
    }
}
