//! [`GrpcClientError`] — the gRPC sibling of `crate::error::ClientError`
//! (REST) and `crate::rpc::RpcClientError` (RPC).
//!
//! REST and RPC both decode a *body* on a non-2xx response
//! (`CoolErrorResponse` / `RpcErrorBody`) to recover a stable error code —
//! the HTTP status line alone doesn't carry one. gRPC doesn't have this
//! problem: `tonic::Status` already **is** the structured error, with a
//! `tonic::Code` the server derived from the exact same `CoolError` via
//! `cratestack_grpc::error::cool_error_to_status` (server-side,
//! `cool_error_to_status` -> `rpc_code` -> `tonic::Code`, the identical
//! table `cratestack_grpc::error::rpc_code_to_tonic_code` documents). So
//! `GrpcClientError::Status` wraps the `tonic::Status` tonic itself hands
//! back from a failed `Grpc::unary` call, unparsed — there is nothing left
//! to decode, and re-deriving a code string from `status.code()` would
//! just be inverting a mapping the server already computed correctly.

#[derive(Debug, thiserror::Error)]
pub enum GrpcClientError {
    /// The call reached the server and it returned a gRPC error status.
    /// `status.code()` is one of `cratestack_grpc::error::
    /// rpc_code_to_tonic_code`'s table entries; `status.message()` is the
    /// same `CoolError::public_message` text every other binding surfaces.
    #[error("gRPC call failed: {0}")]
    Status(#[from] tonic::Status),
    /// Transport-level failure — connecting, or the inner `GrpcService`
    /// reporting not-ready and staying that way (`Grpc::ready`'s error,
    /// boxed since `tonic::client::GrpcService::Error: Into<BoxError>` is
    /// the only bound tonic guarantees on it).
    #[error("gRPC transport error: {0}")]
    Transport(#[source] tonic::codegen::StdError),
    /// A response decoded off the wire but failed to convert into the
    /// domain type (`TryFrom<pb::Model> for Model`, the same
    /// `CoolError`-returning conversion `grpc_pb::message::render_message`
    /// generates for every binding).
    #[error("codec error: {0}")]
    Codec(#[from] cratestack_core::CoolError),
    /// A `RequestAuthorizer` rejected building headers for this call —
    /// same variant shape as `crate::error::ClientError::BadInput`.
    #[error("bad input: {0}")]
    BadInput(String),
}
