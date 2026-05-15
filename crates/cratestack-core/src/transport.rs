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
    /// key. True for reads and pure procedures; false for mutations.
    pub idempotent_by_default: bool,
    pub auth_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    /// One input, one output. The common case — every CRUD verb and
    /// every non-streaming procedure.
    Unary,
    /// One input, a finite sequence of outputs. Used for `@stream`
    /// procedures and (future) streamed `list`. Terminates server-side.
    Sequence,
    /// One input, an open-ended sequence of outputs ended only by
    /// client cancellation or disconnect. WebSocket-only — see §3.4
    /// of the design doc. Fire-and-forget: no cursors, no replay
    /// buffer.
    Subscription,
}

impl OpKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            OpKind::Unary => "unary",
            OpKind::Sequence => "sequence",
            OpKind::Subscription => "subscription",
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
