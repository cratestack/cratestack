//! [`RpcBindingResolver`]: a `transport rpc` router's three routes.

use cratestack_core::rpc::{RPC_BATCH_PATH, RPC_SUBSCRIBE_PATH, RPC_UNARY_PATH};
use http::Method;

use super::resolver::{BindingResolver, Resolution, ResolvedRoute, RouteRequest};
use crate::idempotency::mount_prefix;

/// RPC: the op id axum decoded from `/rpc/{op_id}`, `batch` for
/// `/rpc/batch` (decision D11), and `subscribe/<op id>` for a subscription.
/// The op id and the body digest bind the call, so path parameters are
/// empty (ADR 0006 §4), except under a parameterised mount
/// (`nest("/t/{tenant}", ..)`), whose values are bound for the same reason
/// as [`super::RestBindingResolver`]'s.
///
/// **Query (for cratestack#1007's client):** an RPC call binds the
/// request's canonical query like a REST one does. Generated RPC clients
/// send none, so it is `null`; a client that adds one (a cache-buster, a
/// gateway's routing parameter) must bind it too, or the call is a `401`.
///
/// **The op id is validated, not looked up in `OPS`** (decision B2): the
/// unary template binds only an op id a generated op can have, one with a
/// `.` and no `/` that is not `batch`. Anything else (`/rpc/%62atch`, which
/// axum decodes to `batch`, or `/rpc/a%2Fb`) is [`Resolution::NotAnOp`]:
/// a COSE body is refused, a plain one passes to the router, whose `404` it
/// is. A well-formed but unknown op id is bound anyway, so the router's
/// `404` for it is sealed like any other error. Only `POST` is an op on the
/// unary and batch routes, only `GET` (and `HEAD`, which axum routes to
/// it) on the subscription route.
#[derive(Debug, Clone)]
pub struct RpcBindingResolver {
    prefix: String,
}

impl RpcBindingResolver {
    /// For a router mounted at `prefix`, as for
    /// [`super::RestBindingResolver::new`].
    pub fn new(prefix: &str) -> Self {
        Self {
            prefix: mount_prefix::normalize(prefix),
        }
    }
}

/// Every generated op id is dotted (`model.<Model>.<verb>`,
/// `procedure.<name>`), none contains `/`, and none is `batch`. Checked on
/// the value axum decoded, so no percent-encoding gets around it.
fn is_op_id(op_id: &str) -> bool {
    op_id != "batch" && op_id.contains('.') && !op_id.contains('/')
}

impl BindingResolver for RpcBindingResolver {
    fn resolve(&self, request: &RouteRequest<'_>) -> Resolution {
        let Some(template) = request
            .matched_path()
            .and_then(|matched| mount_prefix::strip(matched, &self.prefix))
        else {
            return Resolution::Unresolved;
        };
        let values = |params: &[(String, String)]| -> Vec<String> {
            params.iter().map(|(_, value)| value.clone()).collect()
        };
        let (expected, route) = if template == RPC_BATCH_PATH {
            (Method::POST, None)
        } else if template == RPC_UNARY_PATH {
            (Method::POST, Some(""))
        } else if template == RPC_SUBSCRIBE_PATH {
            (Method::GET, Some("subscribe/"))
        } else {
            return Resolution::Unresolved;
        };
        // axum answers `HEAD` with the `GET` route, so a `HEAD` to the
        // subscription route runs its handler: it is that op, bound with
        // its real method, as in the REST resolver (security finding SF-2
        // of the second review). As `MethodNotAllowed` it passed unsigned
        // whenever the policy's `unresolved_mode` was not `Required`.
        let head_as_get = expected == Method::GET && request.method() == Method::HEAD;
        if request.method() != expected && !head_as_get {
            return Resolution::MethodNotAllowed(vec![expected]);
        }
        let Some(route_prefix) = route else {
            return Resolution::Op(ResolvedRoute::new("batch", values(request.path_params())));
        };
        // The op id is the last parameter (a parameterised mount's come
        // first), bound as the router decoded it, which is what it runs.
        let Some(((_, op_id), mount)) = request.path_params().split_last() else {
            return Resolution::Unresolved;
        };
        if !is_op_id(op_id) {
            return Resolution::NotAnOp;
        }
        Resolution::Op(ResolvedRoute::new(
            format!("{route_prefix}{op_id}"),
            values(mount),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::is_op_id;

    #[test]
    fn only_a_generated_shape_of_op_id_is_an_op() {
        for op in ["procedure.ping", "model.Widget.list"] {
            assert!(is_op_id(op), "{op}");
        }
        for not in ["batch", "ping", "", "a/b.c", "procedure.a/b", "subscribe/x"] {
            assert!(!is_op_id(not), "{not}");
        }
    }
}
