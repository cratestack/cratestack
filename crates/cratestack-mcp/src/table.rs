//! The contract between this crate and the per-schema `mcp` module the
//! macro generates (`cratestack-macros/src/include/server/mcp_module/`).
//!
//! Split in two on purpose: [`McpTools::decode`] turns JSON into the
//! procedure's typed `Args`, and [`McpTools::execute`] runs it. L3 admission
//! sits between the two (ADR 0002 § Dispatch): arguments that cannot decode
//! must be refused *before* they charge a rate-limit token or take an
//! idempotency reservation, and nothing may run before admission says so.

use std::fmt;
use std::future::Future;

use cratestack_core::{CratestackContext, CratestackError, OpDescriptor};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// One exposed tool, as the generated table states it at compile time.
///
/// `#[non_exhaustive]` so a later phase can add a field without a breaking
/// release; the generated code builds it with [`ToolDescriptor::new`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ToolDescriptor {
    /// The MCP tool name (`@mcp(tool: "...")`, or the procedure name, ADR 0002 Q2).
    pub name: &'static str,
    /// `@mcp(description: "...")`. Deliberately not defaulted from the
    /// procedure's `///` docs: those are written for the service's own
    /// developers, and nothing reviewed them for an agent audience.
    pub description: Option<&'static str>,
    /// JSON Schema 2020-12 of the procedure's `Args`, from the phase 2
    /// generator, serialized at macro-expansion time.
    pub input_schema: &'static str,
    /// Present when the procedure returns an object (a `type`, a `model` or
    /// a `Page<T>`); MCP's `outputSchema` must have an object root.
    pub output_schema: Option<&'static str>,
    /// `true` for `procedure`, `false` for `mutation procedure`.
    pub read_only: bool,
    /// The procedure's participation flags, computed by the same helpers
    /// that fill REST's and RPC's descriptors, so the three transports
    /// cannot disagree about whether a tool is rate limited or reserved.
    pub op: &'static OpDescriptor,
}

impl ToolDescriptor {
    pub const fn new(
        name: &'static str,
        description: Option<&'static str>,
        input_schema: &'static str,
        output_schema: Option<&'static str>,
        read_only: bool,
        op: &'static OpDescriptor,
    ) -> Self {
        Self {
            name,
            description,
            input_schema,
            output_schema,
            read_only,
            op,
        }
    }
}

/// A schema's exposed tools. Implemented by the generated
/// `cratestack_schema::mcp::McpTools`; implementing it by hand is possible,
/// but it would also mean writing the policy call by hand, which is the
/// thing ADR 0002 generates this to avoid.
pub trait McpTools: Send + Sync + 'static {
    /// One decoded call: the tool it names and its typed `Args`.
    type Call: Send + 'static;

    /// Every exposed tool, in declaration order. `tools/list` returns this
    /// order, unfiltered by the caller's authorization (ADR 0002 § Tools).
    fn tools(&self) -> &'static [ToolDescriptor];

    /// Deserialize `arguments` into the named tool's `Args`. `tool` is
    /// always a name from [`Self::tools`]; the caller looked it up first.
    fn decode(&self, tool: &str, arguments: Value) -> Result<Self::Call, ArgumentsError>;

    /// Run the call under `ctx`: policy, then the implementation, then any
    /// `@computed` output fields, through the same generated path REST and
    /// RPC take. The value is the procedure's output as JSON.
    fn execute(
        &self,
        call: Self::Call,
        ctx: &CratestackContext,
    ) -> impl Future<Output = Result<Value, CratestackError>> + Send;
}

/// Arguments that do not deserialize into the tool's `Args`. The message
/// names the offending argument, so an agent can correct itself; it only
/// restates the tool's own public input schema and the caller's own input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgumentsError {
    message: String,
}

impl ArgumentsError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ArgumentsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ArgumentsError {}

/// Deserialize tool arguments, naming the path of the first field that
/// failed. Plain `serde_json::from_value` names a *missing* field but not a
/// *mistyped* one (`invalid type: string "x", expected i64` says nothing of
/// where), which is why this goes through `serde_path_to_error`.
pub fn decode_arguments<T: DeserializeOwned>(arguments: Value) -> Result<T, ArgumentsError> {
    serde_path_to_error::deserialize(arguments).map_err(|error| {
        let path = error.path().to_string();
        let inner = error.into_inner();
        if path == "." {
            ArgumentsError::new(format!("invalid arguments: {inner}"))
        } else {
            ArgumentsError::new(format!("invalid argument `{path}`: {inner}"))
        }
    })
}

/// Serialize a procedure's output for the result. A failure here is a bug
/// in a generated type, not the caller's, so it is an internal error whose
/// detail stays in the log.
pub fn encode_output<T: Serialize>(output: &T) -> Result<Value, CratestackError> {
    serde_json::to_value(output).map_err(|error| {
        CratestackError::Internal(format!("mcp: could not serialize a tool result: {error}"))
    })
}
