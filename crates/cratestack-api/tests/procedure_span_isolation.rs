//! cratestack#1081: a generated procedure or route future must not hold its
//! tracing span entered across an `.await`. While such a future is suspended,
//! the executor polls other tasks on the same thread; if the span were still
//! entered, those tasks would observe it as `Span::current()` and their events
//! and child spans would be mis-parented under a procedure they have nothing
//! to do with.
//!
//! Each test suspends the procedure body on a gate. The body first signals
//! "reached" from inside the span, then awaits the gate. An unrelated task,
//! spawned on the same current-thread runtime, waits for that signal, records
//! `Span::current()`, and only then releases the gate. The observer therefore
//! reads `Span::current()` after the span exists and while the procedure
//! future is parked on the gate, and the body only continues once it has done
//! so. Waiting on the signal (rather than reading as soon as the task first
//! runs) keeps the test from passing vacuously if something ever yields
//! before the span is created. Each test also checks that the procedure span
//! was actually created, so an absent span cannot pass the "not the procedure
//! span" assertion vacuously. No sleeps: ordering comes from the channels.
//!
//! Needs no database: it uses the `db = None` fixture shared with
//! `no_database_procedures.rs`.

use std::sync::{Arc, Mutex};

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{CratestackCodec, CratestackContext, CratestackError};
use cratestack_codec_json::JsonCodec;
use tokio::sync::oneshot;
use tower::ServiceExt;
use tracing::span::Attributes;
use tracing::{Id, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

include_server_schema!("tests/fixtures/no_database_procedures.cstack", db = None);

/// Names of every span created while the dispatcher is installed.
#[derive(Clone, Default)]
struct SpanNameRecorder(Arc<Mutex<Vec<&'static str>>>);

impl<S: Subscriber> Layer<S> for SpanNameRecorder {
    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        self.0
            .lock()
            .expect("span name mutex should not be poisoned")
            .push(attrs.metadata().name());
    }
}

impl SpanNameRecorder {
    fn names(&self) -> Vec<&'static str> {
        self.0
            .lock()
            .expect("span name mutex should not be poisoned")
            .clone()
    }
}

/// Installs a thread-local dispatcher (every test here is current-thread)
/// that records span names. The guard must outlive the test body.
fn install_recorder() -> (SpanNameRecorder, tracing::subscriber::DefaultGuard) {
    let recorder = SpanNameRecorder::default();
    let subscriber = tracing_subscriber::registry().with(recorder.clone());
    let guard = tracing::subscriber::set_default(subscriber);
    (recorder, guard)
}

/// The procedure side of the gate: the body calls [`Gate::pass`] from inside
/// its span.
struct Gate {
    reached: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

impl Gate {
    /// Signals that the body is inside its span and about to suspend, then
    /// suspends until the observer has released the gate.
    async fn pass(self) {
        let _ = self.reached.send(());
        let _ = self.release.await;
    }
}

/// Spawns a task that waits until the body reports it reached the gate, reads
/// `Span::current()`, and then releases the gate. The name it reports is
/// `None` when no span is current on its thread. The task panics if the body
/// never reached the gate, so a body that bypasses it cannot pass vacuously.
fn gate_and_observer() -> (Gate, tokio::task::JoinHandle<Option<&'static str>>) {
    let (reached_tx, reached_rx) = oneshot::channel::<()>();
    let (release_tx, release_rx) = oneshot::channel::<()>();
    let observer = tokio::spawn(async move {
        reached_rx
            .await
            .expect("the gated body never reached its gate");
        let observed = tracing::Span::current()
            .metadata()
            .map(|metadata| metadata.name());
        let _ = release_tx.send(());
        observed
    });
    (
        Gate {
            reached: reached_tx,
            release: release_rx,
        },
        observer,
    )
}

/// A `ProcedureRegistry` whose `ping` suspends on a gate before answering.
#[derive(Clone)]
struct GatedProcedures {
    gate: Arc<Mutex<Option<Gate>>>,
}

impl cratestack_schema::procedures::ProcedureRegistry for GatedProcedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, CratestackError>,
    > + Send {
        let gate = self
            .gate
            .lock()
            .expect("gate mutex should not be poisoned")
            .take();
        async move {
            if let Some(gate) = gate {
                gate.pass().await;
            }
            Ok(cratestack_schema::PingReply {
                echo: args.args.message,
            })
        }
    }
}

/// Always authenticated, so the fixture's `@allow(auth() != null)` passes.
#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

fn ping_args() -> cratestack_schema::procedures::ping::Args {
    cratestack_schema::procedures::ping::Args {
        args: cratestack_schema::PingArgs {
            message: "hello".to_owned(),
        },
    }
}

fn authenticated_ctx() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), cratestack::Value::Int(1))])
}

#[tokio::test]
async fn invoke_with_db_does_not_leak_its_span_across_an_await() {
    let (recorder, _guard) = install_recorder();
    let db = cratestack_schema::Cratestack::builder().build();
    let args = ping_args();
    let ctx = authenticated_ctx();
    let (gate, observer) = gate_and_observer();
    let procedures = GatedProcedures {
        gate: Arc::new(Mutex::new(None)),
    };

    let call_args = args.clone();
    let call_ctx = ctx.clone();
    let output = cratestack_schema::procedures::ping::invoke_with_db(
        &db,
        &args,
        &ctx,
        |authorized| async move {
            gate.pass().await;
            cratestack_schema::procedures::ProcedureRegistry::ping(
                &procedures,
                &db,
                &call_ctx,
                call_args,
                authorized,
            )
            .await
        },
    )
    .await
    .expect("ping should succeed");
    assert_eq!(output.echo, "hello");

    let observed = observer.await.expect("observer task should not panic");
    assert_eq!(
        observed, None,
        "an unrelated task polled during a suspended invoke_with_db saw a span as current"
    );
    assert!(
        recorder
            .names()
            .contains(&"cratestack_procedure_invoke_with_db"),
        "the invoke_with_db span should have been created: {:?}",
        recorder.names()
    );
}

#[tokio::test]
async fn invoke_does_not_leak_its_span_across_an_await() {
    let (recorder, _guard) = install_recorder();
    let args = ping_args();
    let ctx = authenticated_ctx();
    let (gate, observer) = gate_and_observer();

    let output = cratestack_schema::procedures::ping::invoke(&args, &ctx, || async move {
        gate.pass().await;
        Ok(cratestack_schema::PingReply {
            echo: "hello".to_owned(),
        })
    })
    .await
    .expect("invoke should succeed");
    assert_eq!(output.echo, "hello");

    let observed = observer.await.expect("observer task should not panic");
    assert_eq!(
        observed, None,
        "an unrelated task polled during a suspended invoke saw a span as current"
    );
    assert!(
        recorder.names().contains(&"cratestack_procedure_invoke"),
        "the invoke span should have been created: {:?}",
        recorder.names()
    );
}

#[tokio::test]
async fn axum_procedure_route_does_not_leak_its_span_across_an_await() {
    let (recorder, _guard) = install_recorder();
    let (gate, observer) = gate_and_observer();
    let db = cratestack_schema::Cratestack::builder().build();
    let router = cratestack_schema::axum::router(
        db,
        GatedProcedures {
            gate: Arc::new(Mutex::new(Some(gate))),
        },
        (),
        JsonCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );

    let body = serde_json::json!({ "args": { "message": "hello" } });
    let response = router
        .oneshot(
            Request::post("/$procs/ping")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let observed = observer.await.expect("observer task should not panic");
    assert_eq!(
        observed, None,
        "an unrelated task polled during a suspended procedure route saw a span as current"
    );
    assert!(
        recorder.names().contains(&"cratestack_procedure_route"),
        "the procedure route span should have been created: {:?}",
        recorder.names()
    );
}
