//! RPC server with a list-return procedure streamed via
//! `Accept: application/cbor-seq`.
//!
//! The point of this example is that streaming on the RPC binding is a
//! **content-negotiation** decision, not a separate route. The same
//! `POST /rpc/procedure.ticks` returns:
//!
//! - A single CBOR `Vec<Tick>` when called with the default Accept.
//! - A stream of cbor-seq chunks when called with
//!   `Accept: application/cbor-seq`.
//!
//! The macro emits `OpKind::Sequence` for any procedure whose return type
//! is `T[]`. The framework's existing sequence encoder
//! (`encode_transport_sequence_result_with_status_for`) does the rest —
//! the RPC dispatcher just delegates.

use cratestack::axum::Router;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{AuthProvider, CodecSet, CoolContext, CoolError, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;

cratestack::include_server_schema!("schema.cstack", db = Postgres);

pub use cratestack_schema as schema;

#[derive(Clone, Default)]
pub struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ticks(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::ticks::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ticks::Output, CoolError>,
    > + Send {
        async move {
            let count = args.args.count.max(0);
            let ticks: Vec<cratestack_schema::Tick> = (0..count)
                .map(|index| cratestack_schema::Tick {
                    index,
                    value: args.args.start + index,
                })
                .collect();
            Ok(ticks)
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

pub fn build_router() -> Router {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://example:example@localhost/example".to_owned());
    let pool = PgPoolOptions::new()
        .connect_lazy(&url)
        .expect("connect_lazy parses the URL but opens no socket");
    let db = cratestack_schema::Cratestack::builder(pool).build();

    cratestack_schema::axum::rpc_router(
        db,
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
