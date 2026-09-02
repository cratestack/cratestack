//! Deciding whether a request path addresses the RPC binding, so the
//! middleware error envelope can pick the matching `code` vocabulary.
//!
//! Split out of `middleware_error.rs` (cratestack#846 security review)
//! when the naive substring test grew into a real syntactic check.

/// True when this request is addressed to the RPC binding
/// (`/rpc/{op_id}`, `/rpc/batch`, `/rpc/subscribe/{op_id}` — see
/// `cratestack_core::rpc`'s `RPC_*_PATH`).
///
/// Matched on whole path *segments*, and not anchored at the start: the
/// RPC router is routinely nested under an application prefix
/// (`/api/rpc/...`), so a prefix test would hand those deployments the
/// REST vocabulary. A bare `contains("/rpc/")` is too loose in the other
/// direction — a REST collection legitimately named `rpc`
/// (`/accounts/rpc/transactions`) would take the RPC branch — and, note,
/// so is a naive segment scan, because `rpc` is the second-to-last
/// segment in *both* of those.
///
/// What actually separates them is the segment after `rpc`, which for a
/// real RPC call is one of the three shapes the binding defines: the
/// literal `batch`, `subscribe/<op id>`, or a dotted op id
/// (`model.User.list`, `procedure.publishPost` — see
/// `docs/design/rpc-transport.md` §3). `transactions` is none of those.
///
/// Still syntactic, so still fallible in principle: a REST route
/// `/accounts/rpc/foo.bar` would be misread. That costs a code string,
/// not a decode (see [`middleware_error_response`]), which is why this
/// stays a path test rather than growing a `MatchedPath` dependency.
pub(super) fn is_rpc_path(path: &str) -> bool {
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    while let Some(segment) = segments.next() {
        if segment != "rpc" {
            continue;
        }
        // `/rpc` with nothing after it addresses no op.
        let Some(next) = segments.next() else {
            return false;
        };
        if next == "batch" {
            return true;
        }
        let op_id = if next == "subscribe" {
            match segments.next() {
                Some(op_id) => op_id,
                None => return false,
            }
        } else {
            next
        };
        return is_dotted_op_id(op_id);
    }
    false
}

/// Every RPC op id is dotted (`model.<Model>.<verb>`,
/// `procedure.<name>`); no REST path segment in this framework's own
/// route naming ever is.
fn is_dotted_op_id(segment: &str) -> bool {
    segment.contains('.') && !segment.starts_with('.') && !segment.ends_with('.')
}
