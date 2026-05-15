//! Client-side companion to `rpc-streaming-example`.
//!
//! Consumes the `procedure.ticks` cbor-seq stream via the new
//! `RpcClient::call_streaming` API in `cratestack-client-rust`. The
//! point of this example: streaming on the RPC binding is a
//! **content-negotiation** decision on the same `/rpc/{op_id}` URL —
//! from the client side that means one call returns an
//! `mpsc::Receiver` that yields items as cbor-seq frames arrive on
//! the wire, no full-body buffering.
//!
//! `main.rs` is the runnable binary; `tests/smoke.rs` exercises the
//! end-to-end consumption path against a tiny in-process mock so the
//! demo runs in CI without needing the server example running.

use cratestack_client_rust::{
    AuthorizationRequest, ClientError, RequestAuthorizer,
};
use serde::{Deserialize, Serialize};

/// Mirrors the procedure input envelope the server expects:
/// `{ "args": { "start": ..., "count": ... } }`. Procedures always wrap
/// their declared `args` in an outer `args:` envelope so the macro can
/// fan additional fields (e.g. `idem`) into the same input shape later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerInput {
    pub args: TickerArgs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerArgs {
    pub start: i64,
    pub count: i64,
}

/// Server-side `type Tick` mirrored here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tick {
    pub index: i64,
    pub value: i64,
}

/// Injects the `x-auth-id` header on every request. The server example
/// reads it via a `HeaderAuthProvider` so any positive integer
/// authenticates as that caller id. Real apps replace this with a
/// signing authorizer (`HmacEnvelope`, JWT, etc.).
pub struct StaticAuthId(pub i64);

impl RequestAuthorizer for StaticAuthId {
    fn authorize(
        &self,
        _request: &AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError> {
        Ok(vec![("x-auth-id".to_owned(), self.0.to_string())])
    }
}

/// Dotted dispatch key the server emits for the `ticks` procedure.
/// Procedures map to `procedure.<name>` (raw schema name — not
/// camelCased). Models would be `model.<ModelName>.<verb>`.
pub const TICKS_OP_ID: &str = "procedure.ticks";
