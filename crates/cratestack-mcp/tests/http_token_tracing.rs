//! cratestack#1039: a bearer token never appears in `tracing` output.
//!
//! One test in its own binary, because it installs a *global* subscriber at
//! `TRACE` for every target: `rmcp` runs the handler on spawned tasks, which
//! a thread-local default would not see. Every event from this crate,
//! `rmcp` and anything else in the process lands in one buffer, and no
//! token used here may be in it, including one a provider quoted in its own
//! error message.

mod support;

use std::io;
use std::sync::{Arc, Mutex};

use cratestack_core::{CratestackContext, CratestackError};
use cratestack_mcp::{ProtectedResource, StreamableHttpServer};
use http::{HeaderMap, StatusCode};
use serde_json::json;
use support::FakeTools;
use support::http_app::{APP_ORIGIN, RESOURCE, keyed_call, send, served, token};
use support::token::{ISSUER, mint};
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Captured {
        self.clone()
    }
}

/// A provider that puts the token in its error detail, which a careless
/// real one might. The detail is logged (it is how an operator learns why
/// tokens fail), so the guard has to cut the token out of it first.
fn quoting_provider(headers: &HeaderMap) -> Result<CratestackContext, CratestackError> {
    let presented = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    Err(CratestackError::Unauthorized(format!(
        "rejected credential `{presented}`"
    )))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_token_reaches_the_logs() {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(captured.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("the only subscriber");

    let tools = FakeTools::default();
    let app = served(&tools);
    let good = token("u-1");
    let foreign = mint("http://other.example/mcp", json!({ "id": "u-1" }));
    let quoted = token("u-2");

    let ok = send(
        &app,
        keyed_call(
            "/mcp",
            Some(&good),
            "transfer",
            json!({ "amount": 1 }),
            None,
        ),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    let failed = send(
        &app,
        keyed_call(
            "/mcp",
            Some(&good),
            "transfer",
            json!({ "amount": -1 }),
            None,
        ),
    )
    .await;
    assert_eq!(failed.result()["isError"], json!(true));
    let refused = send(
        &app,
        keyed_call("/mcp", Some(&foreign), "echo", json!({ "text": "a" }), None),
    )
    .await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);

    let quoting = StreamableHttpServer::builder(
        tools.clone(),
        quoting_provider,
        [APP_ORIGIN],
        ProtectedResource::new(RESOURCE, [ISSUER]),
    )
    .build()
    .unwrap();
    let app = axum::Router::new().nest_service("/mcp", quoting.service());
    let rejected = send(
        &app,
        keyed_call("/mcp", Some(&quoted), "echo", json!({ "text": "a" }), None),
    )
    .await;
    assert_eq!(rejected.status, StatusCode::UNAUTHORIZED);

    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    // The capture is live across every layer, or its silence proves nothing:
    // the guard's refusal, the tool call on `rmcp`'s task, and `rmcp` itself.
    assert!(logs.contains("mcp: request refused"), "{logs}");
    assert!(
        logs.contains("cratestack mcp tool call completed"),
        "{logs}"
    );
    assert!(
        logs.contains("rejected credential `Bearer [redacted]`"),
        "{logs}"
    );
    assert!(
        logs.contains("rmcp"),
        "no event from rmcp was captured:\n{logs}"
    );

    for secret in [&good, &foreign, &quoted] {
        let signature = secret.rsplit('.').next().unwrap();
        assert!(
            !logs.contains(secret.as_str()),
            "a token was logged:\n{logs}"
        );
        assert!(
            !logs.contains(signature),
            "a token signature was logged:\n{logs}"
        );
    }
}
