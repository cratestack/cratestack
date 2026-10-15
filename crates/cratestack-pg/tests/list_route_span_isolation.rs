//! cratestack#1081: the generated model list route must not hold its tracing
//! span entered across an `.await`. Auth is the first `.await` in that route,
//! so the test suspends `authenticate` on a oneshot gate. An unrelated task
//! spawned on the same current-thread runtime runs during that suspension,
//! records `Span::current()`, and releases the gate.
//!
//! No database is needed. The pool is lazy and is never successfully
//! connected: the gate is released before the route reaches its first query,
//! and any later query error only changes the response, which this test does
//! not assert on. This keeps the check running in every environment, where
//! the rest of this crate's PG-backed tests skip without a database.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use cratestack::axum::body::Body;
use cratestack::axum::http::Request;
use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{AuthProvider, CratestackCodec, CratestackContext, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use tokio::sync::oneshot;
use tower::util::ServiceExt;
use tracing::Id;
use tracing::span::Attributes;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

include_server_schema!("tests/fixtures/span_isolation_list.cstack", db = Postgres);

/// Names of every span created while the dispatcher is installed.
#[derive(Clone, Default)]
struct SpanNameRecorder(Arc<Mutex<Vec<&'static str>>>);

impl<S: tracing::Subscriber> Layer<S> for SpanNameRecorder {
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

/// An `AuthProvider` whose `authenticate` suspends on a gate before answering.
#[derive(Clone)]
struct GatedAuthProvider {
    gate: Arc<Mutex<Option<oneshot::Receiver<()>>>>,
}

impl AuthProvider for GatedAuthProvider {
    type Error = cratestack::CratestackError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let gate = self
            .gate
            .lock()
            .expect("gate mutex should not be poisoned")
            .take();
        async move {
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            Ok(CratestackContext::authenticated([(
                "id".to_owned(),
                Value::Int(1),
            )]))
        }
    }
}

#[tokio::test]
async fn model_list_route_does_not_leak_its_span_across_an_await() {
    let recorder = SpanNameRecorder::default();
    let subscriber = tracing_subscriber::registry().with(recorder.clone());
    let _dispatch = tracing::subscriber::set_default(subscriber);

    let (release, gate) = oneshot::channel::<()>();
    let observer = tokio::spawn(async move {
        let observed = tracing::Span::current()
            .metadata()
            .map(|metadata| metadata.name());
        let _ = release.send(());
        observed
    });

    // Lazy: nothing connects until a query runs, and the gate is released
    // before that. The short acquire timeout only bounds the failure path.
    let pool = PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(200))
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .expect("a lazy pool should build without connecting");
    let router = cratestack_schema::axum::model_router(
        cratestack_schema::Cratestack::builder(pool).build(),
        (),
        JsonCodec,
        GatedAuthProvider {
            gate: Arc::new(Mutex::new(Some(gate))),
        },
    );

    let _response = router
        .oneshot(
            Request::get("/span_isolation_items")
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("route request should complete");

    let observed = observer.await.expect("observer task should not panic");
    assert_eq!(
        observed, None,
        "an unrelated task polled during a suspended model list route saw a span as current"
    );
    assert!(
        recorder.names().contains(&"cratestack_model_list_route"),
        "the model list route span should have been created: {:?}",
        recorder.names()
    );
}
