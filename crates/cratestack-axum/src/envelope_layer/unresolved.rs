//! A request the resolver did not bind to an op (decisions D6 and S2).

use std::time::Duration;

use axum::extract::Request;
use axum::response::Response;
use cratestack_core::log_throttle::{LogThrottle, ThrottleDecision};

use super::layer::Config;
use super::mode::EnvelopeMode;
use super::refusal;
use super::resolver::Resolution;
use super::service::{Inner, call};
use crate::idempotency::mount_prefix;

/// Throttled: every request to a misconfigured layer hits it, and the
/// request rate is the caller's to choose.
static MISCONFIGURED: LogThrottle = LogThrottle::new(Duration::from_secs(60));

/// - A COSE body: the unsigned `415`, always (nothing to bind it to).
/// - [`Resolution::NotAnOp`], or no matched route (a `404`): untouched.
/// - A route in the allow-list: untouched.
/// - Otherwise, when the policy's `unresolved_mode` is `Required`, fail
///   closed (S2): a generated path with another method gets the layer's own
///   `405`, and a matched route nobody resolved gets an unsigned `500`
///   ("envelope misconfigured", logged). Unsigned because there is no op,
///   so no binding a signature could be made against, and because it is
///   decided before any verification, like the other refusals (D4).
/// - Otherwise (`Optional`, `Off`) untouched, as D6 had it, with a warning
///   once per process for a matched route (never for a `405`).
pub(super) async fn handle<S: Inner>(
    config: &Config,
    inner: S,
    request: Request,
    enveloped: bool,
    matched: Option<String>,
    resolution: Resolution,
) -> Response {
    let (headers, path) = (request.headers(), request.uri().path());
    if enveloped {
        return refusal::unsupported_envelope(headers, path);
    }
    let strict = config.policy.unresolved_mode() == EnvelopeMode::Required;
    let allowed = matched
        .as_deref()
        .and_then(|matched| mount_prefix::strip(matched, &config.mount_prefix))
        .is_some_and(|template| config.allow_unresolved.iter().any(|a| a == template));
    match resolution {
        Resolution::MethodNotAllowed(allow) if strict && !allowed => {
            refusal::method_not_allowed(headers, path, &allow)
        }
        Resolution::Unresolved if matched.is_some() && !allowed => {
            let matched = matched.as_deref().unwrap_or("");
            if strict {
                log_misconfigured(matched, &config.mount_prefix);
                return refusal::plain_internal(headers, path);
            }
            warn_unresolved_once();
            call(inner, request).await
        }
        _ => call(inner, request).await,
    }
}

fn log_misconfigured(matched: &str, prefix: &str) {
    if let ThrottleDecision::Emit {
        suppressed_since_last,
    } = MISCONFIGURED.check()
    {
        tracing::error!(
            target: "cratestack",
            cratestack_operation = "envelope",
            matched_route = matched,
            mount_prefix = prefix,
            suppressed_since_last,
            "envelope misconfigured: the router matched a route the binding resolver does not \
             know, under a Required policy, so the request was refused (500). Give the layer \
             the router's mount prefix (rest(\"/api\", ..) / rpc(\"/api\") / mount_prefix), or \
             allow_unresolved([..]) a hand-written route.",
        );
    }
}

/// A matched route the resolver does not know under a policy that is not
/// `Required`: legitimate for a hand-written route merged into the
/// generated router before the layer, but also what a missing or wrong
/// mount prefix looks like. Logged once per process.
fn warn_unresolved_once() {
    static WARNING: std::sync::Once = std::sync::Once::new();
    WARNING.call_once(|| {
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "envelope",
            "the envelope layer saw a matched route its binding resolver does not know, and \
             let it through unsigned. If the generated router is nested, give the layer the \
             same prefix (rest(\"/api\", ..) / rpc(\"/api\")); list hand-written routes with \
             allow_unresolved([..]). Logged once per process.",
        );
    });
}
