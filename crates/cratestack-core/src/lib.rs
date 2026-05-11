use std::borrow::Cow;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use http::StatusCode;
use serde::{Deserialize, Serialize};

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

// -----------------------------------------------------------------------------
// Audit log primitives
//
// The audit subsystem is split into a record format (here, backend-agnostic)
// and a sink trait. The canonical store is a table inside the same database
// as the mutation, written inside the same transaction so audit events never
// drift from the data they describe. Downstream fan-out (Kafka, Redis pubsub,
// HTTP webhook) goes through an `AuditSink` implementation; the table itself
// remains the source of truth for compliance review.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditOperation {
    Create,
    Update,
    Delete,
}

impl AuditOperation {
    pub const fn as_str(&self) -> &'static str {
        match self {
            AuditOperation::Create => "create",
            AuditOperation::Update => "update",
            AuditOperation::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AuditActor {
    /// Actor identifier — typically the user id from the auth context. Omit
    /// when the operation runs without an authenticated principal (system
    /// jobs, migrations).
    pub id: Option<String>,
    /// Free-form claims captured from the auth context at the time of the
    /// operation. Banks use this for role/scope replay during forensics.
    pub claims: BTreeMap<String, Value>,
    /// Source IP recorded by the transport layer, if available.
    pub ip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_id: uuid::Uuid,
    /// Schema name as declared in the `.cstack` file — lets you scope audit
    /// queries to a single service without inspecting model strings.
    pub schema_name: String,
    /// Model name as declared in the schema (e.g. `Account`, `Transfer`).
    pub model: String,
    pub operation: AuditOperation,
    pub primary_key: serde_json::Value,
    pub actor: AuditActor,
    /// Tenant identifier captured from `PrincipalContext.tenant.id` when
    /// present. Banks running multi-tenant clusters use this to scope
    /// per-tenant audit exports.
    pub tenant: Option<String>,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    /// W3C `traceparent`-style request id, if the transport layer captured
    /// one. Useful for stitching audit rows to APM traces.
    pub request_id: Option<String>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

/// Pluggable audit sink. Implementations fan audit events out to downstream
/// systems (Kafka topics, Redis pubsub, HTTP webhooks, S3 buckets) for
/// long-term retention or SIEM ingestion. The in-database table written by
/// `cratestack_sqlx` remains the canonical record; sinks are best-effort
/// projections.
#[async_trait::async_trait]
pub trait AuditSink: Send + Sync + 'static {
    async fn record(&self, event: &AuditEvent) -> Result<(), CoolError>;
}

/// Default sink that does nothing. The in-database audit table is treated as
/// authoritative; downstream consumers are added by wrapping a different
/// sink (or composing several).
#[derive(Debug, Clone, Default)]
pub struct NoopAuditSink;

#[async_trait::async_trait]
impl AuditSink for NoopAuditSink {
    async fn record(&self, _event: &AuditEvent) -> Result<(), CoolError> {
        Ok(())
    }
}

/// Fan an audit event out to multiple sinks. Errors from any individual
/// sink are aggregated into `CoolError::Internal` so a single failing
/// downstream does not silently swallow problems with the others.
pub struct MulticastAuditSink {
    sinks: Vec<Arc<dyn AuditSink>>,
}

impl MulticastAuditSink {
    pub fn new(sinks: Vec<Arc<dyn AuditSink>>) -> Self {
        Self { sinks }
    }
}

#[async_trait::async_trait]
impl AuditSink for MulticastAuditSink {
    async fn record(&self, event: &AuditEvent) -> Result<(), CoolError> {
        let mut errors = Vec::new();
        for sink in &self.sinks {
            if let Err(error) = sink.record(event).await {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(CoolError::Internal(format!(
                "{} audit sink(s) failed: {}",
                errors.len(),
                errors
                    .iter()
                    .map(|e| e.detail().unwrap_or("(no detail)").to_owned())
                    .collect::<Vec<_>>()
                    .join("; "),
            )))
        }
    }
}

/// Transaction isolation level requested by a procedure via `@isolation(...)`.
/// Mirrors the PostgreSQL spec: lower variants tolerate more anomalies, the
/// higher ones cost more under contention. Banks running multi-row updates
/// (transfers, postings) typically pick `Serializable` and pair it with
/// retry-on-serialization-failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionIsolation {
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

impl TransactionIsolation {
    pub fn parse(value: &str) -> Result<Self, CoolError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "read_committed" | "read committed" => Ok(Self::ReadCommitted),
            "repeatable_read" | "repeatable read" => Ok(Self::RepeatableRead),
            "serializable" => Ok(Self::Serializable),
            other => Err(CoolError::Validation(format!(
                "unknown transaction isolation level '{other}'; expected one of \
                 'read_committed', 'repeatable_read', 'serializable'",
            ))),
        }
    }

    pub const fn as_sql(&self) -> &'static str {
        match self {
            Self::ReadCommitted => "READ COMMITTED",
            Self::RepeatableRead => "REPEATABLE READ",
            Self::Serializable => "SERIALIZABLE",
        }
    }
}

// -----------------------------------------------------------------------------
// Signed envelope (HMAC-SHA-256)
//
// Phase 3 ships a working symmetric-key signed envelope that satisfies
// `CoolEnvelope`. The contract is intentionally close to COSE_Sign1 with
// HS256: a content header (kid, alg, timestamp, nonce) is folded into the
// signing input alongside the body bytes, and the sealed message is a CBOR
// map `{ kid, alg, ts, nonce, body, mac }`. A full COSE_Sign1 implementation
// with ES256/EdDSA lands in a follow-up — adding it is non-breaking thanks
// to the `KeyProvider` trait below.
// -----------------------------------------------------------------------------

/// Resolves signing keys by kid (key id). Banks running multi-tenant or
/// rotating keysets implement this so the envelope code never has to know
/// the storage mechanism. Implementations must be constant-time for
/// not-found vs wrong-tenant errors — never use the error message to leak
/// whether a key id exists.
#[async_trait::async_trait]
pub trait KeyProvider: Send + Sync + 'static {
    /// Return the raw key bytes for the given `kid`. For HMAC this is the
    /// symmetric secret. Error if the key is unknown.
    async fn resolve_signing_key(&self, kid: &str) -> Result<Vec<u8>, CoolError>;
}

/// In-memory `KeyProvider` for tests and single-tenant deployments. Banks
/// running real workloads bring a backed implementation (KMS, Vault, HSM).
#[derive(Debug, Clone, Default)]
pub struct StaticKeyProvider {
    keys: BTreeMap<String, Vec<u8>>,
}

impl StaticKeyProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_key(mut self, kid: impl Into<String>, key: Vec<u8>) -> Self {
        self.keys.insert(kid.into(), key);
        self
    }
}

#[async_trait::async_trait]
impl KeyProvider for StaticKeyProvider {
    async fn resolve_signing_key(&self, kid: &str) -> Result<Vec<u8>, CoolError> {
        self.keys
            .get(kid)
            .cloned()
            .ok_or_else(|| CoolError::Unauthorized("unknown signing key".to_owned()))
    }
}

/// Maximum tolerable clock skew between sender and receiver when verifying
/// signed envelopes. Banks running cross-region traffic with NTP-sync
/// servers can lower this; off-the-shelf deployments leave it at the
/// default 5 minutes.
const ENVELOPE_DEFAULT_CLOCK_SKEW_SECS: i64 = 300;

/// Tracks the nonces of sealed envelopes that have already been verified
/// inside the clock-skew window, so a captured-and-replayed request gets
/// rejected the second time. Banks running multi-replica deployments back
/// this with Redis so the rejection holds cluster-wide.
#[async_trait::async_trait]
pub trait NonceStore: Send + Sync + 'static {
    /// Attempt to register `nonce` as seen. Returns `Ok(true)` if it is the
    /// first time we see it (caller may proceed); `Ok(false)` if it was
    /// already recorded (caller should reject). Implementations must drop
    /// entries past `expires_at` to keep the working set bounded.
    async fn record_if_unseen(
        &self,
        nonce: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, CoolError>;
}

/// In-memory nonce store. One mutex; the working set is bounded by the
/// clock-skew window — a 5-minute skew at 10k req/s caps at ~3M entries,
/// which is fine. Production multi-replica deployments swap in Redis.
#[derive(Debug, Clone, Default)]
pub struct InMemoryNonceStore {
    seen: Arc<RwLock<BTreeMap<String, chrono::DateTime<chrono::Utc>>>>,
}

impl InMemoryNonceStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl NonceStore for InMemoryNonceStore {
    async fn record_if_unseen(
        &self,
        nonce: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, CoolError> {
        let mut seen = self
            .seen
            .write()
            .map_err(|_| CoolError::Internal("nonce store poisoned".to_owned()))?;
        let now = chrono::Utc::now();
        seen.retain(|_, exp| *exp > now);
        if seen.contains_key(nonce) {
            return Ok(false);
        }
        seen.insert(nonce.to_owned(), expires_at);
        Ok(true)
    }
}

/// HMAC-SHA-256 backed envelope. Sealed messages are self-describing CBOR
/// maps: signature recipients can decode the envelope, fetch the key by
/// `kid`, and verify without out-of-band coordination.
#[derive(Clone)]
pub struct HmacEnvelope<K: KeyProvider> {
    keys: Arc<K>,
    signing_kid: String,
    clock_skew_secs: i64,
    nonces: Option<Arc<dyn NonceStore>>,
}

impl<K: KeyProvider> HmacEnvelope<K> {
    pub fn new(keys: Arc<K>, signing_kid: impl Into<String>) -> Self {
        Self {
            keys,
            signing_kid: signing_kid.into(),
            clock_skew_secs: ENVELOPE_DEFAULT_CLOCK_SKEW_SECS,
            nonces: None,
        }
    }

    pub fn with_clock_skew_secs(mut self, secs: i64) -> Self {
        self.clock_skew_secs = secs;
        self
    }

    /// Attach a nonce store so `open` rejects replays. Without this, the
    /// envelope is only protected by the clock-skew window — an attacker
    /// who captured a sealed message can replay it inside that window.
    pub fn with_nonce_store(mut self, store: Arc<dyn NonceStore>) -> Self {
        self.nonces = Some(store);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedEnvelope {
    pub kid: String,
    pub alg: String,
    pub ts: i64,
    pub nonce: String,
    pub body: serde_json::Value,
    pub mac_b64: String,
}

impl SealedEnvelope {
    fn signing_input(&self) -> Result<Vec<u8>, CoolError> {
        let mut buf = Vec::with_capacity(256);
        buf.extend_from_slice(self.kid.as_bytes());
        buf.push(0);
        buf.extend_from_slice(self.alg.as_bytes());
        buf.push(0);
        buf.extend_from_slice(&self.ts.to_be_bytes());
        buf.push(0);
        buf.extend_from_slice(self.nonce.as_bytes());
        buf.push(0);
        // Body is canonicalised via serde_json::to_vec which uses key-sort
        // order for objects when the input went through `serde_json::Value`
        // — adequate for HMAC integrity (the verifier reconstructs the
        // same bytes the sender signed).
        let body_bytes = serde_json::to_vec(&self.body)
            .map_err(|error| CoolError::Codec(format!("encode envelope body: {error}")))?;
        buf.extend_from_slice(&body_bytes);
        Ok(buf)
    }
}

impl<K: KeyProvider> HmacEnvelope<K> {
    async fn compute_mac(&self, key: &[u8], input: &[u8]) -> Result<Vec<u8>, CoolError> {
        use hmac::{Hmac, Mac};
        let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(key)
            .map_err(|_| CoolError::Internal("HMAC key length error".to_owned()))?;
        mac.update(input);
        Ok(mac.finalize().into_bytes().to_vec())
    }

    /// Seal a request body. The returned bytes are a CBOR-encoded
    /// `SealedEnvelope` payload — the sender wraps these in their codec of
    /// choice on the way out.
    pub async fn seal(&self, payload: serde_json::Value) -> Result<SealedEnvelope, CoolError> {
        let key = self.keys.resolve_signing_key(&self.signing_kid).await?;
        let ts = chrono::Utc::now().timestamp();
        let nonce = uuid::Uuid::new_v4().to_string();
        let mut envelope = SealedEnvelope {
            kid: self.signing_kid.clone(),
            alg: "HS256".to_owned(),
            ts,
            nonce,
            body: payload,
            mac_b64: String::new(),
        };
        let input = envelope.signing_input()?;
        let mac = self.compute_mac(&key, &input).await?;
        use base64::Engine;
        envelope.mac_b64 = base64::engine::general_purpose::STANDARD.encode(mac);
        Ok(envelope)
    }

    /// Verify a sealed envelope. Returns the body on success. Constant-time
    /// MAC compare; clock-skew window enforced; envelope kid is resolved
    /// through the configured provider so callers can rotate keys without
    /// changing the recipient.
    pub async fn open(&self, envelope: &SealedEnvelope) -> Result<serde_json::Value, CoolError> {
        if envelope.alg != "HS256" {
            return Err(CoolError::Unauthorized(format!(
                "unsupported envelope algorithm '{}'",
                envelope.alg,
            )));
        }
        let now = chrono::Utc::now().timestamp();
        let drift = (now - envelope.ts).abs();
        if drift > self.clock_skew_secs {
            return Err(CoolError::Unauthorized(
                "envelope timestamp outside accepted skew window".to_owned(),
            ));
        }
        let key = self.keys.resolve_signing_key(&envelope.kid).await?;
        let input = envelope.signing_input()?;
        let expected = self.compute_mac(&key, &input).await?;
        use base64::Engine;
        let actual = base64::engine::general_purpose::STANDARD
            .decode(&envelope.mac_b64)
            .map_err(|_| CoolError::Unauthorized("envelope MAC is not base64".to_owned()))?;
        if actual.len() != expected.len() {
            return Err(CoolError::Unauthorized(
                "envelope MAC has wrong length".to_owned(),
            ));
        }
        use subtle::ConstantTimeEq;
        if !bool::from(actual.as_slice().ct_eq(expected.as_slice())) {
            return Err(CoolError::Unauthorized(
                "envelope MAC verification failed".to_owned(),
            ));
        }
        if let Some(nonces) = &self.nonces {
            let expires_at = chrono::DateTime::<chrono::Utc>::from_timestamp(
                envelope.ts + self.clock_skew_secs,
                0,
            )
            .ok_or_else(|| CoolError::Unauthorized("envelope timestamp out of range".to_owned()))?;
            let recorded = nonces.record_if_unseen(&envelope.nonce, expires_at).await?;
            if !recorded {
                return Err(CoolError::Unauthorized(
                    "envelope nonce replay detected".to_owned(),
                ));
            }
        }
        Ok(envelope.body.clone())
    }
}

pub type CoolBody = bytes::Bytes;
pub type CoolEventFuture = Pin<Box<dyn Future<Output = Result<(), CoolError>> + Send + 'static>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    pub datasource: Option<Datasource>,
    pub auth: Option<AuthBlock>,
    pub config_blocks: Vec<ConfigBlock>,
    pub mixins: Vec<MixinDecl>,
    pub models: Vec<Model>,
    pub types: Vec<TypeDecl>,
    pub enums: Vec<EnumDecl>,
    pub procedures: Vec<Procedure>,
}

impl Schema {
    pub fn summary(&self) -> OwnedSchemaSummary {
        OwnedSchemaSummary {
            mixins: self.mixins.iter().map(|mixin| mixin.name.clone()).collect(),
            models: self.models.iter().map(|model| model.name.clone()).collect(),
            types: self.types.iter().map(|ty| ty.name.clone()).collect(),
            enums: self
                .enums
                .iter()
                .map(|enum_decl| enum_decl.name.clone())
                .collect(),
            procedures: self
                .procedures
                .iter()
                .map(|procedure| procedure.name.clone())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaSummary {
    pub mixins: &'static [&'static str],
    pub models: &'static [&'static str],
    pub types: &'static [&'static str],
    pub enums: &'static [&'static str],
    pub procedures: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedSchemaSummary {
    pub mixins: Vec<String>,
    pub models: Vec<String>,
    pub types: Vec<String>,
    pub enums: Vec<String>,
    pub procedures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Datasource {
    pub docs: Vec<String>,
    pub name: String,
    pub entries: Vec<ConfigEntry>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthBlock {
    pub docs: Vec<String>,
    pub name: String,
    pub fields: Vec<Field>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigBlock {
    pub docs: Vec<String>,
    pub name: String,
    pub entries: Vec<String>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    pub fields: Vec<Field>,
    pub attributes: Vec<Attribute>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixinDecl {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    pub fields: Vec<Field>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeDecl {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    pub fields: Vec<Field>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumDecl {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    pub variants: Vec<EnumVariant>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumVariant {
    pub docs: Vec<String>,
    pub name: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    pub ty: TypeRef,
    pub attributes: Vec<Attribute>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeRef {
    pub name: String,
    pub name_span: SourceSpan,
    pub arity: TypeArity,
    pub generic_args: Vec<TypeRef>,
}

impl TypeRef {
    pub fn is_page(&self) -> bool {
        self.name == "Page"
    }

    pub fn page_item(&self) -> Option<&TypeRef> {
        if self.is_page() {
            self.generic_args.first()
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeArity {
    Required,
    Optional,
    List,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub has_next_page: bool,
    pub has_previous_page: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total_count: Option<i64>,
    pub page_info: PageInfo,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>, page_info: PageInfo) -> Self {
        Self {
            items,
            total_count: None,
            page_info,
        }
    }

    pub fn with_total_count(mut self, total_count: Option<i64>) -> Self {
        self.total_count = total_count;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Procedure {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    pub kind: ProcedureKind,
    pub args: Vec<ProcedureArg>,
    pub return_type: TypeRef,
    pub attributes: Vec<Attribute>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcedureKind {
    Query,
    Mutation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcedureArg {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    pub ty: TypeRef,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribute {
    pub raw: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionQuery {
    pub fields: Vec<String>,
    pub includes: Vec<String>,
    pub include_fields: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelEventKind {
    Created,
    Updated,
    Deleted,
}

impl ModelEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
            Self::Deleted => "deleted",
        }
    }

    pub fn parse(value: &str) -> Result<Self, CoolError> {
        match value {
            "created" => Ok(Self::Created),
            "updated" => Ok(Self::Updated),
            "deleted" => Ok(Self::Deleted),
            other => Err(CoolError::Validation(format!(
                "unsupported model event operation `{other}`"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolEventEnvelope {
    pub event_id: uuid::Uuid,
    pub model: String,
    pub operation: ModelEventKind,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelEvent<T> {
    pub event_id: uuid::Uuid,
    pub model: String,
    pub operation: ModelEventKind,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub data: T,
}

impl<T> TryFrom<CoolEventEnvelope> for ModelEvent<T>
where
    T: serde::de::DeserializeOwned,
{
    type Error = CoolError;

    fn try_from(value: CoolEventEnvelope) -> Result<Self, Self::Error> {
        Ok(Self {
            event_id: value.event_id,
            model: value.model,
            operation: value.operation,
            occurred_at: value.occurred_at,
            data: serde_json::from_value(value.data).map_err(|error| {
                CoolError::Codec(format!("failed to decode event payload: {error}"))
            })?,
        })
    }
}

type EventHandler = Arc<dyn Fn(CoolEventEnvelope) -> CoolEventFuture + Send + Sync>;

#[derive(Clone, Default)]
pub struct CoolEventBus {
    handlers: Arc<RwLock<BTreeMap<String, Vec<EventHandler>>>>,
}

impl std::fmt::Debug for CoolEventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let handler_count = self
            .handlers
            .read()
            .map(|handlers| handlers.values().map(Vec::len).sum::<usize>())
            .unwrap_or_default();
        f.debug_struct("CoolEventBus")
            .field("handler_count", &handler_count)
            .finish()
    }
}

impl CoolEventBus {
    pub fn subscribe<F>(&self, model: &'static str, operation: ModelEventKind, handler: F)
    where
        F: Fn(CoolEventEnvelope) -> CoolEventFuture + Send + Sync + 'static,
    {
        let mut handlers = self
            .handlers
            .write()
            .expect("event bus handler registry should not be poisoned");
        handlers
            .entry(event_topic(model, operation))
            .or_default()
            .push(Arc::new(handler));
    }

    pub async fn emit(&self, envelope: CoolEventEnvelope) -> Result<(), CoolError> {
        let handlers = self
            .handlers
            .read()
            .expect("event bus handler registry should not be poisoned")
            .get(&event_topic(&envelope.model, envelope.operation))
            .cloned()
            .unwrap_or_default();

        for handler in handlers {
            handler(envelope.clone()).await?;
        }

        Ok(())
    }
}

pub fn event_topic(model: &str, operation: ModelEventKind) -> String {
    format!("{}.{}", model, operation.as_str())
}

pub fn parse_emit_attribute(raw: &str) -> Result<Vec<ModelEventKind>, String> {
    let Some(inner) = raw
        .strip_prefix("@@emit(")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Err(format!("unsupported event attribute `{raw}`"));
    };

    let mut operations = Vec::new();
    for part in inner
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let operation = match part {
            "created" => ModelEventKind::Created,
            "updated" => ModelEventKind::Updated,
            "deleted" => ModelEventKind::Deleted,
            other => {
                return Err(format!(
                    "unsupported event operation `{other}` in `{raw}`; expected created, updated, or deleted"
                ));
            }
        };
        if !operations.contains(&operation) {
            operations.push(operation);
        }
    }

    if operations.is_empty() {
        return Err(format!(
            "event attribute `{raw}` must declare at least one operation"
        ));
    }

    Ok(operations)
}

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

impl SelectionQuery {
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.includes.is_empty() && self.include_fields.is_empty()
    }
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CoolAuthIdentity {
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrincipalFacet {
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrincipalContext {
    pub actor: Option<PrincipalFacet>,
    pub session: Option<PrincipalFacet>,
    pub tenant: Option<PrincipalFacet>,
    pub claims: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CoolContext {
    pub auth: Option<CoolAuthIdentity>,
    pub principal: Option<PrincipalContext>,
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy)]
pub struct RequestContext<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: Option<&'a str>,
    pub headers: &'a http::HeaderMap,
    pub body: &'a [u8],
}

pub trait AuthProvider: Clone + Send + Sync + 'static {
    type Error: Into<CoolError> + Send;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl ::core::future::Future<Output = Result<CoolContext, Self::Error>> + Send;
}

impl<F, E> AuthProvider for F
where
    F: Clone + Send + Sync + 'static + for<'a> Fn(&'a http::HeaderMap) -> Result<CoolContext, E>,
    E: Into<CoolError> + Send,
{
    type Error = E;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl ::core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        let result = (self)(request.headers);
        ::core::future::ready(result)
    }
}

impl CoolContext {
    pub fn anonymous() -> Self {
        Self::default()
    }

    pub fn authenticated(fields: impl IntoIterator<Item = (String, Value)>) -> Self {
        let fields = fields.into_iter().collect::<BTreeMap<_, _>>();
        Self {
            auth: Some(CoolAuthIdentity {
                fields: fields.clone(),
            }),
            principal: Some(PrincipalContext::from_claims(fields)),
            extensions: BTreeMap::new(),
        }
    }

    pub fn is_authenticated(&self) -> bool {
        self.auth.is_some() || self.principal.is_some()
    }

    pub fn auth_field(&self, name: &str) -> Option<&Value> {
        if let Some(auth) = self.auth.as_ref()
            && let Some(value) = auth
                .fields
                .get(name)
                .or_else(|| lookup_value_path_in_map(&auth.fields, name))
        {
            return Some(value);
        }

        self.principal
            .as_ref()
            .and_then(|principal| principal.field(name))
    }

    pub fn from_principal<P: Serialize>(principal: Option<P>) -> Result<Self, CoolError> {
        let Some(principal) = principal else {
            return Ok(Self::anonymous());
        };

        let principal = PrincipalContext::from_principal(principal)?;
        let auth = principal.as_auth_identity();
        Ok(Self {
            auth: Some(auth),
            principal: Some(principal),
            extensions: BTreeMap::new(),
        })
    }

    pub fn with_principal(principal: PrincipalContext) -> Self {
        Self {
            auth: Some(principal.as_auth_identity()),
            principal: Some(principal),
            extensions: BTreeMap::new(),
        }
    }

    /// Convenience accessor for the principal's actor id. Falls back from
    /// the structured `principal.actor.id` facet to `principal.claims.id`
    /// and `auth.fields.id` so audit rows capture an identity regardless of
    /// which CoolContext builder the caller used.
    pub fn principal_actor_id(&self) -> Option<&str> {
        let from_facet = self
            .principal
            .as_ref()
            .and_then(|p| p.actor.as_ref())
            .and_then(|facet| facet.fields.get("id"));
        let from_claims = self.principal.as_ref().and_then(|p| p.claims.get("id"));
        let from_auth = self.auth.as_ref().and_then(|auth| auth.fields.get("id"));
        from_facet
            .or(from_claims)
            .or(from_auth)
            .and_then(|v| match v {
                Value::String(s) => Some(s.as_str()),
                _ => None,
            })
    }

    /// Tenant id surfaced for audit/log scoping.
    pub fn tenant_id(&self) -> Option<&str> {
        self.principal
            .as_ref()
            .and_then(|p| p.tenant.as_ref())
            .and_then(|facet| facet.fields.get("id"))
            .and_then(|v| match v {
                Value::String(s) => Some(s.as_str()),
                _ => None,
            })
    }

    /// Client IP if one was injected into the extensions map (typically by
    /// the auth provider after parsing `X-Forwarded-For` or the socket
    /// remote-addr).
    pub fn client_ip(&self) -> Option<&str> {
        self.extensions.get("client_ip").and_then(|v| match v {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        })
    }

    /// W3C `traceparent` value, if surfaced into the context by the
    /// correlation-id middleware.
    pub fn request_id(&self) -> Option<&str> {
        self.extensions.get("request_id").and_then(|v| match v {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        })
    }

    /// Snapshot of principal claims for audit recording. Returns the full
    /// claims map regardless of nesting depth, so forensic queries can pivot
    /// on any claim later. Returns an empty map for anonymous contexts.
    pub fn audit_claims_snapshot(&self) -> BTreeMap<String, Value> {
        self.principal
            .as_ref()
            .map(|p| p.claims.clone())
            .unwrap_or_default()
    }

    /// Attach a W3C `traceparent`-style request id to the context. Surfaces
    /// in tracing spans and is recorded on audit events so SIEM tools can
    /// stitch the trail across systems.
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.extensions
            .insert("request_id".to_owned(), Value::String(request_id.into()));
        self
    }

    /// Attach a client IP for the same reasons as `with_request_id`. Banks
    /// generally derive this from `X-Forwarded-For` or the socket address
    /// inside the auth provider.
    pub fn with_client_ip(mut self, ip: impl Into<String>) -> Self {
        self.extensions
            .insert("client_ip".to_owned(), Value::String(ip.into()));
        self
    }
}

impl PrincipalContext {
    pub fn from_principal<P: Serialize>(principal: P) -> Result<Self, CoolError> {
        let auth = CoolAuthIdentity::from_principal(principal)?;
        Ok(Self::from_auth_identity(&auth))
    }

    pub fn from_claims(claims: BTreeMap<String, Value>) -> Self {
        Self {
            actor: None,
            session: None,
            tenant: None,
            claims,
        }
    }

    pub fn from_auth_identity(auth: &CoolAuthIdentity) -> Self {
        let mut claims = auth.fields.clone();
        let actor = take_principal_facet(&mut claims, "actor");
        let session = take_principal_facet(&mut claims, "session");
        let tenant = take_principal_facet(&mut claims, "tenant");
        Self {
            actor,
            session,
            tenant,
            claims,
        }
    }

    pub fn field(&self, name: &str) -> Option<&Value> {
        if let Some(value) = self
            .claims
            .get(name)
            .or_else(|| lookup_value_path_in_map(&self.claims, name))
        {
            return Some(value);
        }

        let (root, rest) = name.split_once('.')?;
        match root {
            "actor" => lookup_principal_facet_path(self.actor.as_ref(), rest),
            "session" => lookup_principal_facet_path(self.session.as_ref(), rest),
            "tenant" => lookup_principal_facet_path(self.tenant.as_ref(), rest),
            _ => None,
        }
    }

    pub fn as_auth_identity(&self) -> CoolAuthIdentity {
        CoolAuthIdentity {
            fields: self.legacy_fields(),
        }
    }

    pub fn legacy_fields(&self) -> BTreeMap<String, Value> {
        let mut fields = self.claims.clone();
        if let Some(actor) = &self.actor {
            fields.insert("actor".to_owned(), Value::Map(actor.fields.clone()));
        }
        if let Some(session) = &self.session {
            fields.insert("session".to_owned(), Value::Map(session.fields.clone()));
        }
        if let Some(tenant) = &self.tenant {
            fields.insert("tenant".to_owned(), Value::Map(tenant.fields.clone()));
        }
        fields
    }
}

impl CoolAuthIdentity {
    pub fn from_principal<P: Serialize>(principal: P) -> Result<Self, CoolError> {
        let value = serde_json::to_value(principal).map_err(|error| {
            CoolError::Internal(format!("failed to serialize auth principal: {error}"))
        })?;
        let serde_json::Value::Object(object) = value else {
            return Err(CoolError::Internal(
                "auth principal must serialize to a JSON object".to_owned(),
            ));
        };

        let mut fields = BTreeMap::new();
        for (key, value) in object {
            fields.insert(key, json_value_to_cool_value(value)?);
        }

        Ok(Self { fields })
    }
}

fn json_value_to_cool_value(value: serde_json::Value) -> Result<Value, CoolError> {
    match value {
        serde_json::Value::Null => Ok(Value::Null),
        serde_json::Value::Bool(value) => Ok(Value::Bool(value)),
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Ok(Value::Int(value))
            } else if let Some(value) = number.as_f64() {
                Ok(Value::Float(value))
            } else {
                Err(CoolError::Internal(format!(
                    "unsupported auth principal number '{number}'"
                )))
            }
        }
        serde_json::Value::String(value) => Ok(Value::String(value)),
        serde_json::Value::Array(values) => values
            .into_iter()
            .map(json_value_to_cool_value)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List),
        serde_json::Value::Object(object) => object
            .into_iter()
            .map(|(key, value)| json_value_to_cool_value(value).map(|value| (key, value)))
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map(Value::Map),
    }
}

fn lookup_value_path_in_map<'a>(map: &'a BTreeMap<String, Value>, path: &str) -> Option<&'a Value> {
    let mut segments = path.split('.');
    let first = segments.next()?;
    let mut current = map.get(first)?;
    for segment in segments {
        current = match current {
            Value::Map(entries) => entries.get(segment)?,
            _ => return None,
        };
    }
    Some(current)
}

fn lookup_principal_facet_path<'a>(
    facet: Option<&'a PrincipalFacet>,
    path: &str,
) -> Option<&'a Value> {
    let facet = facet?;
    facet
        .fields
        .get(path)
        .or_else(|| lookup_value_path_in_map(&facet.fields, path))
}

fn take_principal_facet(claims: &mut BTreeMap<String, Value>, key: &str) -> Option<PrincipalFacet> {
    match claims.remove(key) {
        Some(Value::Map(fields)) => Some(PrincipalFacet { fields }),
        Some(value) => {
            claims.insert(key.to_owned(), value);
            None
        }
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_field_prefers_exact_key_before_dotted_lookup() {
        let ctx = CoolContext::authenticated([
            ("tenant.slug".to_owned(), Value::String("exact".to_owned())),
            (
                "tenant".to_owned(),
                Value::Map(BTreeMap::from([(
                    "slug".to_owned(),
                    Value::String("nested".to_owned()),
                )])),
            ),
        ]);

        assert_eq!(
            ctx.auth_field("tenant.slug"),
            Some(&Value::String("exact".to_owned()))
        );
    }

    #[test]
    fn auth_field_resolves_nested_map_paths() {
        let ctx = CoolContext::from_principal(Some(serde_json::json!({
            "tenant": {
                "slug": "acme",
                "owner": { "id": 7 }
            }
        })))
        .expect("principal should bind");

        assert_eq!(
            ctx.auth_field("tenant.slug"),
            Some(&Value::String("acme".to_owned()))
        );
        assert_eq!(ctx.auth_field("tenant.owner.id"), Some(&Value::Int(7)));
        assert!(ctx.auth_field("tenant.owner.missing").is_none());
    }

    #[test]
    fn from_principal_promotes_actor_session_and_tenant_facets() {
        let ctx = CoolContext::from_principal(Some(serde_json::json!({
            "actor": { "id": "usr_1" },
            "session": { "id": "sess_1" },
            "tenant": { "id": "org_1" },
            "role": "admin"
        })))
        .expect("principal should bind");

        let principal = ctx.principal.expect("principal should exist");
        assert_eq!(
            principal
                .actor
                .as_ref()
                .and_then(|facet| facet.fields.get("id")),
            Some(&Value::String("usr_1".to_owned()))
        );
        assert_eq!(
            principal
                .session
                .as_ref()
                .and_then(|facet| facet.fields.get("id")),
            Some(&Value::String("sess_1".to_owned()))
        );
        assert_eq!(
            principal
                .tenant
                .as_ref()
                .and_then(|facet| facet.fields.get("id")),
            Some(&Value::String("org_1".to_owned()))
        );
        assert_eq!(
            principal.claims.get("role"),
            Some(&Value::String("admin".to_owned()))
        );
    }

    #[test]
    fn internal_error_public_message_does_not_leak_detail() {
        let secret = "SELECT * FROM accounts WHERE pan = '4111-1111-1111-1111'";
        let err = CoolError::Internal(secret.to_owned());
        let response = err.into_response();
        assert_eq!(response.code, "INTERNAL_ERROR");
        assert_eq!(response.message, "internal error");
        assert!(
            !response.message.contains("SELECT"),
            "5xx public message must not echo internal detail",
        );
        assert!(
            !response.message.contains("4111"),
            "5xx public message must not echo any sensitive substring",
        );
        assert!(response.details.is_none());
    }

    #[test]
    fn database_error_public_message_is_canned() {
        let err = CoolError::Database("FATAL: connection refused at db.internal:5432".to_owned());
        assert_eq!(err.public_message(), "internal error");
        assert_eq!(
            err.detail(),
            Some("FATAL: connection refused at db.internal:5432")
        );
    }

    #[test]
    fn codec_error_public_message_is_canned() {
        let err = CoolError::Codec("malformed CBOR major type 7 at offset 42".to_owned());
        assert_eq!(err.public_message(), "invalid request payload");
        assert_eq!(
            err.detail(),
            Some("malformed CBOR major type 7 at offset 42")
        );
    }

    #[test]
    fn client_error_public_message_passes_through_caller_string() {
        let err = CoolError::BadRequest("missing query parameter 'limit'".to_owned());
        let response = err.into_response();
        assert_eq!(response.code, "BAD_REQUEST");
        assert_eq!(response.message, "missing query parameter 'limit'");
    }

    #[test]
    fn precondition_failed_maps_to_412() {
        let err = CoolError::PreconditionFailed("stale ETag".to_owned());
        assert_eq!(err.status_code(), StatusCode::PRECONDITION_FAILED);
        assert_eq!(err.code(), "PRECONDITION_FAILED");
        let response = err.into_response();
        assert_eq!(response.message, "stale ETag");
    }

    #[test]
    fn detail_is_none_for_empty_string() {
        let err = CoolError::Internal(String::new());
        assert_eq!(err.detail(), None);
    }

    #[test]
    fn into_response_never_populates_details_field() {
        for err in [
            CoolError::BadRequest("x".to_owned()),
            CoolError::Validation("y".to_owned()),
            CoolError::Internal("z".to_owned()),
            CoolError::Database("w".to_owned()),
            CoolError::Codec("v".to_owned()),
            CoolError::PreconditionFailed("u".to_owned()),
        ] {
            let response = err.into_response();
            assert!(
                response.details.is_none(),
                "details field must remain None until structured details are introduced",
            );
        }
    }
}

pub fn parse_cuid(value: &str) -> Result<String, CoolError> {
    if is_valid_cuid(value) {
        Ok(value.to_owned())
    } else {
        Err(CoolError::BadRequest(format!(
            "invalid cuid '{}': expected a lowercase alphanumeric id starting with 'c'",
            value,
        )))
    }
}

fn is_valid_cuid(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first != 'c' || value.len() < 2 {
        return false;
    }
    chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolErrorResponse {
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum CoolError {
    /// 4xx — `String` is the public message returned to the client.
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("not acceptable: {0}")]
    NotAcceptable(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("unsupported media type: {0}")]
    UnsupportedMediaType(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("validation: {0}")]
    Validation(String),
    #[error("precondition failed: {0}")]
    PreconditionFailed(String),
    /// 5xx — `String` is operator-only detail. Never returned to clients;
    /// the public message is a fixed canned string per variant.
    #[error("codec: {0}")]
    Codec(String),
    #[error("database: {0}")]
    Database(String),
    #[error("internal: {0}")]
    Internal(String),
}

impl CoolError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "BAD_REQUEST",
            Self::NotAcceptable(_) => "NOT_ACCEPTABLE",
            Self::Unauthorized(_) => "UNAUTHORIZED",
            Self::UnsupportedMediaType(_) => "UNSUPPORTED_MEDIA_TYPE",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::NotFound(_) => "NOT_FOUND",
            Self::Conflict(_) => "CONFLICT",
            Self::Validation(_) => "VALIDATION_ERROR",
            Self::PreconditionFailed(_) => "PRECONDITION_FAILED",
            Self::Codec(_) => "CODEC_ERROR",
            Self::Database(_) => "DATABASE_ERROR",
            Self::Internal(_) => "INTERNAL_ERROR",
        }
    }

    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::NotAcceptable(_) => StatusCode::NOT_ACCEPTABLE,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::UnsupportedMediaType(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::PreconditionFailed(_) => StatusCode::PRECONDITION_FAILED,
            Self::Codec(_) => StatusCode::BAD_REQUEST,
            Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Public, safe-to-expose message returned in HTTP responses.
    ///
    /// For 4xx variants this is the caller-supplied string. For 5xx variants
    /// this is a fixed canned message; the caller-supplied string flows to
    /// `detail` instead and is recorded via tracing only.
    pub fn public_message(&self) -> Cow<'_, str> {
        match self {
            Self::BadRequest(s)
            | Self::NotAcceptable(s)
            | Self::Unauthorized(s)
            | Self::UnsupportedMediaType(s)
            | Self::Forbidden(s)
            | Self::NotFound(s)
            | Self::Conflict(s)
            | Self::Validation(s)
            | Self::PreconditionFailed(s) => Cow::Borrowed(s.as_str()),
            Self::Codec(_) => Cow::Borrowed("invalid request payload"),
            Self::Database(_) => Cow::Borrowed("internal error"),
            Self::Internal(_) => Cow::Borrowed("internal error"),
        }
    }

    /// Operator-only detail string. For 5xx variants this is the message
    /// supplied at construction time; for 4xx variants this returns the same
    /// string as `public_message` (callers are expected to pre-redact 4xx
    /// messages they emit).
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::BadRequest(s)
            | Self::NotAcceptable(s)
            | Self::Unauthorized(s)
            | Self::UnsupportedMediaType(s)
            | Self::Forbidden(s)
            | Self::NotFound(s)
            | Self::Conflict(s)
            | Self::Validation(s)
            | Self::PreconditionFailed(s)
            | Self::Codec(s)
            | Self::Database(s)
            | Self::Internal(s) => {
                if s.is_empty() {
                    None
                } else {
                    Some(s.as_str())
                }
            }
        }
    }

    pub fn into_response(self) -> CoolErrorResponse {
        let code = self.code().to_owned();
        let message = self.public_message().into_owned();
        CoolErrorResponse {
            code,
            message,
            details: None,
        }
    }
}

pub trait CoolCodec: Clone + Send + Sync + 'static {
    const CONTENT_TYPE: &'static str;

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CoolError>;

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CoolError>;
}

pub trait CoolEnvelope: Clone + Send + Sync + 'static {
    fn request_content_type(&self) -> &'static str;

    fn response_content_type(&self) -> &'static str;

    fn open_request(&self, bytes: &[u8], _ctx: &mut CoolContext) -> Result<Vec<u8>, CoolError>;

    fn seal_response(&self, bytes: &[u8], _ctx: &CoolContext) -> Result<Vec<u8>, CoolError>;
}

#[derive(Debug, Clone, Default)]
pub struct NoEnvelope;

impl CoolEnvelope for NoEnvelope {
    fn request_content_type(&self) -> &'static str {
        "application/octet-stream"
    }

    fn response_content_type(&self) -> &'static str {
        "application/octet-stream"
    }

    fn open_request(&self, bytes: &[u8], _ctx: &mut CoolContext) -> Result<Vec<u8>, CoolError> {
        Ok(bytes.to_vec())
    }

    fn seal_response(&self, bytes: &[u8], _ctx: &CoolContext) -> Result<Vec<u8>, CoolError> {
        Ok(bytes.to_vec())
    }
}

// -----------------------------------------------------------------------------
// Field-level validators
//
// Standalone helpers invoked from generated `validate` methods on Create/Update
// input structs. Each returns `Ok(())` on success or a redacted
// `CoolError::Validation` whose public message names the field but never
// echoes the rejected value (so PII does not leak via 422 bodies).
// -----------------------------------------------------------------------------

pub fn validate_length(
    field: &'static str,
    value: &str,
    min: Option<usize>,
    max: Option<usize>,
) -> Result<(), CoolError> {
    let len = value.chars().count();
    if let Some(min) = min {
        if len < min {
            return Err(CoolError::Validation(format!(
                "field '{field}' length {len} is below minimum {min}",
            )));
        }
    }
    if let Some(max) = max {
        if len > max {
            return Err(CoolError::Validation(format!(
                "field '{field}' length {len} exceeds maximum {max}",
            )));
        }
    }
    Ok(())
}

pub fn validate_range_i64(
    field: &'static str,
    value: i64,
    min: Option<i64>,
    max: Option<i64>,
) -> Result<(), CoolError> {
    if let Some(min) = min {
        if value < min {
            return Err(CoolError::Validation(format!(
                "field '{field}' is below minimum {min}",
            )));
        }
    }
    if let Some(max) = max {
        if value > max {
            return Err(CoolError::Validation(format!(
                "field '{field}' exceeds maximum {max}",
            )));
        }
    }
    Ok(())
}

/// Pragmatic email check: requires exactly one `@`, non-empty local and domain
/// parts, at least one `.` in the domain, and no whitespace. Not a full RFC
/// 5322 grammar — that grammar admits forms (quoted local parts, IP literals)
/// banks rarely accept anyway. Reject early; let real KYC flows do deeper
/// validation.
pub fn validate_email(field: &'static str, value: &str) -> Result<(), CoolError> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.chars().any(char::is_whitespace)
        || trimmed.chars().filter(|c| *c == '@').count() != 1
    {
        return Err(CoolError::Validation(format!(
            "field '{field}' is not a valid email address",
        )));
    }
    let (local, domain) = trimmed.split_once('@').unwrap();
    if local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return Err(CoolError::Validation(format!(
            "field '{field}' is not a valid email address",
        )));
    }
    Ok(())
}

pub fn validate_uri(field: &'static str, value: &str) -> Result<(), CoolError> {
    if url::Url::parse(value).is_err() {
        return Err(CoolError::Validation(format!(
            "field '{field}' is not a valid URI",
        )));
    }
    Ok(())
}

/// ISO 4217 currency codes are 3 ASCII uppercase letters. We do not enforce
/// the registered set here — that table churns and is downstream policy.
/// Banks typically pin allowed currencies via a separate allow-list anyway.
pub fn validate_iso4217(field: &'static str, value: &str) -> Result<(), CoolError> {
    if value.len() != 3 || !value.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(CoolError::Validation(format!(
            "field '{field}' must be a 3-letter uppercase ISO 4217 code",
        )));
    }
    Ok(())
}

#[cfg(test)]
mod validator_tests {
    use super::*;

    #[test]
    fn length_rejects_below_min_and_above_max() {
        assert!(validate_length("name", "ab", Some(3), None).is_err());
        assert!(validate_length("name", "abcd", None, Some(3)).is_err());
        assert!(validate_length("name", "abc", Some(3), Some(3)).is_ok());
    }

    #[test]
    fn email_accepts_simple_form_and_rejects_bad_shapes() {
        assert!(validate_email("e", "alice@example.com").is_ok());
        assert!(validate_email("e", "alice@example").is_err());
        assert!(validate_email("e", "alice@@example.com").is_err());
        assert!(validate_email("e", "alice example.com").is_err());
        assert!(validate_email("e", "@example.com").is_err());
    }

    #[test]
    fn iso4217_requires_three_uppercase_letters() {
        assert!(validate_iso4217("currency", "USD").is_ok());
        assert!(validate_iso4217("currency", "usd").is_err());
        assert!(validate_iso4217("currency", "USDX").is_err());
        assert!(validate_iso4217("currency", "U1D").is_err());
    }

    #[test]
    fn range_i64_enforces_inclusive_bounds() {
        assert!(validate_range_i64("n", 5, Some(0), Some(10)).is_ok());
        assert!(validate_range_i64("n", -1, Some(0), None).is_err());
        assert!(validate_range_i64("n", 11, None, Some(10)).is_err());
    }

    #[test]
    fn validation_error_does_not_echo_value() {
        let err = validate_email("primary_email", "not-an-email").unwrap_err();
        let msg = err.public_message().into_owned();
        assert!(
            !msg.contains("not-an-email"),
            "validation message must not echo the rejected value: {msg}",
        );
    }

    #[cfg(feature = "decimal-rust-decimal")]
    #[test]
    fn decimal_alias_round_trips_through_json_as_string() {
        use std::str::FromStr;
        let value = Decimal::from_str("1234.56").unwrap();
        let encoded = serde_json::to_string(&value).unwrap();
        // `serde-str` makes Decimal serialize as a JSON string. Critical so
        // amounts never round-trip through f64.
        assert_eq!(encoded, "\"1234.56\"");
        let decoded: Decimal = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn request_id_round_trip_through_extensions() {
        let ctx = CoolContext::anonymous().with_request_id("trace-123");
        assert_eq!(ctx.request_id(), Some("trace-123"));
    }

    #[test]
    fn client_ip_round_trip_through_extensions() {
        let ctx = CoolContext::anonymous().with_client_ip("192.0.2.43");
        assert_eq!(ctx.client_ip(), Some("192.0.2.43"));
    }

    #[tokio::test]
    async fn hmac_envelope_round_trip_succeeds() {
        let keys = Arc::new(StaticKeyProvider::new().with_key("ops-1", vec![0xaa; 32]));
        let env = HmacEnvelope::new(keys.clone(), "ops-1");
        let payload = serde_json::json!({ "transfer": { "amount": "100.00" } });
        let sealed = env.seal(payload.clone()).await.expect("seal");
        let opened = env.open(&sealed).await.expect("open");
        assert_eq!(opened, payload);
    }

    #[tokio::test]
    async fn hmac_envelope_rejects_modified_body() {
        let keys = Arc::new(StaticKeyProvider::new().with_key("ops-1", vec![0xaa; 32]));
        let env = HmacEnvelope::new(keys.clone(), "ops-1");
        let mut sealed = env
            .seal(serde_json::json!({ "amount": "100" }))
            .await
            .expect("seal");
        sealed.body = serde_json::json!({ "amount": "999" });
        let err = env.open(&sealed).await.expect_err("must reject tamper");
        assert_eq!(err.code(), "UNAUTHORIZED");
    }

    #[tokio::test]
    async fn hmac_envelope_rejects_stale_timestamp() {
        let keys = Arc::new(StaticKeyProvider::new().with_key("ops-1", vec![0xaa; 32]));
        let env = HmacEnvelope::new(keys.clone(), "ops-1").with_clock_skew_secs(1);
        let mut sealed = env.seal(serde_json::json!({})).await.expect("seal");
        // Push the timestamp into the past beyond the skew window.
        sealed.ts -= 60;
        // Recompute MAC to ensure the envelope is structurally valid —
        // we want to isolate that the timestamp window is what blocks it.
        use base64::Engine;
        use hmac::{Hmac, Mac};
        let key = keys.resolve_signing_key("ops-1").await.expect("key");
        let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(&key).unwrap();
        mac.update(&sealed.signing_input().unwrap());
        sealed.mac_b64 =
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

        let err = env.open(&sealed).await.expect_err("must reject");
        assert_eq!(err.code(), "UNAUTHORIZED");
    }

    #[tokio::test]
    async fn hmac_envelope_with_nonce_store_rejects_replays() {
        let keys = Arc::new(StaticKeyProvider::new().with_key("ops-1", vec![0xaa; 32]));
        let nonces: Arc<dyn NonceStore> = Arc::new(InMemoryNonceStore::new());
        let env = HmacEnvelope::new(keys.clone(), "ops-1").with_nonce_store(nonces.clone());
        let sealed = env
            .seal(serde_json::json!({ "amount": "1" }))
            .await
            .expect("seal");
        env.open(&sealed).await.expect("first open succeeds");
        let err = env.open(&sealed).await.expect_err("replay must fail");
        assert_eq!(err.code(), "UNAUTHORIZED");
    }

    #[tokio::test]
    async fn nonce_store_purges_expired_entries() {
        let store = InMemoryNonceStore::new();
        let past = chrono::Utc::now() - chrono::Duration::seconds(60);
        // Insert an already-expired entry, then attempt to record the same
        // nonce again — the GC inside `record_if_unseen` should evict it.
        assert!(store.record_if_unseen("n1", past).await.unwrap());
        assert!(
            store
                .record_if_unseen("n1", past + chrono::Duration::seconds(120))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn hmac_envelope_rejects_unknown_alg() {
        let keys = Arc::new(StaticKeyProvider::new().with_key("ops-1", vec![0xaa; 32]));
        let env = HmacEnvelope::new(keys.clone(), "ops-1");
        let mut sealed = env.seal(serde_json::json!({})).await.expect("seal");
        sealed.alg = "none".to_owned();
        let err = env.open(&sealed).await.expect_err("must reject");
        assert_eq!(err.code(), "UNAUTHORIZED");
    }

    #[cfg(feature = "decimal-rust-decimal")]
    #[test]
    fn decimal_supports_precise_arithmetic() {
        use std::str::FromStr;
        // 0.1 + 0.2 — the canonical demonstration that f64 cannot represent
        // monetary arithmetic precisely.
        let a = Decimal::from_str("0.1").unwrap();
        let b = Decimal::from_str("0.2").unwrap();
        let sum = a + b;
        assert_eq!(sum, Decimal::from_str("0.3").unwrap());
    }
}
