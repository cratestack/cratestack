//! The per-request body of [`super::RateLimitService::call`], moved out of
//! `layer.rs` (which was at 194 of its 200 permitted lines) when
//! cratestack#871 added the bucket-budget step.

use std::sync::Arc;

use axum::extract::Request;
use axum::response::Response;
use cratestack_core::{BoundedOutcome, Charged, ConsumeRequest, RateLimitDecision};

use crate::middleware_error::middleware_error_response;

use super::budget::warn::BudgetWarnings;
use super::decision::{key_failure_response, throttled_response, with_budget_headers};
use super::scope::{BudgetScope, KeyDerivation};
use super::service::RateLimitService;
use super::store_error::{StoreFailure, classify_store_failure};

/// Runs the limiter for one request that is not exempt, and returns the
/// response — either the inner service's, or one of the layer's own.
pub(super) async fn run<S>(service: RateLimitService<S>, req: Request) -> Response
where
    S: tower::Service<Request, Response = Response, Error = std::convert::Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    let mut inner = service.inner;
    let config = service.config;

    let derivation: KeyDerivation = match (service.key_fn)(&req) {
        Ok(derivation) => derivation,
        Err(error) => return key_failure_response(&req, error),
    };

    // ONE budget for the whole lookup, retry included: the store
    // is free to retry internally, but the caller must not pay
    // for it twice. An elapse is reported as a transport-class
    // error, so it is subject to the same policy as any other
    // "the store did not answer" — cratestack#846.
    let request = ConsumeRequest::new(&derivation.key, config, derivation.budget.as_ref());
    let outcome = match tokio::time::timeout(
        service.store_timeout,
        service.store.consume_bounded(request),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(_elapsed) => Err(super::policy::store_timeout_error()),
    };

    match outcome {
        Ok(BoundedOutcome {
            decision, charged, ..
        }) => {
            report(charged, &derivation, &service.budget_warnings);

            match decision {
                RateLimitDecision::Allowed { remaining } => {
                    let response = match inner.call(req).await {
                        Ok(response) => response,
                        Err(infallible) => match infallible {},
                    };
                    with_budget_headers(response, config, remaining)
                }
                RateLimitDecision::Throttled { retry_after_secs } => {
                    throttled_response(req.headers(), req.uri().path(), retry_after_secs)
                }
            }
        }
        Err(error) => {
            match classify_store_failure(error, service.store_error_policy, &service.warnings) {
                StoreFailure::Serve => match inner.call(req).await {
                    Ok(response) => response,
                    Err(infallible) => match infallible {},
                },
                StoreFailure::Refuse(error) => {
                    middleware_error_response(req.headers(), req.uri().path(), error)
                }
            }
        }
    }
}

/// Turn the store's [`Charged`] report into a log line, and refine
/// `Fallback` into `Overflow` for the process-global scope.
///
/// The refinement lives here, not in the store: a store is handed a
/// [`cratestack_core::BucketBudget`] and cannot tell a per-peer scope from
/// the global one, and teaching every backend that rule would be three
/// copies of it. The layer *chose* the scope, so it already knows.
///
/// Returns whether a line was emitted — the throttles make that
/// observable without a `tracing` subscriber, which is what the
/// cratestack#871 tests assert on.
pub(super) fn report(
    charged: Charged,
    derivation: &KeyDerivation,
    warnings: &Arc<BudgetWarnings>,
) -> bool {
    match charged {
        Charged::Requested => false,
        Charged::Unbounded => {
            // Only worth saying when a bound was actually asked for. A
            // store that never sees a budget (a `with_key_fn` override,
            // a verified principal) is not failing to honour anything.
            derivation.budget.is_some() && warnings.unbounded_store()
        }
        Charged::Fallback | Charged::Overflow => match derivation.scope {
            Some(BudgetScope::Global) => warnings.overflow(),
            _ => warnings.fallback(
                derivation
                    .budget
                    .as_ref()
                    .map_or("<none>", |budget| budget.scope_key.as_str()),
            ),
        },
        // `Charged` is `#[non_exhaustive]`. A variant this build has never
        // heard of says nothing actionable about THIS layer's budget, so
        // stay quiet rather than mislabel it as one of the cases above.
        _ => false,
    }
}
