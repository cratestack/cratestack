//! `cratestack-core` — backend-agnostic primitives shared by every
//! crate in the framework: schema IR, audit + envelope primitives,
//! the `CoolError` / `CoolContext` / `Value` types, batch envelopes,
//! RPC wire shapes, and field-level validators.
//!
//! The public surface is intentionally flat at the crate root: every
//! type re-exports from a focused submodule below, so callers can
//! keep writing `cratestack_core::CoolError` while the implementation
//! lives in `cratestack_core::error`. New code can opt into the
//! submodule paths directly.

pub mod audit;
pub mod batch;
pub mod codec;
pub mod context;
pub mod envelope;
pub mod error;
pub mod events;
pub mod json;
pub mod page;
pub mod projection;
pub mod rpc;
pub mod schema;
pub mod transport;
pub mod validators;
pub mod value;

// -----------------------------------------------------------------------------
// Decimal scalar
//
// Selected at compile time via mutually-exclusive Cargo features. Generated
// code references `cratestack::Decimal` regardless of backend, so swapping
// backends is a workspace-feature flip rather than a code change.
// -----------------------------------------------------------------------------

#[cfg(all(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal"))]
compile_error!(
    "cratestack: features `decimal-rust-decimal` and `decimal-bigdecimal` are mutually exclusive"
);

#[cfg(not(any(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal")))]
compile_error!(
    "cratestack: enable exactly one Decimal backend feature (`decimal-rust-decimal` or `decimal-bigdecimal`)"
);

#[cfg(feature = "decimal-rust-decimal")]
pub type Decimal = rust_decimal::Decimal;

#[cfg(feature = "decimal-bigdecimal")]
compile_error!(
    "cratestack: the `decimal-bigdecimal` backend is reserved but not yet implemented; use `decimal-rust-decimal` for now"
);

/// Body bytes carried through the transport layer.
pub type CoolBody = bytes::Bytes;

// Backwards-compatible re-exports so external crates keep using
// `cratestack_core::Type` rather than `cratestack_core::module::Type`.

pub use audit::{
    AuditActor, AuditEvent, AuditOperation, AuditSink, MulticastAuditSink, NoopAuditSink,
    TransactionIsolation,
};
pub use batch::{
    BATCH_MAX_ITEMS, BatchItemError, BatchItemResult, BatchItemStatus, BatchRequest, BatchResponse,
    BatchSummary, find_duplicate_position,
};
pub use codec::{CoolCodec, CoolEnvelope, NoEnvelope};
pub use context::{
    AuthProvider, CoolAuthIdentity, CoolContext, PrincipalContext, PrincipalFacet, RequestContext,
};
pub use envelope::{
    HmacEnvelope, InMemoryNonceStore, KeyProvider, NonceStore, SealedEnvelope, StaticKeyProvider,
};
pub use error::{CoolError, CoolErrorResponse, parse_cuid};
pub use events::{
    CoolEventBus, CoolEventEnvelope, CoolEventFuture, ModelEvent, ModelEventKind, event_topic,
    parse_emit_attribute,
};
pub use json::Json;
pub use page::{Page, PageInfo};
pub use projection::ProjectionDecoder;
pub use schema::{
    Attribute, AuthBlock, ConfigBlock, ConfigEntry, Datasource, EnumDecl, EnumVariant, Field,
    MixinDecl, Model, OwnedSchemaSummary, Procedure, ProcedureArg, ProcedureKind, Schema,
    SchemaSummary, SelectionQuery, SourceSpan, TransportStyle, TypeArity, TypeDecl, TypeRef, View,
    ViewSource,
};
pub use transport::{
    OpDescriptor, OpKind, RouteTransportCapabilities, RouteTransportDescriptor,
    canonical_request_string,
};
pub use validators::{
    validate_email, validate_iso4217, validate_length, validate_range_decimal, validate_range_i64,
    validate_uri,
};
pub use value::Value;
