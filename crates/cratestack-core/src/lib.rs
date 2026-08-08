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
pub mod find_many;
pub mod idempotency_record;
pub mod json;
pub mod page;
pub mod projection;
pub mod route_naming;
pub mod rpc;
pub mod rust_keywords;
pub mod schema;
pub mod store;
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

#[cfg(not(feature = "decimal-rust-decimal"))]
compile_error!("cratestack: enable the `decimal-rust-decimal` backend feature");

#[cfg(feature = "decimal-rust-decimal")]
pub type Decimal = rust_decimal::Decimal;

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
pub use error::{CoolError, CoolErrorResponse, DbErrorInfo, parse_cuid};
pub use events::{
    CoolEventBus, CoolEventEnvelope, CoolEventFuture, ModelEvent, ModelEventKind,
    SubscriptionGuard, SubscriptionHandle, event_topic, parse_emit_attribute,
};
pub use find_many::FieldFilterInput;
pub use idempotency_record::{IdempotencyRecord, ReservationOutcome};
pub use json::Json;
pub use page::{MAX_LIST_LIMIT, Page, PageInfo, PageInput};
pub use projection::ProjectionDecoder;
pub use schema::{
    Attribute, AuthBlock, ConfigBlock, ConfigEntry, Datasource, EnumDecl, EnumVariant,
    ExtensionKind, Field, MixinDecl, Model, OwnedSchemaSummary, ParsedIndexAttribute, Procedure,
    ProcedureArg, ProcedureKind, Schema, SchemaSummary, SelectionQuery, SourceSpan, TransportStyle,
    TypeArity, TypeDecl, TypeRef, View, ViewSource, parse_composite_id_attribute,
    parse_composite_unique_attribute, parse_index_attribute,
};
pub use store::{
    ClientStateStore, IdempotencyStore, InMemoryStateStore, JsonFileStateStore,
    PersistedClientState, RateLimitConfig, RateLimitDecision, RateLimitStore, RequestJournalEntry,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_rust_decimal_is_available() {
        // Verify that decimal-rust-decimal backend is available and the Decimal type works
        let d = Decimal::from(42);
        assert_eq!(d.to_string(), "42");
    }

    #[test]
    fn decimal_type_serialization() {
        // Verify basic decimal operations work
        let d1 = Decimal::from(10);
        let d2 = Decimal::from(20);
        // Just verify the types compile and basic operations work
        let _ = d1 + d2;
    }
}
