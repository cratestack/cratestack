//! Transport-binding wire shapes shared by every generator (REST,
//! RPC) and every server emitter.

/// Wire-level capabilities for one route under a REST binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteTransportCapabilities {
    pub request_types: &'static [&'static str],
    pub response_types: &'static [&'static str],
    pub default_response_type: &'static str,
    pub supports_sequence_response: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteTransportDescriptor {
    pub name: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub capabilities: RouteTransportCapabilities,
    /// Whether this route may be safely retried without an idempotency
    /// key, and therefore takes **no** idempotency reservation.
    ///
    /// This is the single participation flag — there is no second boolean
    /// distinguishing "inherently safe" from "opted out" (ADR 0015 (d)),
    /// because no consumer has ever needed to tell them apart and an
    /// unread flag is a worse artefact than no flag. It is `true` for
    /// three things: reads (`GET` model routes), pure procedures
    /// (`query procedure`), and any mutation the schema marked
    /// `@no_idempotency`. `false` for every other model write.
    ///
    /// Mirrors `OpDescriptor::idempotent_by_default` so REST and RPC
    /// carry the same fact about the same op, even though only one of
    /// `ROUTE_TRANSPORTS`/`OPS` is ever populated for a given schema —
    /// the transport-parity rule in `CLAUDE.md`, and the reason this
    /// field exists at all: shipping idempotency admission on RPC alone
    /// would have reproduced cratestack#474 in a narrower form.
    ///
    /// Read at runtime by `cratestack_exec::OpExecutor::admit`, via the
    /// resolver `cratestack_axum::idempotency::build_rest_op_resolver`
    /// installs. A route whose descriptor no resolver can find still
    /// reserves — see that function's fail-closed direction.
    pub idempotent_by_default: bool,
    /// Whether the dispatcher should treat this route as participating in
    /// rate limiting. `true` for every route by default; `false` only for
    /// a procedure marked `@no_rate_limit` in a schema that declares
    /// `extension rate_limit { }` (`docs/design/extensions.md` §5) — model
    /// CRUD routes have no opt-out today and are always `true`. Mirrors
    /// `OpDescriptor::rate_limited_by_default` (cratestack#474) so REST
    /// and RPC transports carry the same fact about the same op, even
    /// though only one of `ROUTE_TRANSPORTS`/`OPS` is ever populated for
    /// a given schema. This is participation only: it carries no
    /// burst/refill/window numbers, and changes nothing about whether
    /// `RateLimitLayer` is actually wired up at runtime.
    pub rate_limited_by_default: bool,
}

/// Wire-shape of a single op in a `transport rpc` schema. See
/// `docs/design/rpc-transport.md` for the full design — in short, an
/// op is the dispatch unit shared by every RPC binding (HTTP unary,
/// HTTP batch, HTTP stream, WebSocket). The macro emits one
/// `OpDescriptor` per CRUD verb and per procedure when
/// `Schema.transport == TransportStyle::Rpc`.
///
/// REST schemas continue to emit [`RouteTransportDescriptor`] instead;
/// nothing emits both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpDescriptor {
    /// Stable dotted id, e.g. `"model.User.list"` or
    /// `"procedure.publishPost"`. This is the only dispatch key —
    /// same string appears in URLs (`POST /rpc/:op_id`), in
    /// batch/WS `Request.op` fields, and in generated client SDK
    /// call sites.
    pub op_id: &'static str,
    pub kind: OpKind,
    /// Schema-level name of the input type (e.g. `"PublishPostInput"`).
    /// Empty string when the op takes no input.
    pub input_ty: &'static str,
    /// Schema-level name of the output type. Empty string when the
    /// op returns nothing (e.g. `delete` with no echo).
    pub output_ty: &'static str,
    /// Whether the op can be safely retried without an idempotency
    /// key, and therefore takes **no** idempotency reservation.
    ///
    /// This is the single participation flag — there is no second boolean
    /// distinguishing "inherently safe" from "opted out" (ADR 0015 (d)),
    /// because no consumer has ever needed to tell them apart and an
    /// unread flag is a worse artefact than no flag. It is `true` for
    /// three things: reads (`model.<X>.{list,get,subscribe}`), pure
    /// procedures (`query procedure`), and any mutation the schema marked
    /// `@no_idempotency`. `false` for every other model write.
    ///
    /// Unlike `rate_limited_by_default` below, `@no_idempotency` is NOT
    /// gated on an `extension` block — whether idempotency should acquire
    /// one is an epic-level question, not a slice-1 one, so the attribute
    /// is honoured wherever it appears.
    ///
    /// Read at runtime by `cratestack_exec::OpExecutor::admit`, via the
    /// resolver `cratestack_axum::idempotency::build_rpc_op_resolver`
    /// installs. An op no resolver can find still reserves — see that
    /// function's fail-closed direction.
    pub idempotent_by_default: bool,
    /// Whether the dispatcher should treat this op as participating in
    /// rate limiting. `true` for every op by default; `false` only for a
    /// procedure marked `@no_rate_limit` in a schema that declares
    /// `extension rate_limit { }` (`docs/design/extensions.md` §5) — model
    /// CRUD ops have no opt-out today and are always `true`. This is
    /// participation only: it carries no burst/refill/window numbers, and
    /// changes nothing about whether `RateLimitLayer` is actually wired up
    /// at runtime, mirroring how `idempotent_by_default` above describes a
    /// fact about the op rather than configuring anything.
    pub rate_limited_by_default: bool,
    pub auth_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpKind {
    /// One input, one output. The common case — every CRUD verb and
    /// every non-streaming procedure.
    Unary,
    /// One input, a finite sequence of outputs. Used for `@stream`
    /// procedures and (future) streamed `list`. Terminates server-side.
    Sequence,
    /// No input, an open-ended sequence of outputs ended only by
    /// backpressure overflow or client disconnect. Emitted for
    /// `model.<X>.subscribe` when a model declares `@@subscribe`.
    /// Dispatched today via SSE (`GET /rpc/subscribe/{op_id}`, design
    /// doc §3.4a) — the recommended first binding per issue #183's
    /// spike decision; WebSocket (§3.4) remains speced but unbuilt,
    /// gated on a real bidirectional/high-multiplexing need. Both
    /// bindings share the same fire-and-forget semantics: no cursors,
    /// no replay buffer.
    Subscription,
}

impl OpKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            OpKind::Unary => "unary",
            OpKind::Sequence => "sequence",
            OpKind::Subscription => "subscription",
            #[allow(unreachable_patterns)]
            _ => "unknown",
        }
    }
}

/// Canonical string assembled by the envelope signing path:
/// `METHOD\nPATH\nQUERY\nCONTENT-TYPE\nbody-hex`. Both seal and verify
/// reconstruct the same string from the same inputs.
pub fn canonical_request_string(
    method: &str,
    path: &str,
    canonical_query: Option<&str>,
    content_type: Option<&str>,
    body: &[u8],
) -> String {
    let query = canonical_query.unwrap_or_default();
    let content_type = content_type.unwrap_or_default();
    let body_hex = body
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{method}\n{path}\n{query}\n{content_type}\n{body_hex}")
}

#[cfg(test)]
mod tests;
