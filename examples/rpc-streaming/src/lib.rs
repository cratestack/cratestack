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
//! The HTTP response itself is now genuinely incremental too
//! (cratestack#283): `cratestack-axum`'s `Body::from_stream`-backed
//! encoder (`crates/cratestack-axum/src/transport/stream_sequence.rs`)
//! flushes each item onto the wire as it's produced, instead of
//! buffering the whole sequence first. `tests/stream_wire_timing.rs`
//! proves this over a real HTTP response (item N arrives before item
//! N+1 is even produced server-side); `tests/stream_disconnect.rs`
//! proves a client disconnect actually stops server-side production
//! instead of leaking it. Non-`@stream` `T[]` procedures are
//! deliberately unaffected — see `procedure_invoke_call_tokens` in
//! `cratestack-macros` for the byte-identical-behavior guarantee.
//!
//! The macro emits `OpKind::Sequence` for any procedure whose return type
//! is `T[]`, `@stream` or not. `@stream`-marked ones additionally route
//! through the incremental encoder
//! (`encode_transport_stream_result_with_status_for`) when
//! `application/cbor-seq` is negotiated; everything else — including a
//! `@stream` op under a plain JSON `Accept` — still goes through the
//! original buffered `encode_transport_sequence_result_with_status_for`.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use cratestack::axum::Router;
use cratestack::futures::Stream;
use cratestack::{
    AuthProvider, CodecSet, CratestackContext, CratestackError, RequestContext, Value,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;

cratestack::include_server_schema!("schema.cstack", db = None);

pub use cratestack_schema as schema;

/// Delay between successive `Tick`s, standing in for whatever a real
/// incremental source would `.await` on (a DB cursor, an upstream feed
/// subscription, ...) — long enough for
/// `tests/stream_incremental.rs` to observe clear separation between
/// "first item ready" and "last item ready" without making the test
/// suite noticeably slower.
const TICK_INTERVAL: Duration = Duration::from_millis(20);

/// `produced`/`produced_at` are test-only instrumentation (unused by
/// the real server binary, which only ever constructs
/// `Procedures::default()`): they let integration tests observe *when*
/// each item was actually produced server-side, independent of when it
/// reaches a client over the wire —
/// `tests/stream_disconnect.rs` uses `produced` to prove item
/// production actually stops shortly after a client disconnects (not
/// silently running to completion), and
/// `tests/stream_wire_timing.rs` uses `produced_at` to prove item N
/// arrives over the real HTTP response before item N+1 was even
/// produced server-side.
#[derive(Clone, Default)]
pub struct Procedures {
    pub produced: Arc<AtomicUsize>,
    pub produced_at: Arc<Mutex<Vec<Instant>>>,
}

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ticks(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::ticks::Args,
        _authorized: cratestack_schema::procedures::ticks::Authorized,
    ) -> impl Stream<Item = Result<cratestack_schema::Tick, CratestackError>> + Send {
        let produced = self.produced.clone();
        let produced_at = self.produced_at.clone();
        async_stream::stream! {
            let count = args.args.count.max(0);
            for index in 0..count {
                tokio::time::sleep(TICK_INTERVAL).await;
                produced.fetch_add(1, Ordering::SeqCst);
                produced_at.lock().unwrap().push(Instant::now());
                yield Ok(cratestack_schema::Tick {
                    index,
                    value: args.args.start + index,
                });
            }
        }
    }

    /// Same shape as `ticks`, but always fails on the item after the
    /// last successful one it yields — a genuinely-streaming server
    /// hitting a mid-stream error. `cratestack-axum`'s incremental
    /// encoder (cratestack#283) turns that trailing `Err` into the
    /// real, wire-accurate CBOR-tagged error sentinel (cratestack#281)
    /// via the exact same production code path `ticks` uses for its
    /// successful items — no hand-rolled bytes. See
    /// `tests/stream_ts_fixture_bytes.rs`, which captures this
    /// procedure's real response bytes as a fixture for
    /// `packages/cratestack-ts-types`'s TypeScript boundary-scanner
    /// tests (issue #277).
    fn flaky_ticks(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::flaky_ticks::Args,
        _authorized: cratestack_schema::procedures::flaky_ticks::Authorized,
    ) -> impl Stream<Item = Result<cratestack_schema::Tick, CratestackError>> + Send {
        async_stream::stream! {
            let count = args.args.count.max(0);
            for index in 0..count {
                yield Ok(cratestack_schema::Tick {
                    index,
                    value: args.args.start + index,
                });
            }
            yield Err(CratestackError::Internal("flakyTicks always fails after its successful items".to_owned()));
        }
    }
}

#[derive(Clone)]
pub struct HeaderAuthProvider;

impl AuthProvider for HeaderAuthProvider {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let ctx = request
            .headers
            .get("x-auth-id")
            .and_then(|value| value.to_str().ok())
            .and_then(|raw| raw.parse::<i64>().ok())
            .map(|id| CratestackContext::authenticated([("id".to_owned(), Value::Int(id))]))
            .unwrap_or_else(CratestackContext::anonymous);
        core::future::ready(Ok(ctx))
    }
}

/// `db = None` handle — no `PgPool`, no connection string, nothing to
/// open. Cheap to build both for the real server (`build_router`) and for
/// tests that only need a handle to satisfy `ProcedureRegistry::ticks`'s
/// signature (the example's `ticks` doesn't touch a DB at all; see
/// `tests/stream_incremental.rs`).
pub fn build_db() -> cratestack_schema::Cratestack {
    cratestack_schema::Cratestack::builder().build()
}

pub fn build_router() -> Router {
    build_router_with(Procedures::default())
}

/// Same router the real server (and `build_router`) mounts, parameterized
/// over the `Procedures` instance — lets tests supply one whose
/// `produced`/`produced_at` fields they've kept a handle to, so they can
/// observe server-side production behavior (timing, cancellation)
/// alongside driving the router over real HTTP.
pub fn build_router_with(procedures: Procedures) -> Router {
    cratestack_schema::axum::rpc_router(
        build_db(),
        procedures,
        CodecSet::new(CborCodec, JsonCodec),
        HeaderAuthProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
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
