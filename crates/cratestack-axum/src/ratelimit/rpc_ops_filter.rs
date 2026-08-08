//! Rate-limit filter for RPC transport: check if an operation is exempt
//! from rate limiting based on its `OpDescriptor.rate_limited_by_default`.

use axum::extract::Request;
use cratestack_core::OpDescriptor;

/// Build a rate-limit filter function for RPC schemas.
///
/// Returns a function that:
/// - Extracts `op_id` from `/rpc/{op_id}` paths
/// - Looks up the op in the provided descriptors
/// - Returns `false` (exempt) if `rate_limited_by_default` is false
/// - Returns `true` (apply rate limit) if the op participates, or if lookup fails
///
/// Fails closed: if descriptor lookup misses for any reason, the op is
/// rate-limited. This prevents accidental exemptions from missing data.
pub fn build_rpc_ops_filter(
    ops: &'static [OpDescriptor],
) -> impl Fn(&Request) -> bool + Send + Sync {
    move |req: &Request| {
        let path = req.uri().path();

        // Only apply descriptor lookup to `/rpc/{op_id}` paths.
        if !path.starts_with("/rpc/") {
            // Not an RPC path; default to rate-limit.
            return true;
        }

        // Extract op_id from `/rpc/{op_id}` (strip `/rpc/` prefix).
        // Note: `/rpc/batch` and `/rpc/subscribe/{op_id}` are handled separately
        // by RPC dispatch and subscription endpoints, not generic ops. Only
        // unary ops live at `/rpc/{op_id}`.
        let op_id = &path[5..]; // skip "/rpc/"

        // If the path is `/rpc/batch` or `/rpc/subscribe/...`, those are
        // framework dispatch points, not op invocations. Rate-limit them.
        if op_id == "batch" || op_id.starts_with("subscribe/") {
            return true;
        }

        // Look up the op in the descriptor array.
        // Note: The array is not sorted, so we use linear search.
        match ops.iter().find(|op| op.op_id == op_id) {
            Some(op) => {
                // Op found. Return whether it should be rate-limited.
                op.rate_limited_by_default
            }
            None => {
                // Op not found in descriptors. Fail closed: rate-limit it.
                // This could indicate a malformed op_id or a schema mismatch,
                // so treating it conservatively is correct.
                true
            }
        }
    }
}
