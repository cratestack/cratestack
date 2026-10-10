//! Shared helpers for the `bigint_*` Postgres tests (ADR 0019 PR B, B11).
//!
//! Written against the spec in `docs/adr/0019-int-and-bigint-built-in-types.md`
//! (D2, D3) and `docs/design/int-and-bigint.md` section 7, not against the
//! generators: a helper here knows what a `BigInt` looks like on the wire (a
//! canonical decimal string, a CBOR text string), never how the macros emit it.
//!
//! Lives in a directory with a `mod.rs` so cargo does not treat it as its own
//! test binary; a test file opts in with `mod bigint_support;`.

#![allow(dead_code)] // each test binary uses a subset of these helpers

use cratestack::axum::Router;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{HeaderMap, Request, StatusCode};
use cratestack::serde_json::{self, Value as Json};
use cratestack::sqlx::PgPool;
use cratestack::{CratestackCodec, CratestackContext, CratestackError, Value};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_migrate::diff;
use cratestack_migrate::emit::postgres;
use cratestack_parser::parse_schema;
use tower::util::ServiceExt;

/// `i64::MAX`, the largest value a `BigInt` holds.
pub const I64_MAX: &str = "9223372036854775807";
/// `i64::MIN`, the smallest value a `BigInt` holds.
pub const I64_MIN: &str = "-9223372036854775808";
/// `2^53 + 1`, the first integer a JavaScript `number` cannot hold exactly.
pub const ABOVE_2_53: &str = "9007199254740993";

/// The three boundary values of the ADR, as the canonical string and as `i64`.
pub const BOUNDARY: [(&str, i64); 3] = [
    (I64_MAX, i64::MAX),
    (I64_MIN, i64::MIN),
    (ABOVE_2_53, 9_007_199_254_740_993),
];

/// The two wire formats of the server. Every behavioural test runs on both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wire {
    Json,
    Cbor,
}

impl Wire {
    pub const BOTH: [Wire; 2] = [Wire::Json, Wire::Cbor];

    pub fn content_type(self) -> &'static str {
        match self {
            Wire::Json => JsonCodec::CONTENT_TYPE,
            Wire::Cbor => CborCodec::CONTENT_TYPE,
        }
    }

    pub fn encode(self, value: &Json) -> Vec<u8> {
        match self {
            Wire::Json => JsonCodec.encode(value).expect("encode JSON"),
            Wire::Cbor => CborCodec.encode(value).expect("encode CBOR"),
        }
    }

    pub fn decode(self, bytes: &[u8]) -> Json {
        if bytes.is_empty() {
            return Json::Null;
        }
        match self {
            Wire::Json => JsonCodec
                .decode(bytes)
                .unwrap_or_else(|e| panic!("response is not JSON ({e}): {bytes:02x?}")),
            Wire::Cbor => CborCodec
                .decode(bytes)
                .unwrap_or_else(|e| panic!("response is not CBOR ({e}): {bytes:02x?}")),
        }
    }
}

/// One HTTP answer, kept as raw bytes so a test can inspect the wire itself.
pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
    pub wire: Wire,
}

impl Reply {
    pub fn json(&self) -> Json {
        self.wire.decode(&self.bytes)
    }

    pub fn etag(&self) -> Option<String> {
        self.headers
            .get("etag")
            .map(|v| v.to_str().expect("ascii etag").to_owned())
    }

    /// Asserts the status, printing the decoded body when it is wrong.
    #[track_caller]
    pub fn expect(self, status: StatusCode) -> Self {
        assert_eq!(
            self.status,
            status,
            "unexpected status ({:?} wire), body: {}",
            self.wire,
            self.body_for_message()
        );
        self
    }

    fn body_for_message(&self) -> String {
        match self.wire {
            Wire::Json => String::from_utf8_lossy(&self.bytes).into_owned(),
            Wire::Cbor => format!("{:02x?}", self.bytes),
        }
    }
}

/// Sends one request. `body` is a JSON value encoded with `wire`'s codec.
pub async fn call(
    router: &Router,
    method: &str,
    path: &str,
    wire: Wire,
    headers: &[(&str, &str)],
    body: Option<&Json>,
) -> Reply {
    call_raw(
        router,
        method,
        path,
        wire,
        headers,
        body.map(|value| wire.encode(value)),
    )
    .await
}

/// Sends one request with a body the caller assembled byte by byte.
pub async fn call_raw(
    router: &Router,
    method: &str,
    path: &str,
    wire: Wire,
    headers: &[(&str, &str)],
    body: Option<Vec<u8>>,
) -> Reply {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("accept", wire.content_type());
    if body.is_some() {
        builder = builder.header("content-type", wire.content_type());
    }
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = builder
        .body(body.map_or_else(Body::empty, Body::from))
        .expect("request should build");
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("request should be served");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read")
        .to_vec();
    Reply {
        status,
        headers,
        bytes,
        wire,
    }
}

/// Auth for every test that does not vary the caller: an authenticated operator.
pub fn operator_auth(_headers: &HeaderMap) -> Result<CratestackContext, CratestackError> {
    Ok(CratestackContext::authenticated([(
        "id".to_owned(),
        Value::Int(1),
    )]))
}

/// A CBOR text string (major type 3) holding `text`.
pub fn cbor_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    match text.len() {
        len @ 0..=23 => bytes.push(0x60 | len as u8),
        len @ 24..=255 => bytes.extend([0x78, len as u8]),
        _ => panic!("only short text strings are built"),
    }
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A definite-length CBOR map from `(key, already encoded value)` pairs,
/// assembled by hand: encoding a Rust value can only ever produce the forms
/// the encoder chooses, never the forbidden ones a test needs to send.
pub fn cbor_map(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    assert!(entries.len() < 24, "only short maps are built");
    let mut bytes = vec![0xa0 | entries.len() as u8];
    for (key, value) in entries {
        bytes.extend(cbor_text(key));
        bytes.extend(value);
    }
    bytes
}

/// A one-element definite-length CBOR array.
pub fn cbor_list(item: Vec<u8>) -> Vec<u8> {
    let mut bytes = vec![0x81];
    bytes.extend(item);
    bytes
}

/// True when `haystack` carries `text` as a CBOR text string (header and all).
pub fn has_cbor_text(haystack: &[u8], text: &str) -> bool {
    let needle = cbor_text(text);
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_slice())
}

/// True when `haystack` carries `n` as an 8-byte CBOR integer (major type 0 or
/// 1), the form a `BigInt` must never take on the wire.
pub fn has_cbor_integer(haystack: &[u8], n: i64) -> bool {
    let needle: Vec<u8> = if n >= 0 {
        [&[0x1b][..], &n.to_be_bytes()[..]].concat()
    } else {
        [&[0x3b][..], &(!n).to_be_bytes()[..]].concat()
    };
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_slice())
}

/// The DDL `cratestack migrate diff` would write for `schema_src`: the real
/// emitter, never a hand-written `CREATE TABLE`, so a scalar the emitter maps
/// to `TEXT` instead of `BIGINT` fails on the first bind (ADR 0019 risk 3).
pub fn emitted_up(schema_src: &str) -> String {
    let empty = parse_schema("").expect("empty schema parses");
    let next = parse_schema(schema_src).expect("fixture parses");
    postgres::emit(&diff(&empty, &next).expect("diff should succeed")).up
}

/// Drops `tables` (a comma-separated list, children first) and applies the
/// emitted DDL for `schema_src`. Does not touch `cratestack_migrations`, so
/// the binaries sharing an external database do not race on it.
pub async fn reset(pool: &PgPool, schema_src: &str, tables: &str) {
    reset_with(pool, schema_src, tables, |up| up).await;
}

/// [`reset`] with a rewrite of the emitted DDL, for the one thing the emitter
/// writes that Postgres cannot run: an auth-derived `@default(auth().claim)` is
/// copied into the column as `DEFAULT auth().claim` (true for every scalar, not
/// only `BigInt`; the default is applied by the runtime, never by the database).
pub async fn reset_with(
    pool: &PgPool,
    schema_src: &str,
    tables: &str,
    patch: impl FnOnce(String) -> String,
) {
    // Test-only: `tables` is a literal list written by the calling test.
    cratestack::sqlx::query(cratestack::sqlx::AssertSqlSafe(format!(
        "DROP TABLE IF EXISTS {tables} CASCADE"
    )))
    .execute(pool)
    .await
    .expect("drop tables");
    cratestack::sqlx::raw_sql(cratestack::sqlx::AssertSqlSafe(patch(emitted_up(
        schema_src,
    ))))
    .execute(pool)
    .await
    .expect("emitter-generated DDL must apply against real Postgres");
}

/// `SELECT <one bigint column> ...` bound to one `i64`, as the exact `i64`.
pub async fn db_i64(pool: &PgPool, sql: &'static str, bind: i64) -> i64 {
    cratestack::sqlx::query_scalar::<_, i64>(sql)
        .bind(bind)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| panic!("`{sql}` failed: {e}"))
}

/// The Postgres `data_type` of `table.column`.
pub async fn column_type(pool: &PgPool, table: &str, column: &str) -> String {
    cratestack::sqlx::query_scalar::<_, String>(
        "SELECT data_type FROM information_schema.columns \
         WHERE table_name = $1 AND column_name = $2",
    )
    .bind(table)
    .bind(column)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|e| panic!("no column {table}.{column}: {e}"))
}

pub fn obj(value: &Json) -> &serde_json::Map<String, Json> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("expected a JSON object, got {value}"))
}
