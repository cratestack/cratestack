//! RPC server with a list-return, `@stream`-marked procedure streamed via
//! `Accept: application/cbor-seq`.
//!
//! The point of this example is twofold:
//!
//! - Streaming on the RPC binding is a **content-negotiation** decision,
//!   not a separate route. The same `POST /rpc/procedure.ticks` returns a
//!   single CBOR `Vec<Tick>` with the default Accept, or a stream of
//!   cbor-seq chunks with `Accept: application/cbor-seq`.
//! - `@stream` (cratestack#282) makes that genuinely incremental at the
//!   *implementer* boundary: `Procedures::ticks` below returns a real
//!   `impl Stream`, producing one `Tick` at a time via `async_stream::stream!`
//!   instead of collecting a `Vec` up front. `tests/stream_incremental.rs`
//!   proves this by polling the stream directly and observing that the
//!   first item arrives long before the last one — not just that the
//!   final content happens to be correct.
//!
//! The HTTP response itself is still buffered end-to-end for now (see
//! `procedure_invoke_call_tokens` in `cratestack-macros`) — wiring
//! `@stream` into a genuinely incremental `Body::from_stream` response is
//! cratestack-axum's concern, tracked separately as cratestack#283. This
//! example's job is only to prove the new trait shape is real and usable,
//! not to change the wire behavior.
//!
//! The macro emits `OpKind::Sequence` for any procedure whose return type
//! is `T[]`, `@stream` or not. The framework's existing sequence encoder
//! (`encode_transport_sequence_result_with_status_for`) does the rest —
//! the RPC dispatcher just delegates.

use std::time::Duration;

use cratestack::axum::Router;
use cratestack::futures::Stream;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{AuthProvider, CodecSet, CoolContext, CoolError, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;

cratestack::include_server_schema!("schema.cstack", db = Postgres);

pub use cratestack_schema as schema;

/// Delay between successive `Tick`s, standing in for whatever a real
/// incremental source would `.await` on (a DB cursor, an upstream feed
/// subscription, ...) — long enough for
/// `tests/stream_incremental.rs` to observe clear separation between
/// "first item ready" and "last item ready" without making the test
/// suite noticeably slower.
const TICK_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Clone, Default)]
pub struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ticks(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::ticks::Args,
    ) -> impl Stream<Item = Result<cratestack_schema::Tick, CoolError>> + Send {
        async_stream::stream! {
            let count = args.args.count.max(0);
            for index in 0..count {
                tokio::time::sleep(TICK_INTERVAL).await;
                yield Ok(cratestack_schema::Tick {
                    index,
                    value: args.args.start + index,
                });
            }
        }
    }
}

#[derive(Clone)]
pub struct HeaderAuthProvider;

impl AuthProvider for HeaderAuthProvider {
    type Error = CoolError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        let ctx = request
            .headers
            .get("x-auth-id")
            .and_then(|value| value.to_str().ok())
            .and_then(|raw| raw.parse::<i64>().ok())
            .map(|id| CoolContext::authenticated([("id".to_owned(), Value::Int(id))]))
            .unwrap_or_else(CoolContext::anonymous);
        core::future::ready(Ok(ctx))
    }
}

/// Lazily-connected `Cratestack` handle — opens no socket, so it's cheap
/// to build both for the real server (`build_router`) and for tests that
/// only need a handle to satisfy `ProcedureRegistry::ticks`'s signature
/// (the example's `ticks` doesn't touch the DB at all; see
/// `tests/stream_incremental.rs`).
pub fn build_db() -> cratestack_schema::Cratestack {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://example:example@localhost/example".to_owned());
    let pool = PgPoolOptions::new()
        .connect_lazy(&url)
        .expect("connect_lazy parses the URL but opens no socket");
    cratestack_schema::Cratestack::builder(pool).build()
}

pub fn build_router() -> Router {
    cratestack_schema::axum::rpc_router(
        build_db(),
        Procedures,
        CodecSet::new(CborCodec, JsonCodec),
        HeaderAuthProvider,
    )
}

/// Decode a buffered cbor-seq body into a `Vec<T>`. The wire format is
/// concatenated CBOR items with no length prefix — a real client would
/// decode each chunk as it arrives (see `tests/smoke.rs` for the pattern).
/// This helper buffers everything first, which is fine for the example.
pub fn decode_cbor_seq<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Vec<T> {
    let mut values = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let mut deserializer = minicbor_serde::Deserializer::new(&bytes[offset..]);
        let value = T::deserialize(&mut deserializer).expect("cbor-seq item should decode");
        values.push(value);
        let consumed = deserializer.decoder().position();
        assert!(consumed > 0, "decoder must make progress on each chunk");
        offset += consumed;
    }
    values
}
