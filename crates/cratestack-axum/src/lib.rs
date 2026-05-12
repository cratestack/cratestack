pub use axum;

pub mod idempotency;
pub mod ratelimit;

use sha2::{Digest, Sha256};

/// Derive a stable, namespaced fingerprint of the caller from the
/// `Authorization` header. Both the idempotency and rate-limit middlewares
/// scope per-principal state and used to maintain byte-identical
/// SHA-256-of-Authorization functions that differed only in their string
/// prefix. This single helper takes the prefix as a parameter so each
/// caller's keyspace stays distinct.
///
/// Layout: `"<prefix>:<sha256_hex(authorization)>"` when the header is
/// present (or just the hex digest if `prefix` is empty), and the bare
/// string `"anonymous"` when it's absent. The anonymous fallback is
/// intentionally NOT prefixed — both pre-extraction implementations
/// returned plain `"anonymous"` for the no-auth case, and that is the
/// keyspace contract callers rely on.
pub fn principal_fingerprint(req: &axum::extract::Request, prefix: &str) -> String {
    let Some(value) = req
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return "anonymous".to_owned();
    };
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    if prefix.is_empty() {
        format!("{:x}", hasher.finalize())
    } else {
        format!("{prefix}:{:x}", hasher.finalize())
    }
}

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use cratestack_core::{CoolCodec, CoolError, CoolErrorResponse, RouteTransportCapabilities};
use serde::{Deserialize, Serialize};
use url::form_urlencoded;

pub const CBOR_SEQUENCE_CONTENT_TYPE: &str = "application/cbor-seq";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryExpr {
    Predicate { key: String, value: String },
    All(Vec<QueryExpr>),
    Any(Vec<QueryExpr>),
    Not(Box<QueryExpr>),
}

pub trait HttpTransport: Clone + Send + Sync + 'static {
    fn decode_request<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: for<'de> Deserialize<'de>;

    fn encode_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &T,
    ) -> Result<Response, CoolError>
    where
        T: Serialize + ?Sized;

    fn encode_sequence_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        values: &[T],
    ) -> Result<Response, CoolError>
    where
        T: Serialize;

    fn encode_sequence_error_response(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &CoolErrorResponse,
    ) -> Result<Response, CoolError>;
}

impl<C> HttpTransport for C
where
    C: CoolCodec,
{
    fn decode_request<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: for<'de> Deserialize<'de>,
    {
        if media_type_matches(content_type, C::CONTENT_TYPE) {
            decode_codec_request(self, body)
        } else {
            Err(CoolError::UnsupportedMediaType(format!(
                "unsupported request Content-Type {content_type}"
            )))
        }
    }

    fn encode_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &T,
    ) -> Result<Response, CoolError>
    where
        T: Serialize + ?Sized,
    {
        if media_type_matches(content_type, C::CONTENT_TYPE) {
            encode_codec_response(self, status, value)
        } else {
            Err(CoolError::NotAcceptable(format!(
                "no encoder configured for response Content-Type {content_type}"
            )))
        }
    }

    fn encode_sequence_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        values: &[T],
    ) -> Result<Response, CoolError>
    where
        T: Serialize,
    {
        if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
            encode_cbor_sequence_response(self, status, values)
        } else {
            self.encode_response(content_type, status, values)
        }
    }

    fn encode_sequence_error_response(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &CoolErrorResponse,
    ) -> Result<Response, CoolError> {
        if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
            encode_cbor_sequence_response(self, status, std::slice::from_ref(value))
        } else {
            self.encode_response(content_type, status, value)
        }
    }
}

#[derive(Debug, Clone)]
pub struct CodecSet<Primary, Secondary> {
    primary: Primary,
    secondary: Secondary,
}

impl<Primary, Secondary> CodecSet<Primary, Secondary> {
    pub fn new(primary: Primary, secondary: Secondary) -> Self {
        Self { primary, secondary }
    }
}

impl<Primary, Secondary> HttpTransport for CodecSet<Primary, Secondary>
where
    Primary: CoolCodec,
    Secondary: CoolCodec,
{
    fn decode_request<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: for<'de> Deserialize<'de>,
    {
        if content_type == Primary::CONTENT_TYPE {
            self.primary.decode(body)
        } else if content_type == Secondary::CONTENT_TYPE {
            self.secondary.decode(body)
        } else {
            Err(CoolError::UnsupportedMediaType(format!(
                "unsupported request Content-Type {content_type}"
            )))
        }
    }

    fn encode_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &T,
    ) -> Result<Response, CoolError>
    where
        T: Serialize + ?Sized,
    {
        if content_type == Primary::CONTENT_TYPE {
            encode_codec_response(&self.primary, status, value)
        } else if content_type == Secondary::CONTENT_TYPE {
            encode_codec_response(&self.secondary, status, value)
        } else {
            Err(CoolError::NotAcceptable(format!(
                "no encoder configured for response Content-Type {content_type}"
            )))
        }
    }

    fn encode_sequence_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        values: &[T],
    ) -> Result<Response, CoolError>
    where
        T: Serialize,
    {
        if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
            if Primary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.primary, status, values)
            } else if Secondary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.secondary, status, values)
            } else {
                Err(CoolError::NotAcceptable(
                    "router does not have a CBOR codec for cbor-seq responses".to_owned(),
                ))
            }
        } else if content_type == Primary::CONTENT_TYPE {
            self.encode_response(content_type, status, values)
        } else if content_type == Secondary::CONTENT_TYPE {
            self.encode_response(content_type, status, values)
        } else {
            Err(CoolError::NotAcceptable(format!(
                "no encoder configured for response Content-Type {content_type}"
            )))
        }
    }

    fn encode_sequence_error_response(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &CoolErrorResponse,
    ) -> Result<Response, CoolError> {
        if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
            if Primary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.primary, status, std::slice::from_ref(value))
            } else if Secondary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.secondary, status, std::slice::from_ref(value))
            } else {
                Err(CoolError::NotAcceptable(
                    "router does not have a CBOR codec for cbor-seq responses".to_owned(),
                ))
            }
        } else if content_type == Primary::CONTENT_TYPE {
            self.encode_response(content_type, status, value)
        } else if content_type == Secondary::CONTENT_TYPE {
            self.encode_response(content_type, status, value)
        } else {
            Err(CoolError::NotAcceptable(format!(
                "no encoder configured for response Content-Type {content_type}"
            )))
        }
    }
}

struct CborCodecMarker;

impl CborCodecMarker {
    const CONTENT_TYPE: &'static str = "application/cbor";
}

pub fn validate_codec_response_headers<C>(headers: &HeaderMap) -> Result<(), CoolError>
where
    C: CoolCodec,
{
    validate_accept_header::<C>(headers)
}

pub fn validate_transport_request_headers_for<T>(
    _transport: &T,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
) -> Result<(), CoolError>
where
    T: HttpTransport,
{
    validate_transport_accept_header(headers, capabilities.response_types)?;
    if capabilities.request_types.is_empty() {
        Ok(())
    } else {
        validate_transport_content_type_header(headers, capabilities.request_types)
    }
}

pub fn validate_transport_response_headers_for<T>(
    _transport: &T,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
) -> Result<(), CoolError>
where
    T: HttpTransport,
{
    validate_transport_accept_header(headers, capabilities.response_types)
}

pub fn decode_transport_request_for<TTransport, TValue>(
    transport: &TTransport,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
    body: &[u8],
) -> Result<TValue, CoolError>
where
    TTransport: HttpTransport,
    TValue: for<'de> Deserialize<'de>,
{
    let content_type = request_content_type(headers, capabilities.request_types)?;
    transport.decode_request(content_type, body)
}

pub fn parse_query_pairs(raw_query: Option<&str>) -> Result<Vec<(String, String)>, CoolError> {
    let Some(raw_query) = raw_query else {
        return Ok(Vec::new());
    };

    let mut pairs = Vec::new();
    for (key, value) in form_urlencoded::parse(raw_query.as_bytes()) {
        pairs.push((key.into_owned(), value.into_owned()));
    }
    Ok(pairs)
}

pub fn parse_filter_expression(input: &str) -> Result<QueryExpr, CoolError> {
    let mut parser = FilterExpressionParser::new(input);
    let expr = parser.parse_expr()?;
    parser.skip_whitespace();
    if !parser.is_eof() {
        return Err(CoolError::BadRequest(format!(
            "unexpected trailing filter expression content near '{}'",
            parser.remaining(),
        )));
    }
    Ok(expr)
}

struct FilterExpressionParser<'a> {
    input: &'a str,
    cursor: usize,
}

impl<'a> FilterExpressionParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, cursor: 0 }
    }

    fn parse_expr(&mut self) -> Result<QueryExpr, CoolError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<QueryExpr, CoolError> {
        let mut nodes = vec![self.parse_and()?];
        loop {
            self.skip_whitespace();
            if !self.consume('|') {
                break;
            }
            nodes.push(self.parse_and()?);
        }
        Ok(if nodes.len() == 1 {
            nodes.pop().expect("single node should exist")
        } else {
            QueryExpr::Any(nodes)
        })
    }

    fn parse_and(&mut self) -> Result<QueryExpr, CoolError> {
        let mut nodes = vec![self.parse_factor()?];
        loop {
            self.skip_whitespace();
            if !self.consume(',') {
                break;
            }
            nodes.push(self.parse_factor()?);
        }
        Ok(if nodes.len() == 1 {
            nodes.pop().expect("single node should exist")
        } else {
            QueryExpr::All(nodes)
        })
    }

    fn parse_factor(&mut self) -> Result<QueryExpr, CoolError> {
        self.skip_whitespace();
        if self.consume_keyword("not") {
            self.skip_whitespace();
            if !self.consume('(') {
                return Err(CoolError::BadRequest(
                    "negated filter expression must use not(...)".to_owned(),
                ));
            }
            let expr = self.parse_expr()?;
            self.skip_whitespace();
            if !self.consume(')') {
                return Err(CoolError::BadRequest(
                    "unterminated negated filter expression".to_owned(),
                ));
            }
            return Ok(QueryExpr::Not(Box::new(expr)));
        }
        if self.consume('(') {
            let expr = self.parse_expr()?;
            self.skip_whitespace();
            if !self.consume(')') {
                return Err(CoolError::BadRequest(
                    "unterminated grouped filter expression".to_owned(),
                ));
            }
            return Ok(expr);
        }

        self.parse_predicate()
    }

    fn parse_predicate(&mut self) -> Result<QueryExpr, CoolError> {
        let start = self.cursor;
        while let Some(ch) = self.peek() {
            if matches!(ch, ',' | '|' | ')') {
                break;
            }
            self.cursor += ch.len_utf8();
        }
        let raw = self.input[start..self.cursor].trim();
        let (key, value) = raw.split_once('=').ok_or_else(|| {
            CoolError::BadRequest(format!(
                "invalid grouped filter '{}': expected key=value",
                raw,
            ))
        })?;
        if key.trim().is_empty() || value.trim().is_empty() {
            return Err(CoolError::BadRequest(format!(
                "invalid grouped filter '{}': expected non-empty key and value",
                raw,
            )));
        }
        Ok(QueryExpr::Predicate {
            key: key.trim().to_owned(),
            value: value.trim().to_owned(),
        })
    }

    fn consume(&mut self, expected: char) -> bool {
        match self.peek() {
            Some(ch) if ch == expected => {
                self.cursor += ch.len_utf8();
                true
            }
            _ => false,
        }
    }

    fn consume_keyword(&mut self, expected: &str) -> bool {
        let remaining = &self.input[self.cursor..];
        if !remaining.starts_with(expected) {
            return false;
        }
        let boundary = remaining[expected.len()..].chars().next();
        if boundary.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            return false;
        }
        self.cursor += expected.len();
        true
    }

    fn peek(&self) -> Option<char> {
        self.input[self.cursor..].chars().next()
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if !ch.is_whitespace() {
                break;
            }
            self.cursor += ch.len_utf8();
        }
    }

    fn remaining(&self) -> &str {
        &self.input[self.cursor..]
    }

    fn is_eof(&self) -> bool {
        self.cursor >= self.input.len()
    }
}

pub fn validate_codec_request_headers<C>(headers: &HeaderMap) -> Result<(), CoolError>
where
    C: CoolCodec,
{
    validate_accept_header::<C>(headers)?;
    validate_content_type_header::<C>(headers)
}

fn validate_accept_header<C>(headers: &HeaderMap) -> Result<(), CoolError>
where
    C: CoolCodec,
{
    validate_transport_accept_header(headers, &[C::CONTENT_TYPE])
}

fn validate_content_type_header<C>(headers: &HeaderMap) -> Result<(), CoolError>
where
    C: CoolCodec,
{
    validate_transport_content_type_header(headers, &[C::CONTENT_TYPE])
}

fn validate_transport_accept_header(
    headers: &HeaderMap,
    supported: &[&'static str],
) -> Result<(), CoolError> {
    let Some(accept) = headers.get(header::ACCEPT) else {
        return Ok(());
    };
    let accept = accept
        .to_str()
        .map_err(|error| CoolError::BadRequest(format!("invalid Accept header: {error}")))?;

    if supported
        .iter()
        .any(|content_type| accepts_content_type(accept, content_type))
    {
        Ok(())
    } else {
        Err(CoolError::NotAcceptable(format!(
            "router only serves {} responses",
            supported.join(", "),
        )))
    }
}

fn validate_transport_content_type_header(
    headers: &HeaderMap,
    supported: &[&'static str],
) -> Result<(), CoolError> {
    request_content_type(headers, supported).map(|_| ())
}

fn request_content_type(
    headers: &HeaderMap,
    supported: &[&'static str],
) -> Result<&'static str, CoolError> {
    let Some(content_type) = headers.get(header::CONTENT_TYPE) else {
        return Err(CoolError::UnsupportedMediaType(format!(
            "expected Content-Type one of {}",
            supported.join(", "),
        )));
    };
    let content_type = content_type
        .to_str()
        .map_err(|error| CoolError::BadRequest(format!("invalid Content-Type header: {error}")))?;

    supported
        .iter()
        .copied()
        .find(|expected| media_type_matches(content_type, expected))
        .ok_or_else(|| {
            CoolError::UnsupportedMediaType(format!(
                "expected Content-Type one of {}, got {}",
                supported.join(", "),
                content_type,
            ))
        })
}

fn select_response_content_type(
    headers: &HeaderMap,
    supported: &[&'static str],
    default: &'static str,
) -> Result<&'static str, CoolError> {
    let Some(accept) = headers.get(header::ACCEPT) else {
        return Ok(default);
    };
    let accept = accept
        .to_str()
        .map_err(|error| CoolError::BadRequest(format!("invalid Accept header: {error}")))?;

    supported
        .iter()
        .copied()
        .find(|content_type| accepts_content_type(accept, content_type))
        .ok_or_else(|| {
            CoolError::NotAcceptable(format!(
                "router only serves {} responses",
                supported.join(", "),
            ))
        })
}

fn accepts_content_type(accept: &str, expected: &str) -> bool {
    accept.split(',').map(str::trim).any(|value| {
        if value == "*/*" {
            return true;
        }
        let media_type = strip_media_type_params(value);
        media_type == expected
            || media_type == wildcard_media_type(expected)
            || media_type == "application/*"
    })
}

fn media_type_matches(candidate: &str, expected: &str) -> bool {
    strip_media_type_params(candidate) == expected
}

fn strip_media_type_params(value: &str) -> &str {
    value.split(';').next().unwrap_or(value).trim()
}

fn wildcard_media_type(content_type: &str) -> &str {
    content_type
        .split_once('/')
        .map(|(prefix, _)| {
            if prefix == "application" {
                "application/*"
            } else {
                "*/*"
            }
        })
        .unwrap_or("*/*")
}

pub fn decode_codec_request<C, T>(codec: &C, body: &[u8]) -> Result<T, CoolError>
where
    C: CoolCodec,
    T: for<'de> Deserialize<'de>,
{
    codec.decode(body)
}

pub fn encode_codec_response<C, T>(
    codec: &C,
    status: StatusCode,
    value: &T,
) -> Result<Response, CoolError>
where
    C: CoolCodec,
    T: Serialize + ?Sized,
{
    let bytes = codec.encode(value)?;
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(C::CONTENT_TYPE),
    );
    Ok(response)
}

pub fn encode_codec_result<C, T>(codec: &C, result: Result<T, CoolError>) -> Response
where
    C: CoolCodec,
    T: Serialize,
{
    encode_codec_result_with_status(codec, StatusCode::OK, result)
}

pub fn encode_transport_result_with_status_for<TTransport, TValue>(
    transport: &TTransport,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
    success_status: StatusCode,
    result: Result<TValue, CoolError>,
) -> Response
where
    TTransport: HttpTransport,
    TValue: Serialize,
{
    let content_type = match select_response_content_type(
        headers,
        capabilities.response_types,
        capabilities.default_response_type,
    ) {
        Ok(content_type) => content_type,
        Err(error) => return fallback_error_response(error),
    };
    match result {
        Ok(value) => transport
            .encode_response(content_type, success_status, &value)
            .unwrap_or_else(fallback_error_response),
        Err(error) => {
            let status = error.status_code();
            let body = error.into_response();
            transport
                .encode_response(content_type, status, &body)
                .unwrap_or_else(fallback_error_response)
        }
    }
}

pub fn encode_transport_sequence_result_with_status_for<TTransport, TValue>(
    transport: &TTransport,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
    success_status: StatusCode,
    result: Result<Vec<TValue>, CoolError>,
) -> Response
where
    TTransport: HttpTransport,
    TValue: Serialize,
{
    if !capabilities.supports_sequence_response {
        return fallback_error_response(CoolError::Internal(
            "sequence response encoding requested for a route without sequence capability"
                .to_owned(),
        ));
    }
    let content_type = match select_response_content_type(
        headers,
        capabilities.response_types,
        capabilities.default_response_type,
    ) {
        Ok(content_type) => content_type,
        Err(error) => return fallback_error_response(error),
    };
    match result {
        Ok(values) => transport
            .encode_sequence_response(content_type, success_status, &values)
            .unwrap_or_else(fallback_error_response),
        Err(error) => {
            let status = error.status_code();
            let body = error.into_response();
            transport
                .encode_sequence_error_response(content_type, status, &body)
                .unwrap_or_else(fallback_error_response)
        }
    }
}

fn encode_cbor_sequence_response<C, T>(
    codec: &C,
    status: StatusCode,
    values: &[T],
) -> Result<Response, CoolError>
where
    C: CoolCodec,
    T: Serialize,
{
    if C::CONTENT_TYPE != CborCodecMarker::CONTENT_TYPE {
        return Err(CoolError::NotAcceptable(
            "cbor-seq requires a CBOR codec".to_owned(),
        ));
    }

    let mut bytes = Vec::new();
    for value in values {
        bytes.extend(codec.encode(value)?);
    }
    encode_bytes_response(status, CBOR_SEQUENCE_CONTENT_TYPE, bytes)
}

fn encode_bytes_response(
    status: StatusCode,
    content_type: &'static str,
    bytes: Vec<u8>,
) -> Result<Response, CoolError> {
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    Ok(response)
}

pub fn encode_codec_result_with_status<C, T>(
    codec: &C,
    success_status: StatusCode,
    result: Result<T, CoolError>,
) -> Response
where
    C: CoolCodec,
    T: Serialize,
{
    match result {
        Ok(value) => encode_codec_response(codec, success_status, &value)
            .unwrap_or_else(fallback_error_response),
        Err(error) => {
            let status = error.status_code();
            let body = error.into_response();
            encode_codec_response(codec, status, &body).unwrap_or_else(fallback_error_response)
        }
    }
}

fn fallback_error_response(error: CoolError) -> Response {
    let mut response = Response::new(Body::from(error.public_message().into_owned()));
    *response.status_mut() = error.status_code();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

/// Parse an `If-Match` header carrying a strong ETag of the form `"<int>"`.
/// Returns `None` if the header is absent. Returns an error if the header
/// is present but malformed (weak validators, non-integer payloads, etc.).
pub fn parse_if_match_version(headers: &HeaderMap) -> Result<Option<i64>, CoolError> {
    let Some(value) = headers.get(header::IF_MATCH) else {
        return Ok(None);
    };
    let raw = value
        .to_str()
        .map_err(|_| CoolError::BadRequest("If-Match header must be ASCII".to_owned()))?
        .trim();
    if raw == "*" {
        return Err(CoolError::BadRequest(
            "If-Match: * is not supported on versioned models".to_owned(),
        ));
    }
    let stripped = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| {
            CoolError::BadRequest(
                "If-Match must be a strong ETag of the form \"<integer>\"".to_owned(),
            )
        })?;
    stripped
        .parse::<i64>()
        .map(Some)
        .map_err(|_| CoolError::BadRequest("If-Match ETag must be an integer".to_owned()))
}

/// Insert an `ETag` header onto a response, formatted as a strong validator
/// over the integer optimistic-locking version.
pub fn set_version_etag(response: &mut Response, version: i64) {
    if let Ok(value) = HeaderValue::from_str(&format!("\"{version}\"")) {
        response.headers_mut().insert(header::ETAG, value);
    }
}

/// Enrich a `CoolContext` with the request id (from `traceparent`) and the
/// client IP (from `Forwarded`/`X-Forwarded-For`). Malformed `traceparent`
/// headers are silently ignored here — the auth/header-validation layer is
/// the right place to reject them, not the enrichment seam.
pub fn enrich_context_from_headers(
    ctx: cratestack_core::CoolContext,
    headers: &HeaderMap,
) -> cratestack_core::CoolContext {
    let mut ctx = ctx;
    if let Ok(Some(trace_id)) = parse_traceparent(headers) {
        ctx = ctx.with_request_id(trace_id);
    }
    if let Some(ip) = parse_client_ip(headers) {
        ctx = ctx.with_client_ip(ip);
    }
    ctx
}

/// Extract a W3C `traceparent` header, returning the trace-id portion when
/// the header is present and well-formed. Returns `Ok(None)` when absent —
/// callers should mint their own request id in that case so every audit row
/// carries something. The trace-id is the second hyphen-delimited segment
/// per [W3C Trace Context]; this implementation does **not** validate the
/// flags/version segments since banks usually rebuild traceparent at the
/// edge anyway.
///
/// [W3C Trace Context]: https://www.w3.org/TR/trace-context/
pub fn parse_traceparent(headers: &HeaderMap) -> Result<Option<String>, CoolError> {
    let Some(value) = headers.get("traceparent") else {
        return Ok(None);
    };
    let raw = value
        .to_str()
        .map_err(|_| CoolError::BadRequest("traceparent must be ASCII".to_owned()))?
        .trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let parts: Vec<&str> = raw.split('-').collect();
    if parts.len() != 4 {
        return Err(CoolError::BadRequest(
            "traceparent must have 4 hyphen-delimited segments".to_owned(),
        ));
    }
    let trace_id = parts[1];
    if trace_id.len() != 32 || !trace_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(CoolError::BadRequest(
            "traceparent trace-id must be 32 lowercase hex characters".to_owned(),
        ));
    }
    if trace_id == "00000000000000000000000000000000" {
        return Err(CoolError::BadRequest(
            "traceparent trace-id must not be all zeros".to_owned(),
        ));
    }
    Ok(Some(trace_id.to_owned()))
}

/// Extract the most-specific client IP available from the request headers,
/// falling back to none. Prefers `Forwarded` (RFC 7239) over the legacy
/// `X-Forwarded-For`. Banks running behind a single trusted L7 take the
/// leftmost entry; deeper proxy chains must verify and rewrite at the edge.
pub fn parse_client_ip(headers: &HeaderMap) -> Option<String> {
    if let Some(forwarded) = headers.get("forwarded").and_then(|v| v.to_str().ok()) {
        for segment in forwarded.split(',').map(str::trim) {
            for kv in segment.split(';').map(str::trim) {
                if let Some(rest) = kv.strip_prefix("for=") {
                    let cleaned = rest.trim_matches('"');
                    let cleaned = cleaned
                        .strip_prefix('[')
                        .and_then(|s| s.strip_suffix(']'))
                        .unwrap_or(cleaned);
                    if !cleaned.is_empty() {
                        return Some(cleaned.to_owned());
                    }
                }
            }
        }
    }
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.split(',').next())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod if_match_tests {
    use super::*;

    fn header_map_with_if_match(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_MATCH, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn returns_none_when_header_absent() {
        let headers = HeaderMap::new();
        assert_eq!(parse_if_match_version(&headers).unwrap(), None);
    }

    #[test]
    fn parses_strong_quoted_integer() {
        let headers = header_map_with_if_match("\"42\"");
        assert_eq!(parse_if_match_version(&headers).unwrap(), Some(42));
    }

    #[test]
    fn rejects_unquoted_payload() {
        let headers = header_map_with_if_match("42");
        let error = parse_if_match_version(&headers).unwrap_err();
        assert_eq!(error.code(), "BAD_REQUEST");
    }

    #[test]
    fn rejects_wildcard() {
        let headers = header_map_with_if_match("*");
        let error = parse_if_match_version(&headers).unwrap_err();
        assert_eq!(error.code(), "BAD_REQUEST");
    }

    #[test]
    fn rejects_non_integer_payload() {
        let headers = header_map_with_if_match("\"v42\"");
        let error = parse_if_match_version(&headers).unwrap_err();
        assert_eq!(error.code(), "BAD_REQUEST");
    }
}

#[cfg(test)]
mod correlation_tests {
    use super::*;

    fn headers_with(name: &'static str, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn traceparent_absent_returns_none() {
        assert!(parse_traceparent(&HeaderMap::new()).unwrap().is_none());
    }

    #[test]
    fn parses_canonical_traceparent_into_trace_id() {
        let h = headers_with(
            "traceparent",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
        );
        let trace_id = parse_traceparent(&h).unwrap().unwrap();
        assert_eq!(trace_id, "0af7651916cd43dd8448eb211c80319c");
    }

    #[test]
    fn rejects_traceparent_with_wrong_segment_count() {
        let h = headers_with("traceparent", "00-deadbeef");
        let err = parse_traceparent(&h).unwrap_err();
        assert_eq!(err.code(), "BAD_REQUEST");
    }

    #[test]
    fn rejects_traceparent_with_short_trace_id() {
        let h = headers_with("traceparent", "00-deadbeef-b7ad6b7169203331-01");
        let err = parse_traceparent(&h).unwrap_err();
        assert_eq!(err.code(), "BAD_REQUEST");
    }

    #[test]
    fn rejects_all_zero_trace_id() {
        let h = headers_with(
            "traceparent",
            "00-00000000000000000000000000000000-b7ad6b7169203331-01",
        );
        let err = parse_traceparent(&h).unwrap_err();
        assert_eq!(err.code(), "BAD_REQUEST");
    }

    #[test]
    fn rfc7239_forwarded_takes_priority_over_x_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            HeaderValue::from_static("for=192.0.2.43;proto=https"),
        );
        headers.insert("x-forwarded-for", HeaderValue::from_static("10.0.0.1"));
        assert_eq!(parse_client_ip(&headers), Some("192.0.2.43".to_owned()));
    }

    #[test]
    fn x_forwarded_for_takes_leftmost_address() {
        let h = headers_with("x-forwarded-for", "192.0.2.43, 10.0.0.1");
        assert_eq!(parse_client_ip(&h), Some("192.0.2.43".to_owned()));
    }

    #[test]
    fn client_ip_strips_brackets_around_ipv6() {
        let h = headers_with("forwarded", "for=\"[2001:db8::1]\"");
        assert_eq!(parse_client_ip(&h), Some("2001:db8::1".to_owned()));
    }
}
