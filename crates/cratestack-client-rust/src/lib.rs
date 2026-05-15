use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
pub use cratestack_codec_cbor::CborCodec;
pub use cratestack_codec_json::JsonCodec;
use cratestack_core::{
    CoolCodec, CoolError, CoolErrorResponse, Page, SelectionQuery, canonical_request_string,
};
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

const BRIDGE_CONTENT_TYPE: &str = "application/json";
const CBOR_SEQUENCE_CONTENT_TYPE: &str = "application/cbor-seq";

pub trait Projection {
    type Output;

    fn selection_query(&self) -> SelectionQuery;

    fn decode_one(&self, value: JsonValue) -> Result<Self::Output, CoolError>;

    fn decode_many(&self, value: JsonValue) -> Result<Vec<Self::Output>, CoolError> {
        match value {
            JsonValue::Array(values) => values
                .into_iter()
                .map(|value| self.decode_one(value))
                .collect(),
            other => Err(CoolError::Internal(format!(
                "projected list payload must be an array, got {other:?}"
            ))),
        }
    }

    fn decode_page(&self, value: JsonValue) -> Result<Page<Self::Output>, CoolError> {
        let page = serde_json::from_value::<Page<JsonValue>>(value).map_err(|error| {
            CoolError::Codec(format!("failed to decode projected page payload: {error}"))
        })?;
        let items = page
            .items
            .into_iter()
            .map(|value| self.decode_one(value))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::new(items, page.page_info).with_total_count(page.total_count))
    }
}

impl Projection for SelectionQuery {
    type Output = JsonValue;

    fn selection_query(&self) -> SelectionQuery {
        self.clone()
    }

    fn decode_one(&self, value: JsonValue) -> Result<Self::Output, CoolError> {
        Ok(value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum RuntimeCodecConfig {
    #[default]
    Cbor,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum RuntimeEnvelopeConfig {
    #[default]
    None,
    CoseSign1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeTransportConfig {
    pub codec: RuntimeCodecConfig,
    pub envelope: RuntimeEnvelopeConfig,
}

pub trait HttpClientCodec: CoolCodec {
    fn accept_header_value(&self) -> &'static str;

    fn sequence_accept_header_value(&self) -> &'static str;

    fn decode_response<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: DeserializeOwned;

    fn decode_sequence_response<T>(
        &self,
        content_type: &str,
        body: &[u8],
    ) -> Result<Vec<T>, CoolError>
    where
        T: DeserializeOwned;
}

impl HttpClientCodec for CborCodec {
    fn accept_header_value(&self) -> &'static str {
        "application/cbor, application/json"
    }

    fn sequence_accept_header_value(&self) -> &'static str {
        "application/cbor-seq, application/cbor, application/json"
    }

    fn decode_response<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: DeserializeOwned,
    {
        if media_type_matches(content_type, CborCodec::CONTENT_TYPE) {
            self.decode(body)
        } else if media_type_matches(content_type, JsonCodec::CONTENT_TYPE) {
            JsonCodec.decode(body)
        } else {
            Err(CoolError::Codec(format!(
                "unsupported response Content-Type {content_type}"
            )))
        }
    }

    fn decode_sequence_response<T>(
        &self,
        content_type: &str,
        body: &[u8],
    ) -> Result<Vec<T>, CoolError>
    where
        T: DeserializeOwned,
    {
        if media_type_matches(content_type, CBOR_SEQUENCE_CONTENT_TYPE) {
            decode_cbor_sequence(body)
        } else if media_type_matches(content_type, CborCodec::CONTENT_TYPE) {
            self.decode(body)
        } else if media_type_matches(content_type, JsonCodec::CONTENT_TYPE) {
            JsonCodec.decode(body)
        } else {
            Err(CoolError::Codec(format!(
                "unsupported response Content-Type {content_type}"
            )))
        }
    }
}

impl HttpClientCodec for JsonCodec {
    fn accept_header_value(&self) -> &'static str {
        "application/json, application/cbor"
    }

    fn sequence_accept_header_value(&self) -> &'static str {
        "application/cbor-seq, application/json, application/cbor"
    }

    fn decode_response<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: DeserializeOwned,
    {
        if media_type_matches(content_type, JsonCodec::CONTENT_TYPE) {
            self.decode(body)
        } else if media_type_matches(content_type, CborCodec::CONTENT_TYPE) {
            CborCodec.decode(body)
        } else {
            Err(CoolError::Codec(format!(
                "unsupported response Content-Type {content_type}"
            )))
        }
    }

    fn decode_sequence_response<T>(
        &self,
        content_type: &str,
        body: &[u8],
    ) -> Result<Vec<T>, CoolError>
    where
        T: DeserializeOwned,
    {
        if media_type_matches(content_type, CBOR_SEQUENCE_CONTENT_TYPE) {
            decode_cbor_sequence(body)
        } else if media_type_matches(content_type, JsonCodec::CONTENT_TYPE) {
            self.decode(body)
        } else if media_type_matches(content_type, CborCodec::CONTENT_TYPE) {
            CborCodec.decode(body)
        } else {
            Err(CoolError::Codec(format!(
                "unsupported response Content-Type {content_type}"
            )))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestJournalEntry {
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub content_type: Option<String>,
    pub recorded_at: DateTime<Utc>,
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedClientState {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub state_version: u64,
    #[serde(default)]
    pub request_journal: Vec<RequestJournalEntry>,
}

impl Default for PersistedClientState {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            state_version: 0,
            request_journal: Vec::new(),
        }
    }
}

pub trait ClientStateStore: Send + Sync {
    fn load(&self) -> Result<PersistedClientState, ClientError>;
    fn save(&self, state: &PersistedClientState) -> Result<(), ClientError>;

    fn append_request_journal(&self, entry: &RequestJournalEntry) -> Result<(), ClientError> {
        let mut state = self.load()?;
        state.request_journal.push(entry.clone());
        state.state_version = state.state_version.saturating_add(1);
        self.save(&state)
    }
}

#[derive(Debug, Default)]
pub struct InMemoryStateStore {
    state: Mutex<PersistedClientState>,
}

impl ClientStateStore for InMemoryStateStore {
    fn load(&self) -> Result<PersistedClientState, ClientError> {
        self.state
            .lock()
            .map_err(|error| ClientError::State(format!("failed to lock state store: {error}")))
            .map(|state| state.clone())
    }

    fn save(&self, state: &PersistedClientState) -> Result<(), ClientError> {
        let mut guard = self
            .state
            .lock()
            .map_err(|error| ClientError::State(format!("failed to lock state store: {error}")))?;
        *guard = state.clone();
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct JsonFileStateStore {
    path: PathBuf,
}

impl JsonFileStateStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ClientStateStore for JsonFileStateStore {
    fn load(&self) -> Result<PersistedClientState, ClientError> {
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                ClientError::State(format!(
                    "failed to decode state file {}: {error}",
                    self.path.display()
                ))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(PersistedClientState::default())
            }
            Err(error) => Err(ClientError::State(format!(
                "failed to read state file {}: {error}",
                self.path.display()
            ))),
        }
    }

    fn save(&self, state: &PersistedClientState) -> Result<(), ClientError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                ClientError::State(format!(
                    "failed to create state directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
        let bytes = serde_json::to_vec_pretty(state).map_err(|error| {
            ClientError::State(format!(
                "failed to encode state file {}: {error}",
                self.path.display()
            ))
        })?;
        fs::write(&self.path, bytes).map_err(|error| {
            ClientError::State(format!(
                "failed to write state file {}: {error}",
                self.path.display()
            ))
        })
    }
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub base_url: Url,
}

impl ClientConfig {
    pub fn new(base_url: Url) -> Self {
        Self { base_url }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("codec error: {0}")]
    Codec(#[from] CoolError),
    #[error("state error: {0}")]
    State(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("bad input: {0}")]
    BadInput(String),
    #[error("remote call failed with status {status}: {message}")]
    Remote {
        status: StatusCode,
        error: Option<CoolErrorResponse>,
        message: String,
    },
}

pub type HeaderPair<'a> = (&'a str, &'a str);
pub type QueryPair<'a> = (&'a str, &'a str);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRequest {
    pub method: String,
    pub path: String,
    pub canonical_query: Option<String>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub canonical_request: String,
}

pub trait RequestAuthorizer: Send + Sync {
    fn authorize(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeRequestWire {
    pub method: String,
    pub path: String,
    pub canonical_query: Option<String>,
    pub headers: Vec<RuntimeHeader>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeResponseWire {
    pub status_code: u16,
    pub headers: Vec<RuntimeHeader>,
    pub body: Vec<u8>,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RuntimeErrorCode {
    Transport = 1,
    Codec = 2,
    State = 3,
    InvalidResponse = 4,
    Remote = 5,
    BadInput = 6,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeErrorWire {
    pub code: RuntimeErrorCode,
    pub http_status: Option<u16>,
    pub message: String,
    pub remote_code: Option<String>,
    pub remote_body: Option<Vec<u8>>,
}

impl From<ClientError> for RuntimeErrorWire {
    fn from(value: ClientError) -> Self {
        match value {
            ClientError::Transport(error) => Self {
                code: RuntimeErrorCode::Transport,
                http_status: None,
                message: error.to_string(),
                remote_code: None,
                remote_body: None,
            },
            ClientError::Codec(error) => Self {
                code: RuntimeErrorCode::Codec,
                http_status: Some(error.status_code().as_u16()),
                message: error.to_string(),
                remote_code: Some(error.code().to_owned()),
                remote_body: None,
            },
            ClientError::State(message) => Self {
                code: RuntimeErrorCode::State,
                http_status: None,
                message,
                remote_code: None,
                remote_body: None,
            },
            ClientError::InvalidResponse(message) => Self {
                code: RuntimeErrorCode::InvalidResponse,
                http_status: None,
                message,
                remote_code: None,
                remote_body: None,
            },
            ClientError::BadInput(message) => Self {
                code: RuntimeErrorCode::BadInput,
                http_status: None,
                message,
                remote_code: None,
                remote_body: None,
            },
            ClientError::Remote {
                status,
                error,
                message,
            } => Self {
                code: RuntimeErrorCode::Remote,
                http_status: Some(status.as_u16()),
                remote_code: error.as_ref().map(|value| value.code.clone()),
                remote_body: error
                    .as_ref()
                    .and_then(|value| serde_json::to_vec(value).ok()),
                message,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeStateStoreConfig {
    InMemory,
    JsonFile { path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfigWire {
    pub base_url: String,
    pub state_store: RuntimeStateStoreConfig,
    pub transport: RuntimeTransportConfig,
}

pub struct RuntimeHandle {
    runtime: tokio::runtime::Runtime,
    client: RuntimeTransportClient,
}

enum RuntimeTransportClient {
    Cbor(CratestackClient<CborCodec>),
    Json(CratestackClient<JsonCodec>),
}

impl RuntimeHandle {
    pub fn new(config: RuntimeConfigWire) -> Result<Self, RuntimeErrorWire> {
        let base_url = Url::parse(&config.base_url).map_err(|error| RuntimeErrorWire {
            code: RuntimeErrorCode::BadInput,
            http_status: None,
            message: format!("invalid base URL '{}': {error}", config.base_url),
            remote_code: None,
            remote_body: None,
        })?;
        let state_store: Arc<dyn ClientStateStore> = match config.state_store {
            RuntimeStateStoreConfig::InMemory => Arc::new(InMemoryStateStore::default()),
            RuntimeStateStoreConfig::JsonFile { path } => Arc::new(JsonFileStateStore::new(path)),
        };
        if config.transport.envelope != RuntimeEnvelopeConfig::None {
            return Err(RuntimeErrorWire {
                code: RuntimeErrorCode::BadInput,
                http_status: None,
                message: "COSE envelope support is not implemented yet".to_owned(),
                remote_code: None,
                remote_body: None,
            });
        }
        let client = match config.transport.codec {
            RuntimeCodecConfig::Cbor => RuntimeTransportClient::Cbor(
                CratestackClient::new(ClientConfig::new(base_url.clone()), CborCodec)
                    .with_state_store(state_store.clone()),
            ),
            RuntimeCodecConfig::Json => RuntimeTransportClient::Json(
                CratestackClient::new(ClientConfig::new(base_url), JsonCodec)
                    .with_state_store(state_store),
            ),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| RuntimeErrorWire {
                code: RuntimeErrorCode::State,
                http_status: None,
                message: format!("failed to build runtime: {error}"),
                remote_code: None,
                remote_body: None,
            })?;

        Ok(Self { runtime, client })
    }

    pub fn execute(
        &self,
        request: RuntimeRequestWire,
    ) -> Result<RuntimeResponseWire, RuntimeErrorWire> {
        self.runtime
            .block_on(self.client.execute_raw(request))
            .map_err(RuntimeErrorWire::from)
    }

    /// Streaming companion to [`Self::execute`]. The callback receives
    /// one [`RuntimeChunkWire`] per complete cbor-seq item as bytes
    /// arrive on the wire; returning `false` cancels the stream.
    /// Returns when the stream terminates (clean end, error, or
    /// cancellation).
    ///
    /// Designed for the FFI surface — the callback gets raw CBOR bytes
    /// per item, so the host language decodes with its native CBOR
    /// library (Dart, Swift, Kotlin) rather than carrying a typed
    /// generic across the bridge.
    pub fn execute_streamed<F>(
        &self,
        request: RuntimeRequestWire,
        on_chunk: F,
    ) -> Result<(), RuntimeErrorWire>
    where
        F: FnMut(RuntimeChunkWire) -> bool + Send,
    {
        self.runtime
            .block_on(self.client.execute_streamed(request, on_chunk))
    }

    pub fn state(&self) -> Result<PersistedClientState, ClientError> {
        self.client.state()
    }
}

async fn execute_streamed_transport<C>(
    client: &CratestackClient<C>,
    request: RuntimeRequestWire,
    accept: &'static str,
) -> Result<reqwest::Response, ClientError>
where
    C: HttpClientCodec,
{
    let method = Method::from_bytes(request.method.as_bytes()).map_err(|error| {
        ClientError::BadInput(format!("invalid HTTP method '{}': {error}", request.method))
    })?;
    let header_pairs = request
        .headers
        .iter()
        .map(|header| (header.name.as_str(), header.value.as_str()))
        .collect::<Vec<_>>();
    client
        .request_streamed_with_query_and_accept(
            method,
            &request.path,
            if request.body.is_empty() {
                None
            } else {
                Some(request.body)
            },
            request.canonical_query.as_deref(),
            &header_pairs,
            accept,
        )
        .await
}

fn replace_bridge_content_type(headers: &mut Vec<RuntimeHeader>) {
    headers.retain(|header| !header.name.eq_ignore_ascii_case("content-type"));
    headers.push(RuntimeHeader {
        name: "content-type".to_owned(),
        value: BRIDGE_CONTENT_TYPE.to_owned(),
    });
}

impl RuntimeTransportClient {
    async fn execute_raw(
        &self,
        request: RuntimeRequestWire,
    ) -> Result<RuntimeResponseWire, ClientError> {
        let request = self.bridge_request_to_transport(request)?;
        match self {
            Self::Cbor(client) => client.execute_raw_transport(request).await,
            Self::Json(client) => client.execute_raw_transport(request).await,
        }
        .and_then(|response| self.transport_response_to_bridge(response))
    }

    /// Streaming variant of `execute_raw` for the FFI surface. The
    /// callback receives one [`RuntimeChunkWire`] per complete
    /// cbor-seq item; returning `false` cancels the stream. Returns
    /// when the stream terminates by completion, error, or
    /// cancellation. The success response body is **not buffered** —
    /// items reach the callback as they arrive on the wire.
    async fn execute_streamed<F>(
        &self,
        request: RuntimeRequestWire,
        on_chunk: F,
    ) -> Result<(), RuntimeErrorWire>
    where
        F: FnMut(RuntimeChunkWire) -> bool + Send,
    {
        let request = self
            .bridge_request_to_transport(request)
            .map_err(RuntimeErrorWire::from)?;
        let response = match self {
            Self::Cbor(client) => {
                let accept = client.codec.sequence_accept_header_value();
                execute_streamed_transport(client, request, accept).await
            }
            Self::Json(client) => {
                let accept = client.codec.sequence_accept_header_value();
                execute_streamed_transport(client, request, accept).await
            }
        };
        let response = response.map_err(RuntimeErrorWire::from)?;
        pump_streamed_response_callback(response, on_chunk).await
    }

    fn bridge_request_to_transport(
        &self,
        request: RuntimeRequestWire,
    ) -> Result<RuntimeRequestWire, ClientError> {
        if request.body.is_empty() {
            return Ok(request);
        }

        let value: JsonValue = serde_json::from_slice(&request.body).map_err(|error| {
            ClientError::BadInput(format!("invalid bridge payload JSON: {error}"))
        })?;
        let body = match self {
            Self::Cbor(_) => CborCodec.encode(&value)?,
            Self::Json(_) => JsonCodec.encode(&value)?,
        };

        Ok(RuntimeRequestWire { body, ..request })
    }

    fn transport_response_to_bridge(
        &self,
        mut response: RuntimeResponseWire,
    ) -> Result<RuntimeResponseWire, ClientError> {
        if response.body.is_empty() {
            replace_bridge_content_type(&mut response.headers);
            return Ok(response);
        }

        let value = match self {
            Self::Cbor(_) => CborCodec.decode::<JsonValue>(&response.body)?,
            Self::Json(_) => JsonCodec.decode::<JsonValue>(&response.body)?,
        };

        response.body = serde_json::to_vec(&value).map_err(|error| {
            ClientError::InvalidResponse(format!("failed to encode bridge payload JSON: {error}"))
        })?;
        replace_bridge_content_type(&mut response.headers);
        Ok(response)
    }

    fn state(&self) -> Result<PersistedClientState, ClientError> {
        match self {
            Self::Cbor(client) => client.state(),
            Self::Json(client) => client.state(),
        }
    }
}

#[derive(Clone)]
pub struct CratestackClient<C = CborCodec> {
    http: reqwest::Client,
    config: ClientConfig,
    codec: C,
    state_store: Arc<dyn ClientStateStore>,
    request_authorizer: Option<Arc<dyn RequestAuthorizer>>,
}

impl CratestackClient<CborCodec> {
    pub fn cbor(config: ClientConfig) -> Self {
        Self::new(config, CborCodec)
    }
}

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    pub fn new(config: ClientConfig, codec: C) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
            codec,
            state_store: Arc::new(InMemoryStateStore::default()),
            request_authorizer: None,
        }
    }

    pub fn with_http_client(config: ClientConfig, codec: C, http: reqwest::Client) -> Self {
        Self {
            http,
            config,
            codec,
            state_store: Arc::new(InMemoryStateStore::default()),
            request_authorizer: None,
        }
    }

    pub fn with_state_store(mut self, state_store: Arc<dyn ClientStateStore>) -> Self {
        self.state_store = state_store;
        self
    }

    pub fn with_optional_state_store(self, state_store: Option<Arc<dyn ClientStateStore>>) -> Self {
        match state_store {
            Some(state_store) => self.with_state_store(state_store),
            None => self,
        }
    }

    pub fn with_request_authorizer(
        mut self,
        request_authorizer: Arc<dyn RequestAuthorizer>,
    ) -> Self {
        self.request_authorizer = Some(request_authorizer);
        self
    }

    pub fn state(&self) -> Result<PersistedClientState, ClientError> {
        self.state_store.load()
    }

    pub async fn get<Output>(
        &self,
        path: &str,
        query: &[QueryPair<'_>],
        headers: &[HeaderPair<'_>],
    ) -> Result<Output, ClientError>
    where
        Output: DeserializeOwned,
    {
        let response = self
            .request_raw(Method::GET, path, None, query, headers)
            .await?;
        decode_typed_response(&self.codec, &response)
    }

    pub async fn get_view<P>(
        &self,
        path: &str,
        projection: &P,
        headers: &[HeaderPair<'_>],
    ) -> Result<P::Output, ClientError>
    where
        P: Projection,
    {
        let selection = projection.selection_query();
        let canonical_query = canonical_query_from_selection(&selection, &[])?;
        let response = self
            .request_raw_with_query_and_accept(
                Method::GET,
                path,
                None,
                canonical_query.as_deref(),
                headers,
                Some(JsonCodec::CONTENT_TYPE),
            )
            .await?;
        let value = decode_json_value_response(&JsonCodec, &response)?;
        projection.decode_one(value).map_err(ClientError::from)
    }

    pub async fn list_view<P>(
        &self,
        path: &str,
        projection: &P,
        extra_query: &[QueryPair<'_>],
        headers: &[HeaderPair<'_>],
    ) -> Result<Vec<P::Output>, ClientError>
    where
        P: Projection,
    {
        let selection = projection.selection_query();
        let canonical_query = canonical_query_from_selection(&selection, extra_query)?;
        let response = self
            .request_raw_with_query_and_accept(
                Method::GET,
                path,
                None,
                canonical_query.as_deref(),
                headers,
                Some(JsonCodec::CONTENT_TYPE),
            )
            .await?;
        let value = decode_json_value_response(&JsonCodec, &response)?;
        projection.decode_many(value).map_err(ClientError::from)
    }

    pub async fn list_view_paged<P>(
        &self,
        path: &str,
        projection: &P,
        extra_query: &[QueryPair<'_>],
        headers: &[HeaderPair<'_>],
    ) -> Result<Page<P::Output>, ClientError>
    where
        P: Projection,
    {
        let selection = projection.selection_query();
        let canonical_query = canonical_query_from_selection(&selection, extra_query)?;
        let response = self
            .request_raw_with_query_and_accept(
                Method::GET,
                path,
                None,
                canonical_query.as_deref(),
                headers,
                Some(JsonCodec::CONTENT_TYPE),
            )
            .await?;
        let value = decode_json_value_response(&JsonCodec, &response)?;
        projection.decode_page(value).map_err(ClientError::from)
    }

    pub async fn post<Input, Output>(
        &self,
        path: &str,
        input: &Input,
        headers: &[HeaderPair<'_>],
    ) -> Result<Output, ClientError>
    where
        Input: Serialize,
        Output: DeserializeOwned,
    {
        let body = self.codec.encode(input)?;
        let response = self
            .request_raw(Method::POST, path, Some(body), &[], headers)
            .await?;
        decode_typed_response(&self.codec, &response)
    }

    pub async fn post_list<Input, Output>(
        &self,
        path: &str,
        input: &Input,
        headers: &[HeaderPair<'_>],
    ) -> Result<Vec<Output>, ClientError>
    where
        Input: Serialize,
        Output: DeserializeOwned,
    {
        let body = self.codec.encode(input)?;
        let response = self
            .request_raw_with_query_and_accept(
                Method::POST,
                path,
                Some(body),
                None,
                headers,
                Some(self.codec.sequence_accept_header_value()),
            )
            .await?;
        decode_sequence_response(&self.codec, &response)
    }

    /// Streaming variant of [`Self::post_list`]. Returns an
    /// `mpsc::Receiver` that yields decoded items as they arrive over
    /// the network — first-item latency drops from "buffer the whole
    /// body" to "decode one chunk." Useful on mobile / flaky links
    /// where time-to-first-byte matters more than total throughput.
    ///
    /// The receiver yields `Result<Output, ClientError>` per item.
    /// Transport / decode errors are terminal — the next call to
    /// `.recv()` returns `None` after one. A clean end-of-stream
    /// (server closed cleanly after the last item) also surfaces as
    /// `None` from the next `.recv()`.
    ///
    /// The server must return `application/cbor-seq`. If it returns a
    /// buffered `application/cbor` or `application/json` instead, the
    /// caller should use [`Self::post_list`] — this method does not
    /// fall back.
    pub async fn post_list_streamed<Input, Output>(
        &self,
        path: &str,
        input: &Input,
        headers: &[HeaderPair<'_>],
    ) -> Result<tokio::sync::mpsc::Receiver<Result<Output, ClientError>>, ClientError>
    where
        Input: Serialize,
        Output: DeserializeOwned + Send + 'static,
    {
        let body = self.codec.encode(input)?;
        let response = self
            .request_streamed_with_query_and_accept(
                Method::POST,
                path,
                Some(body),
                None,
                headers,
                self.codec.sequence_accept_header_value(),
            )
            .await?;

        // Bounded channel keeps memory tight on the consumer side —
        // 16 items in flight is plenty for a single subscriber.
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        tokio::spawn(pump_streamed_response_typed::<Output>(response, tx));
        Ok(rx)
    }

    pub async fn patch<Input, Output>(
        &self,
        path: &str,
        input: &Input,
        headers: &[HeaderPair<'_>],
    ) -> Result<Output, ClientError>
    where
        Input: Serialize,
        Output: DeserializeOwned,
    {
        let body = self.codec.encode(input)?;
        let response = self
            .request_raw(Method::PATCH, path, Some(body), &[], headers)
            .await?;
        decode_typed_response(&self.codec, &response)
    }

    pub async fn delete<Output>(
        &self,
        path: &str,
        headers: &[HeaderPair<'_>],
    ) -> Result<Output, ClientError>
    where
        Output: DeserializeOwned,
    {
        let response = self
            .request_raw(Method::DELETE, path, None, &[], headers)
            .await?;
        decode_typed_response(&self.codec, &response)
    }

    pub async fn execute_raw_transport(
        &self,
        request: RuntimeRequestWire,
    ) -> Result<RuntimeResponseWire, ClientError> {
        let method = Method::from_bytes(request.method.as_bytes()).map_err(|error| {
            ClientError::BadInput(format!("invalid HTTP method '{}': {error}", request.method))
        })?;
        let header_pairs = request
            .headers
            .iter()
            .map(|header| (header.name.as_str(), header.value.as_str()))
            .collect::<Vec<_>>();
        self.request_raw_with_query(
            method,
            &request.path,
            if request.body.is_empty() {
                None
            } else {
                Some(request.body)
            },
            request.canonical_query.as_deref(),
            &header_pairs,
        )
        .await
    }

    async fn request_raw(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        query: &[QueryPair<'_>],
        headers: &[HeaderPair<'_>],
    ) -> Result<RuntimeResponseWire, ClientError> {
        let canonical_query =
            if query.is_empty() {
                None
            } else {
                Some(serde_urlencoded::to_string(query).map_err(|error| {
                    ClientError::BadInput(format!("invalid query pairs: {error}"))
                })?)
            };
        self.request_raw_with_query(method, path, body, canonical_query.as_deref(), headers)
            .await
    }

    async fn request_raw_with_query_and_accept(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        canonical_query: Option<&str>,
        headers: &[HeaderPair<'_>],
        accept_override: Option<&'static str>,
    ) -> Result<RuntimeResponseWire, ClientError> {
        let url = build_url(&self.config.base_url, path, canonical_query)?;
        let mut header_map = HeaderMap::new();
        header_map.insert(
            ACCEPT,
            HeaderValue::from_static(
                accept_override.unwrap_or_else(|| self.codec.accept_header_value()),
            ),
        );
        let content_type = if body.is_some() {
            header_map.insert(CONTENT_TYPE, HeaderValue::from_static(C::CONTENT_TYPE));
            Some(C::CONTENT_TYPE.to_owned())
        } else {
            None
        };
        if let Some(authorizer) = &self.request_authorizer {
            let canonical_request = canonical_request_string(
                method.as_str(),
                path,
                canonical_query,
                content_type.as_deref(),
                body.as_deref().unwrap_or(&[]),
            );
            let authorization_request = AuthorizationRequest {
                method: method.as_str().to_owned(),
                path: path.to_owned(),
                canonical_query: canonical_query.map(str::to_owned),
                content_type: content_type.clone(),
                body: body.clone().unwrap_or_default(),
                canonical_request,
            };
            for (name, value) in authorizer.authorize(&authorization_request)? {
                header_map.insert(
                    HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                        ClientError::BadInput(format!("invalid header name '{name}': {error}"))
                    })?,
                    HeaderValue::from_str(&value).map_err(|error| {
                        ClientError::BadInput(format!("invalid header value for '{name}': {error}"))
                    })?,
                );
            }
        }
        for (name, value) in headers {
            header_map.insert(
                HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                    ClientError::BadInput(format!("invalid header name '{name}': {error}"))
                })?,
                HeaderValue::from_str(value).map_err(|error| {
                    ClientError::BadInput(format!("invalid header value for '{name}': {error}"))
                })?,
            );
        }

        let mut request = self.http.request(method.clone(), url).headers(header_map);
        if let Some(body) = body {
            request = request.body(body);
        }

        let response = request.send().await?;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.bytes().await?;
        let response_wire = RuntimeResponseWire {
            status_code: status.as_u16(),
            headers: headers_to_runtime(&headers),
            body: bytes.to_vec(),
        };

        self.record_request(method.as_str(), path, status, &headers)?;

        Ok(response_wire)
    }

    async fn request_raw_with_query(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        canonical_query: Option<&str>,
        headers: &[HeaderPair<'_>],
    ) -> Result<RuntimeResponseWire, ClientError> {
        self.request_raw_with_query_and_accept(method, path, body, canonical_query, headers, None)
            .await
    }

    /// Streaming counterpart to `request_raw_with_query_and_accept`.
    /// Same prep (URL, headers, auth, canonical request), but returns
    /// the raw `reqwest::Response` instead of buffering the body — so
    /// callers can drive `bytes_stream()` themselves.
    ///
    /// Rejects non-2xx responses with `ClientError::Remote` after
    /// buffering the body once, since error bodies are bounded by
    /// `CoolErrorResponse` and small. Only successful responses leave
    /// this method unbuffered.
    async fn request_streamed_with_query_and_accept(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        canonical_query: Option<&str>,
        headers: &[HeaderPair<'_>],
        accept: &'static str,
    ) -> Result<reqwest::Response, ClientError> {
        let url = build_url(&self.config.base_url, path, canonical_query)?;
        let mut header_map = HeaderMap::new();
        header_map.insert(ACCEPT, HeaderValue::from_static(accept));
        let content_type = if body.is_some() {
            header_map.insert(CONTENT_TYPE, HeaderValue::from_static(C::CONTENT_TYPE));
            Some(C::CONTENT_TYPE.to_owned())
        } else {
            None
        };
        if let Some(authorizer) = &self.request_authorizer {
            let canonical_request = canonical_request_string(
                method.as_str(),
                path,
                canonical_query,
                content_type.as_deref(),
                body.as_deref().unwrap_or(&[]),
            );
            let authorization_request = AuthorizationRequest {
                method: method.as_str().to_owned(),
                path: path.to_owned(),
                canonical_query: canonical_query.map(str::to_owned),
                content_type: content_type.clone(),
                body: body.clone().unwrap_or_default(),
                canonical_request,
            };
            for (name, value) in authorizer.authorize(&authorization_request)? {
                header_map.insert(
                    HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                        ClientError::BadInput(format!("invalid header name '{name}': {error}"))
                    })?,
                    HeaderValue::from_str(&value).map_err(|error| {
                        ClientError::BadInput(format!("invalid header value for '{name}': {error}"))
                    })?,
                );
            }
        }
        for (name, value) in headers {
            header_map.insert(
                HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                    ClientError::BadInput(format!("invalid header name '{name}': {error}"))
                })?,
                HeaderValue::from_str(value).map_err(|error| {
                    ClientError::BadInput(format!("invalid header value for '{name}': {error}"))
                })?,
            );
        }

        let mut request = self.http.request(method.clone(), url).headers(header_map);
        if let Some(body) = body {
            request = request.body(body);
        }

        let response = request.send().await?;
        let status = response.status();
        let headers_snapshot = response.headers().clone();
        self.record_request(method.as_str(), path, status, &headers_snapshot)?;

        if !status.is_success() {
            // Bounded error path — buffer the body (small by contract)
            // and produce a Remote error, matching the buffered code
            // path's behavior.
            let bytes = response.bytes().await?;
            let response_wire = RuntimeResponseWire {
                status_code: status.as_u16(),
                headers: headers_to_runtime(&headers_snapshot),
                body: bytes.to_vec(),
            };
            let error = remote_error_from_response(&self.codec, &response_wire);
            return Err(error);
        }

        Ok(response)
    }

    fn record_request(
        &self,
        method: &str,
        path: &str,
        status: StatusCode,
        headers: &HeaderMap,
    ) -> Result<(), ClientError> {
        self.state_store
            .append_request_journal(&RequestJournalEntry {
                method: method.to_owned(),
                path: path.to_owned(),
                status_code: status.as_u16(),
                content_type: headers
                    .get(CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(ToOwned::to_owned),
                recorded_at: Utc::now(),
            })
    }
}

fn decode_typed_response<C, Output>(
    codec: &C,
    response: &RuntimeResponseWire,
) -> Result<Output, ClientError>
where
    C: HttpClientCodec,
    Output: DeserializeOwned,
{
    let content_type = response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str())
        .ok_or_else(|| {
            ClientError::InvalidResponse("response is missing Content-Type header".to_owned())
        })?;

    if (200..=299).contains(&response.status_code) {
        codec
            .decode_response::<Output>(content_type, &response.body)
            .map_err(ClientError::from)
    } else {
        let error = codec
            .decode_response::<CoolErrorResponse>(content_type, &response.body)
            .ok();
        let message = error
            .as_ref()
            .map(|value| value.message.clone())
            .unwrap_or_else(|| {
                format!("unexpected error body for status {}", response.status_code)
            });
        Err(ClientError::Remote {
            status: StatusCode::from_u16(response.status_code)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            error,
            message,
        })
    }
}

fn decode_json_value_response<C>(
    codec: &C,
    response: &RuntimeResponseWire,
) -> Result<JsonValue, ClientError>
where
    C: HttpClientCodec,
{
    decode_typed_response(codec, response)
}

/// Build a `ClientError::Remote` from a non-2xx response, decoding the
/// body as a `CoolErrorResponse` if possible. Used by the streaming
/// path which has a separate buffer-on-error step (success path
/// streams, error path is bounded and fits in memory).
fn remote_error_from_response<C>(codec: &C, response: &RuntimeResponseWire) -> ClientError
where
    C: HttpClientCodec,
{
    let content_type = response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str())
        .unwrap_or("");
    let error = codec
        .decode_response::<CoolErrorResponse>(content_type, &response.body)
        .ok();
    let message = error.as_ref().map(|value| value.message.clone()).unwrap_or_else(|| {
        format!("unexpected error body for status {}", response.status_code)
    });
    ClientError::Remote {
        status: StatusCode::from_u16(response.status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        error,
        message,
    }
}

fn decode_sequence_response<C, Output>(
    codec: &C,
    response: &RuntimeResponseWire,
) -> Result<Vec<Output>, ClientError>
where
    C: HttpClientCodec,
    Output: DeserializeOwned,
{
    let content_type = response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str())
        .ok_or_else(|| {
            ClientError::InvalidResponse("response is missing Content-Type header".to_owned())
        })?;

    if (200..=299).contains(&response.status_code) {
        codec
            .decode_sequence_response::<Output>(content_type, &response.body)
            .map_err(ClientError::from)
    } else {
        let error = if media_type_matches(content_type, CBOR_SEQUENCE_CONTENT_TYPE) {
            decode_cbor_sequence::<CoolErrorResponse>(&response.body)
                .ok()
                .and_then(|mut values| {
                    if values.len() == 1 {
                        values.pop()
                    } else {
                        None
                    }
                })
        } else {
            codec
                .decode_response::<CoolErrorResponse>(content_type, &response.body)
                .ok()
        };
        let message = error
            .as_ref()
            .map(|value| value.message.clone())
            .unwrap_or_else(|| {
                format!("unexpected error body for status {}", response.status_code)
            });
        Err(ClientError::Remote {
            status: StatusCode::from_u16(response.status_code)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            error,
            message,
        })
    }
}

fn canonical_query_from_selection(
    selection: &SelectionQuery,
    extra_query: &[QueryPair<'_>],
) -> Result<Option<String>, ClientError> {
    let mut query: Vec<(String, String)> = Vec::new();
    if !selection.fields.is_empty() {
        query.push(("fields".to_owned(), selection.fields.join(",")));
    }
    if !selection.includes.is_empty() {
        query.push(("include".to_owned(), selection.includes.join(",")));
    }
    for (include, fields) in &selection.include_fields {
        if !fields.is_empty() {
            query.push((format!("includeFields[{include}]"), fields.join(",")));
        }
    }
    for (key, value) in extra_query {
        if *key == "fields" || *key == "include" || key.starts_with("includeFields[") {
            return Err(ClientError::BadInput(format!(
                "projection query parameter '{key}' must come from SelectionQuery, not extra_query"
            )));
        }
        query.push(((*key).to_owned(), (*value).to_owned()));
    }
    if query.is_empty() {
        return Ok(None);
    }
    serde_urlencoded::to_string(&query)
        .map(Some)
        .map_err(|error| ClientError::BadInput(format!("invalid selection query: {error}")))
}

fn headers_to_runtime(headers: &HeaderMap) -> Vec<RuntimeHeader> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value.to_str().ok().map(|value| RuntimeHeader {
                name: name.as_str().to_owned(),
                value: value.to_owned(),
            })
        })
        .collect()
}

fn build_url(
    base_url: &Url,
    path: &str,
    canonical_query: Option<&str>,
) -> Result<Url, ClientError> {
    let mut base = base_url.clone();
    if !base.path().ends_with('/') {
        let next_path = format!("{}/", base.path());
        base.set_path(&next_path);
    }
    let mut url = base.join(path.trim_start_matches('/')).map_err(|error| {
        ClientError::InvalidResponse(format!(
            "failed to resolve path '{path}' against {}: {error}",
            base
        ))
    })?;
    match canonical_query {
        Some(query) if !query.is_empty() => url.set_query(Some(query)),
        _ => url.set_query(None),
    }
    Ok(url)
}

fn media_type_matches(candidate: &str, expected: &str) -> bool {
    candidate.split(';').next().unwrap_or(candidate).trim() == expected
}

fn decode_cbor_sequence<T>(bytes: &[u8]) -> Result<Vec<T>, CoolError>
where
    T: DeserializeOwned,
{
    let mut values = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let mut deserializer = minicbor_serde::Deserializer::new(&bytes[offset..]);
        values.push(T::deserialize(&mut deserializer).map_err(|error| {
            CoolError::Codec(format!("failed to decode CBOR sequence body: {error}"))
        })?);
        let consumed = deserializer.decoder().position();
        if consumed == 0 {
            return Err(CoolError::Codec(
                "failed to decode CBOR sequence body: decoder made no progress".to_owned(),
            ));
        }
        offset += consumed;
    }
    Ok(values)
}

// -----------------------------------------------------------------------------
// Chunked cbor-seq decoder + streaming response consumers
//
// The buffered path above (`decode_cbor_sequence`) needs the full
// response body before yielding the first item. On a flaky / metered
// network — typical for mobile clients — that costs time-to-first-byte
// AND memory: a 5 MB streamed list buffers all 5 MB before any item
// reaches the UI.
//
// The pieces below give callers two streaming consumer shapes:
//
// - **`CratestackClient::post_list_streamed`** — typed Rust callers.
//   Returns a `tokio::sync::mpsc::Receiver<Result<T, ClientError>>`
//   that yields items as bytes arrive over the network.
//
// - **`RuntimeHandle::execute_streamed`** — FFI / Flutter shape.
//   Synchronous from the caller's perspective: pass a callback, return
//   when the stream is done. The callback receives raw item bytes
//   (one CBOR-encoded value per call) so the FFI side can decode using
//   whatever native CBOR library it prefers.
// -----------------------------------------------------------------------------

/// Stateful boundary scanner for `application/cbor-seq` streams. Bytes
/// arrive in arbitrary chunks; this type buffers them and emits the
/// byte ranges of any complete top-level CBOR items observed so far.
/// The CBOR-level parse uses `minicbor::Decoder::skip` for boundary
/// detection (cheap, doesn't allocate); the per-item serde decode
/// happens at the caller's leisure on each returned slice.
struct CborSeqChunkDecoder {
    buffer: Vec<u8>,
}

impl CborSeqChunkDecoder {
    fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Append `chunk` to the internal buffer and return the bytes of
    /// every complete top-level CBOR item now in it. Drains those bytes
    /// from the buffer; any trailing bytes that don't yet form a
    /// complete item stay buffered for the next call.
    fn feed_chunk(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>, CoolError> {
        self.buffer.extend_from_slice(chunk);
        let mut items: Vec<Vec<u8>> = Vec::new();
        let mut consumed = 0;
        loop {
            let remaining = &self.buffer[consumed..];
            if remaining.is_empty() {
                break;
            }
            let mut decoder = minicbor::decode::Decoder::new(remaining);
            match decoder.skip() {
                Ok(()) => {
                    let item_len = decoder.position();
                    if item_len == 0 {
                        return Err(CoolError::Codec(
                            "cbor-seq decoder made no progress".to_owned(),
                        ));
                    }
                    items.push(remaining[..item_len].to_vec());
                    consumed += item_len;
                }
                Err(error) if error.is_end_of_input() => {
                    // Truncated final item — wait for the next chunk.
                    break;
                }
                Err(error) => {
                    return Err(CoolError::Codec(format!(
                        "cbor-seq decode failed: {error}",
                    )));
                }
            }
        }
        if consumed > 0 {
            self.buffer.drain(..consumed);
        }
        Ok(items)
    }

    fn pending_len(&self) -> usize {
        self.buffer.len()
    }
}

/// Pump a reqwest streaming response into an `mpsc::Sender`. Each
/// complete cbor-seq item gets deserialized to `T` and sent through;
/// transport / decode errors become terminal `Err` items.
async fn pump_streamed_response_typed<T>(
    response: reqwest::Response,
    tx: tokio::sync::mpsc::Sender<Result<T, ClientError>>,
) where
    T: DeserializeOwned + Send + 'static,
{
    use futures_util::StreamExt;

    let mut byte_stream = response.bytes_stream();
    let mut decoder = CborSeqChunkDecoder::new();
    while let Some(chunk_result) = byte_stream.next().await {
        let chunk = match chunk_result {
            Ok(c) => c,
            Err(error) => {
                let _ = tx.send(Err(ClientError::Transport(error))).await;
                return;
            }
        };
        let items = match decoder.feed_chunk(&chunk) {
            Ok(items) => items,
            Err(error) => {
                let _ = tx.send(Err(ClientError::Codec(error))).await;
                return;
            }
        };
        for item_bytes in items {
            let decoded: Result<T, ClientError> = minicbor_serde::from_slice(&item_bytes)
                .map_err(|error| {
                    ClientError::Codec(CoolError::Codec(format!(
                        "decode cbor-seq item: {error}",
                    )))
                });
            if tx.send(decoded).await.is_err() {
                // Receiver dropped — caller cancelled, stop work.
                return;
            }
        }
    }

    if decoder.pending_len() > 0 {
        let _ = tx
            .send(Err(ClientError::InvalidResponse(format!(
                "stream ended with {} bytes buffered (incomplete final item)",
                decoder.pending_len(),
            ))))
            .await;
    }
}

/// FFI-shaped chunk delivered to the `execute_streamed` callback.
/// `Item` carries one CBOR-encoded item's raw bytes; `Error` is
/// terminal; `End` is terminal and indicates a clean stream close.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeChunkWire {
    /// One complete cbor-seq item. The bytes are CBOR-encoded — decode
    /// on the FFI side with whatever native library the host has.
    Item(Vec<u8>),
    /// Terminal: the stream ended cleanly. No further chunks follow.
    End,
    /// Terminal: the stream failed mid-flight. No further chunks follow.
    Error(RuntimeErrorWire),
}

/// Drive a streaming response through a callback. Used by
/// `RuntimeHandle::execute_streamed` — the callback returns `false` to
/// cancel the stream early. The function returns once the stream is
/// done (by completion, error, or cancellation).
async fn pump_streamed_response_callback<F>(
    response: reqwest::Response,
    mut on_chunk: F,
) -> Result<(), RuntimeErrorWire>
where
    F: FnMut(RuntimeChunkWire) -> bool,
{
    use futures_util::StreamExt;

    let mut byte_stream = response.bytes_stream();
    let mut decoder = CborSeqChunkDecoder::new();
    loop {
        let chunk_result = byte_stream.next().await;
        let chunk = match chunk_result {
            Some(Ok(c)) => c,
            Some(Err(error)) => {
                let err = RuntimeErrorWire::from(ClientError::Transport(error));
                on_chunk(RuntimeChunkWire::Error(err.clone()));
                return Err(err);
            }
            None => {
                if decoder.pending_len() > 0 {
                    let err = RuntimeErrorWire {
                        code: RuntimeErrorCode::InvalidResponse,
                        http_status: None,
                        message: format!(
                            "stream ended with {} bytes buffered (incomplete final item)",
                            decoder.pending_len(),
                        ),
                        remote_code: None,
                        remote_body: None,
                    };
                    on_chunk(RuntimeChunkWire::Error(err.clone()));
                    return Err(err);
                }
                on_chunk(RuntimeChunkWire::End);
                return Ok(());
            }
        };
        let items = match decoder.feed_chunk(&chunk) {
            Ok(items) => items,
            Err(error) => {
                let err = RuntimeErrorWire::from(ClientError::Codec(error));
                on_chunk(RuntimeChunkWire::Error(err.clone()));
                return Err(err);
            }
        };
        for item_bytes in items {
            if !on_chunk(RuntimeChunkWire::Item(item_bytes)) {
                // Caller cancelled.
                return Ok(());
            }
        }
    }
}

// -----------------------------------------------------------------------------
// RPC client surface
//
// Sits alongside `CratestackClient` (the REST client). Schemas declared
// with `transport rpc` generate against the methods on `RpcClient`:
//
//   * `call::<I, O>(op_id, input)`     — POST /rpc/{op_id}, unary
//   * `batch(requests)`                — POST /rpc/batch
//   * `call_streaming::<I, O>(op_id, input)` — POST /rpc/{op_id}, sequence
//
// Wire types and the `RPC_*_PATH` constants come from `cratestack_core::rpc`
// so server (`cratestack-axum::rpc`) and client share one source of truth.
// -----------------------------------------------------------------------------

pub use cratestack_core::rpc::{
    RPC_BATCH_PATH, RPC_UNARY_PATH, RpcErrorBody, RpcRequest, RpcResponseFrame, rpc_code,
};

/// Error variant produced by the RPC client when a remote call fails with
/// an `RpcErrorBody` payload. Distinct from the REST `ClientError::Remote`
/// (which carries the `CoolErrorResponse` shape) so library users can
/// switch on the gRPC-style `code` string directly.
#[derive(Debug, Clone)]
pub struct RpcRemoteError {
    pub status: StatusCode,
    pub body: RpcErrorBody,
}

impl std::fmt::Display for RpcRemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RPC call failed with code {} (status {}): {}",
            self.body.code,
            self.status.as_u16(),
            self.body.message
        )
    }
}

impl std::error::Error for RpcRemoteError {}

/// Top-level error returned by the RPC client. Mirrors `ClientError`
/// (the REST error type) but reports server-side failures as
/// `RpcRemoteError { code, message, details }` rather than the
/// REST-shaped `CoolErrorResponse`.
#[derive(Debug, thiserror::Error)]
pub enum RpcClientError {
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("codec error: {0}")]
    Codec(#[from] CoolError),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("bad input: {0}")]
    BadInput(String),
    #[error("{0}")]
    Remote(RpcRemoteError),
}

/// Thin RPC client built on top of the REST client's transport + codec
/// plumbing.
///
/// Shares a `reqwest::Client` and a codec impl with `CratestackClient`,
/// but speaks the `/rpc/...` URL space instead of REST routes. Both
/// clients can be used side-by-side against the same server.
#[derive(Clone)]
pub struct RpcClient<C = CborCodec> {
    inner: CratestackClient<C>,
}

impl RpcClient<CborCodec> {
    pub fn cbor(config: ClientConfig) -> Self {
        Self::new(CratestackClient::cbor(config))
    }
}

impl<C> RpcClient<C>
where
    C: HttpClientCodec + Clone,
{
    /// Build an RPC client on top of an existing REST client. The two
    /// share their `reqwest::Client`, codec, and state store.
    pub fn new(inner: CratestackClient<C>) -> Self {
        Self { inner }
    }

    /// Underlying REST client. Exposed for callers that want REST + RPC
    /// side-by-side (e.g. a long migration window between the two).
    pub fn inner(&self) -> &CratestackClient<C> {
        &self.inner
    }

    /// POST /rpc/{op_id} — unary call.
    ///
    /// `op_id` is the dotted dispatch key the server emits — `model.X.list`
    /// / `model.X.get` / `model.X.create` / `model.X.update` /
    /// `model.X.delete` for CRUD verbs and `procedure.<name>` for procedures.
    pub async fn call<I, O>(&self, op_id: &str, input: &I) -> Result<O, RpcClientError>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        let body = self.inner.codec.encode(input).map_err(RpcClientError::Codec)?;
        let path = format!("/rpc/{}", op_id);
        let response = self
            .inner
            .request_raw_with_query_and_accept(
                Method::POST,
                &path,
                Some(body),
                None,
                &[],
                None,
            )
            .await
            .map_err(client_error_to_rpc)?;
        decode_rpc_unary_response(&self.inner.codec, &response)
    }

    /// POST /rpc/batch — sequence of `RpcRequest` frames in, sequence of
    /// `RpcResponseFrame` frames out. Per-frame errors do not poison the
    /// batch (each frame's `output` / `error` is reported independently).
    pub async fn batch(
        &self,
        requests: &[RpcRequest],
    ) -> Result<Vec<RpcResponseFrame>, RpcClientError> {
        let body = self.inner.codec.encode(&requests).map_err(RpcClientError::Codec)?;
        let response = self
            .inner
            .request_raw_with_query_and_accept(
                Method::POST,
                RPC_BATCH_PATH_PLAIN,
                Some(body),
                None,
                &[],
                None,
            )
            .await
            .map_err(client_error_to_rpc)?;
        decode_rpc_unary_response::<C, Vec<RpcResponseFrame>>(&self.inner.codec, &response)
    }

    /// POST /rpc/{op_id} — sequence response. Returns the full sequence as
    /// a `Vec<O>` (the binding currently buffers; a `Stream<Item=O>`
    /// adapter can be layered on top once the server emits streamed
    /// responses framed in `application/cbor-seq`).
    pub async fn call_streaming<I, O>(
        &self,
        op_id: &str,
        input: &I,
    ) -> Result<Vec<O>, RpcClientError>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        let body = self.inner.codec.encode(input).map_err(RpcClientError::Codec)?;
        let path = format!("/rpc/{}", op_id);
        let response = self
            .inner
            .request_raw_with_query_and_accept(
                Method::POST,
                &path,
                Some(body),
                None,
                &[],
                Some(self.inner.codec.sequence_accept_header_value()),
            )
            .await
            .map_err(client_error_to_rpc)?;
        decode_rpc_sequence_response(&self.inner.codec, &response)
    }
}

// `RPC_BATCH_PATH` from core is the axum-template form `"/rpc/batch"`,
// so we just reuse it. The unary path is templated (`/rpc/{op_id}`) so
// we format it per call instead of using the constant directly.
const RPC_BATCH_PATH_PLAIN: &str = RPC_BATCH_PATH;

fn client_error_to_rpc(error: ClientError) -> RpcClientError {
    match error {
        ClientError::Transport(error) => RpcClientError::Transport(error),
        ClientError::Codec(error) => RpcClientError::Codec(error),
        ClientError::InvalidResponse(message) => RpcClientError::InvalidResponse(message),
        ClientError::BadInput(message) => RpcClientError::BadInput(message),
        ClientError::State(message) => RpcClientError::InvalidResponse(message),
        ClientError::Remote {
            status,
            error,
            message,
        } => {
            // Legacy translation path — should not fire for /rpc/... URLs
            // (the server-side dispatcher emits RpcErrorBody-shaped error
            // bodies), but keep a sensible fallback rather than dropping
            // the message on the floor.
            let body = error
                .map(cratestack_core::rpc::RpcErrorBody::from_cool_response)
                .unwrap_or_else(|| RpcErrorBody {
                    code: "internal".to_owned(),
                    message,
                    details: None,
                });
            RpcClientError::Remote(RpcRemoteError { status, body })
        }
    }
}

fn decode_rpc_unary_response<C, Output>(
    codec: &C,
    response: &RuntimeResponseWire,
) -> Result<Output, RpcClientError>
where
    C: HttpClientCodec,
    Output: DeserializeOwned,
{
    let content_type = response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str())
        .ok_or_else(|| {
            RpcClientError::InvalidResponse("response is missing Content-Type header".to_owned())
        })?;

    if (200..=299).contains(&response.status_code) {
        codec
            .decode_response::<Output>(content_type, &response.body)
            .map_err(RpcClientError::Codec)
    } else {
        let body = codec
            .decode_response::<RpcErrorBody>(content_type, &response.body)
            .unwrap_or_else(|_| RpcErrorBody {
                code: "internal".to_owned(),
                message: format!(
                    "unexpected RPC error body for status {}",
                    response.status_code
                ),
                details: None,
            });
        Err(RpcClientError::Remote(RpcRemoteError {
            status: StatusCode::from_u16(response.status_code)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            body,
        }))
    }
}

fn decode_rpc_sequence_response<C, Output>(
    codec: &C,
    response: &RuntimeResponseWire,
) -> Result<Vec<Output>, RpcClientError>
where
    C: HttpClientCodec,
    Output: DeserializeOwned,
{
    let content_type = response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str())
        .ok_or_else(|| {
            RpcClientError::InvalidResponse("response is missing Content-Type header".to_owned())
        })?;

    if (200..=299).contains(&response.status_code) {
        codec
            .decode_sequence_response::<Output>(content_type, &response.body)
            .map_err(RpcClientError::Codec)
    } else {
        let body = codec
            .decode_response::<RpcErrorBody>(content_type, &response.body)
            .unwrap_or_else(|_| RpcErrorBody {
                code: "internal".to_owned(),
                message: format!(
                    "unexpected RPC error body for status {}",
                    response.status_code
                ),
                details: None,
            });
        Err(RpcClientError::Remote(RpcRemoteError {
            status: StatusCode::from_u16(response.status_code)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            body,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        ClientStateStore, JsonFileStateStore, PersistedClientState, RequestJournalEntry,
        RuntimeCodecConfig, RuntimeConfigWire, RuntimeEnvelopeConfig, RuntimeErrorCode,
        RuntimeHandle, RuntimeRequestWire, RuntimeStateStoreConfig, RuntimeTransportConfig,
    };

    #[test]
    fn json_file_store_round_trips_state_under_project_tmp() {
        let path = project_tmp_path("state-store-unit.json");
        if path.exists() {
            std::fs::remove_file(&path).expect("existing tmp file should be removable");
        }

        let store = JsonFileStateStore::new(&path);
        store
            .append_request_journal(&RequestJournalEntry {
                method: "GET".to_owned(),
                path: "/posts".to_owned(),
                status_code: 200,
                content_type: Some("application/cbor".to_owned()),
                recorded_at: chrono::Utc::now(),
            })
            .expect("journal entry should append");

        let loaded = store.load().expect("state should load");
        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.state_version, 1);
        assert_eq!(loaded.request_journal.len(), 1);

        std::fs::remove_file(&path).expect("tmp file should be removable");
    }

    #[test]
    fn runtime_handle_rejects_invalid_method_without_running_http() {
        let handle = RuntimeHandle::new(RuntimeConfigWire {
            base_url: "http://127.0.0.1:1/".to_owned(),
            state_store: RuntimeStateStoreConfig::InMemory,
            transport: RuntimeTransportConfig::default(),
        })
        .expect("runtime handle should build");

        let error = handle
            .execute(RuntimeRequestWire {
                method: "BAD METHOD".to_owned(),
                path: "/posts".to_owned(),
                canonical_query: None,
                headers: Vec::new(),
                body: Vec::new(),
            })
            .expect_err("invalid method should fail before transport");

        assert_eq!(error.code as u32, super::RuntimeErrorCode::BadInput as u32);
    }

    #[test]
    fn persisted_state_defaults_missing_state_version() {
        let state: PersistedClientState =
            serde_json::from_str(r#"{"schema_version":1,"request_journal":[]}"#)
                .expect("legacy state should decode");

        assert_eq!(state.state_version, 0);
    }

    #[test]
    fn runtime_handle_rejects_unsupported_envelope_config() {
        let result = RuntimeHandle::new(RuntimeConfigWire {
            base_url: "http://127.0.0.1:1/".to_owned(),
            state_store: RuntimeStateStoreConfig::InMemory,
            transport: RuntimeTransportConfig {
                codec: RuntimeCodecConfig::Cbor,
                envelope: RuntimeEnvelopeConfig::CoseSign1,
            },
        });

        let error = match result {
            Ok(_) => panic!("unsupported envelope should fail"),
            Err(error) => error,
        };

        assert_eq!(error.code, RuntimeErrorCode::BadInput);
    }

    fn project_tmp_path(file_name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp/client-rust-tests")
            .join(file_name)
    }
}
