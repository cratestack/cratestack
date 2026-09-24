//! The per-request body of [`super::RateLimitService::call`], moved out of
//! `layer.rs` (which was at 194 of its 200 permitted lines) when
//! cratestack#871 added the bucket-budget step.

use std::sync::Arc;

use axum::extract::Request;
use axum::response::Response;
use cratestack_core::{BoundedOutcome, Charged, CratestackError, RateLimitDecision};
use cratestack_exec::{OpAdmission, OpInput, RateLimitAdmission, RateLimitBucket};

use crate::middleware_error::middleware_error_response;

use super::budget::warn::BudgetWarnings;
use super::decision::{key_failure_response, throttled_response, with_budget_headers};
use super::scope::{BudgetScope, KeyDerivation};
use super::service::RateLimitService;
use super::store_error::{StoreFailure, classify_store_failure};

/// Runs the limiter for one request that is not exempt, and returns the
/// response — either the inner service's, or one of the layer's own.
///
/// Since ADR 0015 slice 2 (cratestack#877) the store call goes through
/// [`cratestack_exec::OpExecutor::admit_rate_limit`]. Everything around it
/// is unchanged: key derivation before it, the single lookup budget around
/// it, and the response shaping after it all stayed at this layer.
pub(super) async fn run<S>(service: RateLimitService<S>, req: Request, op: OpAdmission) -> Response
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
    let input = OpInput::for_rate_limit(
        op,
        RateLimitBucket::new(&derivation.key, derivation.budget.as_ref()),
    );
    let admitted = match tokio::time::timeout(
        service.store_timeout,
        service.executor.admit_rate_limit(&input),
    )
    .await
    {
        Ok(admitted) => admitted,
        Err(_elapsed) => Err(super::policy::store_timeout_error()),
    };
    let outcome = match admitted {
        Ok(RateLimitAdmission::Consumed(outcome)) => Ok(outcome),
        // Unreachable while `RateLimitService::call` asks
        // `rate_limit_applies` first; if it ever is reached, serve as the
        // exempt path does rather than invent a charge. Reaching it means
        // the two executor answers disagree — loud in debug builds, because
        // serving here masks exactly that regression from the end-to-end
        // suites (cratestack#877 review, mutation c).
        Ok(RateLimitAdmission::Bypass) => {
            debug_assert!(
                !service.executor.rate_limit_applies(&input.op),
                "rate_limit_applies charged this op but admit_rate_limit bypassed it"
            );
            return match inner.call(req).await {
                Ok(response) => response,
                Err(infallible) => match infallible {},
            };
        }
        // `RateLimitAdmission` is `#[non_exhaustive]`. An outcome this
        // build does not know must not run the handler.
        Ok(_) => {
            return middleware_error_response(
                req.headers(),
                req.uri().path(),
                CratestackError::Internal(
                    "rate limit: unhandled admission outcome; refusing rather than running \
                     the operation"
                        .to_owned(),
                ),
            );
        }
        Err(error) => Err(error),
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
