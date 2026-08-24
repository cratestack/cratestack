//! `cratestack-core` — backend-agnostic primitives shared by every
//! crate in the framework: schema IR, audit + envelope primitives,
//! the `CratestackError` / `CratestackContext` / `Value` types, batch envelopes,
//! RPC wire shapes, and field-level validators.
//!
//! The public surface is intentionally flat at the crate root: every
//! type re-exports from a focused submodule below, so callers can
//! keep writing `cratestack_core::CratestackError` while the implementation
//! lives in `cratestack_core::error`. New code can opt into the
//! submodule paths directly.

pub mod audit;
pub mod batch;
pub mod builder;
pub mod codec;
pub mod composite_id;
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
pub mod pascal_case;
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
pub type CratestackBody = bytes::Bytes;

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
pub use codec::{CratestackCodec, CratestackEnvelope, NoEnvelope};
pub use context::{
    AuthProvider, CachedAuthProvider, CratestackAuthIdentity, CratestackContext, PrincipalContext,
    PrincipalFacet, RequestContext, SystemContext,
};
// `Decimal` only exists when EXACTLY ONE decimal backend feature is
// active; `RustDecimal`/`BigDecimal` each only exist under their own
// feature independently of the other; `DecimalValue` is unconditional —
// see `decimal`'s module doc (cratestack#505 Direction 2).
#[cfg(feature = "decimal-bigdecimal")]
pub use decimal::BigDecimal;
#[cfg(all(feature = "decimal-rust-decimal", not(feature = "decimal-bigdecimal")))]
pub use decimal::Decimal;
#[cfg(all(feature = "decimal-bigdecimal", not(feature = "decimal-rust-decimal")))]
pub use decimal::Decimal;
pub use decimal::DecimalValue;
#[cfg(feature = "decimal-rust-decimal")]
pub use decimal::RustDecimal;
pub use envelope::{
    HmacEnvelope, InMemoryNonceStore, KeyProvider, NonceStore, SealedEnvelope, StaticKeyProvider,
};
pub use error::{CratestackError, CratestackErrorResponse, DbErrorInfo, parse_cuid};
pub use events::{
    CratestackEventBus, CratestackEventEnvelope, CratestackEventFuture, ModelEvent, ModelEventKind,
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
    Attribute, AuthBlock, ComputedParamsArg, ConfigBlock, ConfigEntry, Datasource, EnumDecl,
    EnumVariant, ExtensionKind, Field, MixinDecl, Model, OwnedSchemaSummary, ParsedIndexAttribute,
    Procedure, ProcedureArg, ProcedureKind, Schema, SchemaSummary, SelectionQuery, SourceSpan,
    TransportStyle, TypeArity, TypeDecl, TypeRef, View, ViewSource, computed_params_type_name,
    is_computed_attribute, is_computed_field, parse_composite_id_attribute,
    parse_composite_unique_attribute, parse_computed_params_arg, parse_index_attribute,
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
// `validate_range_decimal` is generic over `DecimalValue` (cratestack#505
// Direction 2), so it's unconditional — no decimal feature required.
pub use validators::validate_range_decimal;
pub use value::Value;
