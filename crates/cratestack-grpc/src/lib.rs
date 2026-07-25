//! CrateStack gRPC server runtime — the `cratestack-axum` sibling for
//! `transport grpc` schemas (`docs/design/protobuf.md` §7.2). Holds the
//! primitives macro-generated code calls into via `::cratestack::grpc::...`:
//! `CoolError` -> `tonic::Status` mapping ([`error`]), `tonic::metadata::MetadataMap`
//! <-> `http::HeaderMap` conversion ([`metadata`]) so the existing
//! header-driven `AuthProvider` ports unchanged, gRPC wire-frame handling
//! ([`framing`]), unframed-body envelope canonicalization ([`canonical`]),
//! structured error details on `grpc-status-details-bin` ([`status_details`]),
//! and the gRPC-Web + CORS layering that makes a mounted service reachable
//! from a browser ([`web`], `docs/design/protobuf.md` §7.4).
//!
//! **Scope of this crate today (tickets #171-#172):** runtime primitives
//! (error/metadata/framing/canonical/status-details/web) plus, via
//! `cratestack-macros`' `grpc` feature, macro-generated mirror structs and
//! a hand-rolled tonic service for `transport grpc` model CRUD — see the
//! crate README and the tickets for exact status (procedures and
//! server-streaming are not yet wired into the generated service, so
//! there is nothing beyond CRUD for gRPC-Web to expose either).
//!
//! `prost`, `prost_types`, and `tonic` are re-exported so macro-generated
//! code can reference `::cratestack::grpc::prost::...` /
//! `::cratestack::grpc::tonic::...` without adding its own `Cargo.toml`
//! entries for the *tonic* side. **This does not extend to `prost`
//! itself**: `prost_derive`'s `#[derive(Message)]` expansion hardcodes
//! bare `::prost::...` paths (no `#[prost(crate = "...")]` escape hatch,
//! confirmed against `prost-derive`'s source), so any crate whose
//! `transport grpc` schema expands `#[derive(Message)]` mirror structs —
//! i.e. every consumer enabling `cratestack-pg`'s `grpc` feature — needs
//! `prost` as an actual direct dependency too. This mirrors the existing,
//! pre-`transport grpc` requirement that consumers already depend on
//! `serde` directly for the same reason (`serde_derive`'s scoped `extern
//! crate serde as _serde;`).

pub mod canonical;
pub mod error;
pub mod framing;
pub mod metadata;
pub mod status_details;
pub mod web;

pub use canonical::{GRPC_CONTENT_TYPE, grpc_canonical_request_string, grpc_method_path};
pub use error::{cool_error_code_to_tonic_code, cool_error_to_status, rpc_code_to_tonic_code};
pub use framing::{FrameError, frame_grpc_message, strip_grpc_frame};
pub use metadata::{headers_to_metadata, metadata_to_headers};
pub use status_details::{
    DecodeStatusDetailsError, GrpcStatusDetails, decode_status_details_bin,
    encode_status_details_bin,
};
pub use web::{GRPC_WEB_EXPOSED_HEADERS, apply_grpc_web, grpc_web_cors_layer};

pub use prost;
pub use prost_types;
pub use tonic;
