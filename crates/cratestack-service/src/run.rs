//! Serve a router with request tracing installed.

use cratestack_core::CoolError;
use tower_http::trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::ServiceConfig;

/// Install a request/response tracing layer at `INFO` (this crate offers
/// no separate knob for it — crank a noisy deployment down with
/// `RUST_LOG=info,tower_http=warn`, [`telemetry::DEFAULT_FILTER`]'s
/// default) and serve `router` on `config.bind_addr()` until the process
/// is killed or the listener errors.
///
/// [`telemetry::DEFAULT_FILTER`]: crate::telemetry::DEFAULT_FILTER
pub async fn run(router: axum::Router, config: &ServiceConfig) -> Result<(), CoolError> {
    let addr = config.bind_addr();
    let router = router.layer(
        TraceLayer::new_for_http()
            .on_request(DefaultOnRequest::new().level(Level::INFO))
            .on_response(DefaultOnResponse::new().level(Level::INFO)),
    );

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|error| CoolError::Internal(format!("failed to bind {addr}: {error}")))?;

    tracing::info!(service = %config.service_name, %addr, "listening");

    axum::serve(listener, router)
        .await
        .map_err(|error| CoolError::Internal(error.to_string()))
}
