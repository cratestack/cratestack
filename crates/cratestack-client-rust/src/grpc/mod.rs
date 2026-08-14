//! Native Rust gRPC client runtime (ticket #209) — the `tonic` sibling of
//! `crate::client` (REST) and `crate::rpc` (RPC). Feature-gated
//! (`grpc`, off by default) so a REST/RPC-only consumer never pulls in
//! `tonic`.
//!
//! - [`core`] — [`CratestackGrpcClient`]: wraps a `tonic::client::Grpc<T>`,
//!   carries the same [`crate::auth::RequestAuthorizer`] / schema-sha
//!   conventions `crate::client::CratestackClient` (REST) and
//!   `crate::rpc::RpcClient` (RPC) already use, so a schema author
//!   configures auth once regardless of transport. Exposes one `unary`
//!   helper every generated per-model gRPC client method
//!   (`include_client_schema!`'s `grpc` codegen,
//!   `cratestack-macros::include::client::grpc`) calls into — the
//!   client-side twin of the server's hand-rolled `tonic::server::Grpc`
//!   service arms (`cratestack-macros::include::server::grpc::service`).
//! - [`error`] — [`GrpcClientError`], wrapping `tonic::Status` directly:
//!   unlike REST/RPC (which decode a JSON/CBOR error body), a gRPC error
//!   already arrives as a structured `tonic::Status` carrying the right
//!   `tonic::Code` (`cratestack_grpc::error::cratestack_error_to_status`
//!   already did that mapping server-side), so there is no body left to
//!   parse — see that module's doc.
//! - [`canonical`] — envelope-signing helpers mirroring
//!   `cratestack_grpc::canonical`, adapted for the client side of the
//!   wire (no frame to strip — see that module's doc for the ticket's
//!   highest-risk decision, spelled out in full).

pub mod canonical;
mod core;
mod error;

pub use core::CratestackGrpcClient;
pub use error::GrpcClientError;
