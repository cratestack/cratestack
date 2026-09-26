//! [`PolicyRequest`]: what an [`super::EnvelopePolicy`] sees of a request.

use http::Method;

/// The op a request addresses, as an [`super::EnvelopePolicy`] sees it.
///
/// Deliberately **no headers** and no body (API-review decision,
/// 2026-09-26): a policy that could read a header could be talked into a
/// weaker mode by whoever writes that header. What it gets is the op:
///
/// - REST: the route template the schema declares (`/widgets/{id}`).
/// - RPC unary: the op id (`procedure.ping`).
/// - RPC subscription: the **bare** op id (`model.Widget.subscribe`), with
///   [`is_subscription`](Self::is_subscription) set, so a policy keyed on
///   op ids covers subscriptions without knowing their route (decision B1).
/// - RPC batch: asked once for `batch` itself, and once per frame with the
///   frame's op id and [`is_batch_frame`](Self::is_batch_frame) set; the
///   call runs under the strictest answer (decision B1).
#[derive(Debug, Clone, Copy)]
pub struct PolicyRequest<'a> {
    method: &'a Method,
    op: &'a str,
    subscription: bool,
    batch_frame: bool,
}

impl<'a> PolicyRequest<'a> {
    /// A request for `op`, as the layer describes it. Public so a policy
    /// can be unit-tested; the layer builds its own.
    pub fn new(method: &'a Method, op: &'a str) -> Self {
        Self {
            method,
            op,
            subscription: false,
            batch_frame: false,
        }
    }

    /// The same request, marked as a subscription.
    pub fn subscription(self) -> Self {
        Self {
            subscription: true,
            ..self
        }
    }

    /// The same request, marked as one frame of a `/rpc/batch` call.
    pub fn batch_frame(self) -> Self {
        Self {
            batch_frame: true,
            ..self
        }
    }

    /// The HTTP method (`POST` for every batch frame).
    pub fn method(&self) -> &'a Method {
        self.method
    }

    /// The op: a REST route template or an RPC op id (see the type docs).
    pub fn op(&self) -> &'a str {
        self.op
    }

    /// An RPC subscription (`/rpc/subscribe/{op_id}`).
    pub fn is_subscription(&self) -> bool {
        self.subscription
    }

    /// One frame of a `/rpc/batch` call.
    pub fn is_batch_frame(&self) -> bool {
        self.batch_frame
    }
}
