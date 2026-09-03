//! Op resolver for RPC transport: recover the `OpAdmission` facts the
//! schema declared about the op named in a `/rpc/{op_id}` path.
//!
//! Deliberately shaped after `crate::ratelimit::rpc_ops_filter`, which
//! solved the identical lookup for cratestack#474 — same `/rpc/` prefix
//! strip, same `batch`/`subscribe/` exclusions, same linear search over an
//! unsorted slice. See [`build_rest_op_resolver`] for why this module's
//! fail-closed direction is the inverse of the rate-limit filter's.
//!
//! [`build_rest_op_resolver`]: super::build_rest_op_resolver
//!
//! # A nested mount needs [`build_rpc_op_resolver_with_prefix`]
//!
//! This reads the raw request path, so under `Router::nest("/api", router)`
//! it sees `/api/rpc/procedure.notify`, fails the `/rpc/` prefix test, and
//! resolves everything unresolved — safe, but `@no_idempotency` silently
//! does nothing. Pass the mount point in. See `super::mount_prefix`.
//!
//! # `/rpc/batch` is not per-op, and the consequence is benign here
//!
//! `POST /rpc/batch` carries a sequence of ops in one body
//! (`docs/design/rpc-transport.md`). This resolver runs before the body is
//! decoded, so it cannot see inside — exactly the limitation
//! `rpc_ops_filter` documents. For rate limiting that means a batch is
//! always throttled wholesale; here it means a batch always **reserves**,
//! which is the conservative answer and is also what a batch does today
//! with no resolver installed at all. An op author who needs
//! `@no_idempotency` honoured must call it at `/rpc/{op_id}`.
//!
//! `/rpc/subscribe/{op_id}` is excluded for the same reason it is
//! excluded there: it is a framework dispatch point, not an op invocation.
//! It is also a `GET`, so `is_idempotent_target_method` has already
//! short-circuited it long before this resolver runs — the exclusion is
//! belt-and-braces, kept for symmetry with the filter it mirrors.

use axum::extract::Request;
use cratestack_core::OpDescriptor;
use cratestack_exec::OpAdmission;

use super::mount_prefix;

/// Build an op resolver for `transport rpc` schemas, over the generated
/// `OPS` slice, for a router mounted at the root.
///
/// Returns [`OpAdmission::unresolved`] — which reserves — for a non-RPC
/// path, for `/rpc/batch`, for `/rpc/subscribe/...`, and for any op id
/// absent from `ops`.
///
/// **If the router is nested, use [`build_rpc_op_resolver_with_prefix`].**
pub fn build_rpc_op_resolver(
    ops: &'static [OpDescriptor],
) -> impl Fn(&Request) -> OpAdmission + Send + Sync {
    build_rpc_op_resolver_with_prefix("", ops)
}

/// [`build_rpc_op_resolver`] for a router mounted under `prefix`, e.g.
/// `build_rpc_op_resolver_with_prefix("/api", OPS)` to match
/// `Router::nest("/api", router)`.
///
/// Same forgiving-spelling / strict-boundary rules as the REST twin.
pub fn build_rpc_op_resolver_with_prefix(
    prefix: &str,
    ops: &'static [OpDescriptor],
) -> impl Fn(&Request) -> OpAdmission + Send + Sync {
    let prefix = mount_prefix::normalize(prefix);
    move |req: &Request| {
        let Some(path) = mount_prefix::strip(req.uri().path(), &prefix) else {
            return OpAdmission::unresolved();
        };
        let Some(op_id) = path.strip_prefix("/rpc/") else {
            return OpAdmission::unresolved();
        };
        if op_id == "batch" || op_id.starts_with("subscribe/") {
            return OpAdmission::unresolved();
        }

        ops.iter()
            .find(|op| op.op_id == op_id)
            .map_or_else(OpAdmission::unresolved, OpAdmission::from)
    }
}
