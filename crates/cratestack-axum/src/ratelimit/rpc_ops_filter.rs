//! Rate-limit filter for RPC transport: check if an operation is exempt
//! from rate limiting based on its `OpDescriptor.rate_limited_by_default`.
//!
//! # Known limitation: `/rpc/batch` is not per-op
//!
//! `POST /rpc/batch` (`docs/design/rpc-transport.md`) carries a *sequence*
//! of ops in one request body. This filter runs before the body is
//! decoded — it only sees the HTTP-layer path — so it cannot look inside
//! a batch to exempt individual ops the way it can for `/rpc/{op_id}`.
//! **Accepted tradeoff, not a bug**: `/rpc/batch` is therefore always
//! rate-limited wholesale, regardless of whether every op it contains is
//! `@no_rate_limit`. An op author who needs the exemption to hold
//! unconditionally must call it via `/rpc/{op_id}`, not batch it. Making
//! batch itself descriptor-aware would require decoding + re-encoding the
//! batch body inside this HTTP-layer filter (or moving enforcement into
//! the batch dispatcher itself), which is out of scope here — see
//! cratestack#474's discussion for the full reasoning.

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
///
/// `/rpc/batch` is always rate-limited regardless of its contents — see
/// the module-level "Known limitation" doc above.
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

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use cratestack_core::{OpDescriptor, OpKind};

    use super::build_rpc_ops_filter;

    const OPS: &[OpDescriptor] = &[
        OpDescriptor {
            op_id: "procedure.createPayment",
            kind: OpKind::Unary,
            input_ty: "PingArgs",
            output_ty: "PingArgs",
            idempotent_by_default: false,
            rate_limited_by_default: false,
            auth_required: true,
        },
        OpDescriptor {
            op_id: "procedure.ping",
            kind: OpKind::Unary,
            input_ty: "PingArgs",
            output_ty: "PingArgs",
            idempotent_by_default: true,
            rate_limited_by_default: true,
            auth_required: true,
        },
    ];

    fn get(uri: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .body(Body::empty())
            .expect("request should build")
    }

    #[test]
    fn exempts_no_rate_limit_op_and_throttles_ordinary_op() {
        let filter = build_rpc_ops_filter(OPS);

        assert!(
            !filter(&get("/rpc/procedure.createPayment")),
            "op with rate_limited_by_default: false should be exempt"
        );
        assert!(
            filter(&get("/rpc/procedure.ping")),
            "op with rate_limited_by_default: true should be rate-limited"
        );
    }

    #[test]
    fn batch_and_subscribe_are_always_rate_limited() {
        let filter = build_rpc_ops_filter(OPS);

        assert!(
            filter(&get("/rpc/batch")),
            "/rpc/batch is a framework dispatch point, always rate-limited \
             (see module docs: it can't see per-op exemptions inside the batch body)"
        );
        assert!(
            filter(&get("/rpc/subscribe/model.Widget.subscribe")),
            "/rpc/subscribe/* is a framework dispatch point, always rate-limited"
        );
    }

    #[test]
    fn unknown_op_and_non_rpc_path_fail_closed() {
        let filter = build_rpc_ops_filter(OPS);

        assert!(
            filter(&get("/rpc/procedure.doesNotExist")),
            "an op missing from the descriptor array must fail closed (rate-limited)"
        );
        assert!(
            filter(&get("/api/widgets")),
            "a non-RPC path must fail closed (rate-limited)"
        );
    }
}
