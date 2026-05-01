use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use http::StatusCode;
use serde::{Deserialize, Serialize};

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
    pub models: Vec<Model>,
    pub types: Vec<TypeDecl>,
    pub enums: Vec<EnumDecl>,
    pub procedures: Vec<Procedure>,
}

impl Schema {
    pub fn summary(&self) -> OwnedSchemaSummary {
        OwnedSchemaSummary {
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
    pub models: &'static [&'static str],
    pub types: &'static [&'static str],
    pub enums: &'static [&'static str],
    pub procedures: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedSchemaSummary {
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
            Self::Codec(_) => StatusCode::BAD_REQUEST,
            Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn into_response(self) -> CoolErrorResponse {
        CoolErrorResponse {
            code: self.code().to_owned(),
            message: self.to_string(),
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
