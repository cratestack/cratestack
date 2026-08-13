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
pub mod decimal;
pub mod envelope;
pub mod error;
pub mod events;
pub mod find_many;
pub mod idempotency_record;
pub mod json;
pub mod limits;
pub mod page;
pub mod patch;
pub mod projection;
pub mod route_naming;
pub mod rpc;
pub mod rust_keywords;
pub mod schema;
pub mod store;
pub mod transport;
pub mod validators;
pub mod value;

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
    SystemContext,
};
// `Decimal` only exists on `decimal-rust-decimal` or `decimal-bigdecimal` —
// see `decimal`'s module doc for why the "neither" case has no fallback.
#[cfg(any(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal"))]
pub use decimal::Decimal;
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
pub use limits::{DEFAULT_BODY_LIMIT_BYTES, MAX_RESPONSE_REBUFFER_BYTES};
pub use page::{MAX_LIST_LIMIT, Page, PageInfo, PageInput};
pub use patch::deserialize_double_option;
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
    validate_email, validate_iso4217, validate_length, validate_length_bytes, validate_range_i64,
    validate_uri,
};
// `validate_range_decimal` takes `&Decimal`, so it only exists when
// `Decimal` does — see `decimal`'s "Selecting NEITHER feature is not an
// error" module doc section.
#[cfg(any(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal"))]
pub use validators::validate_range_decimal;
pub use value::Value;
