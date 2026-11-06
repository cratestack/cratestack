//! A signed, compact-capable RPC service: `transferFunds` behind a COSE
//! `EnvelopeLayer` (ADR 0006) that also opts in to compact wire keys
//! (`docs/design/compact-wire-keys.md` section 5.1).
//!
//! The layering, outside in: the observer (what crossed the wire) ->
//! `EnvelopeLayer` (opens the seal, seals the answer) -> the generated
//! router, whose compact layer turns compact bodies back into named ones for
//! the handlers. Handlers never see compact keys.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use cratestack::axum::Router;
use cratestack::axum::extract::Request;
use cratestack::axum::middleware::{Next, from_fn};
use cratestack::compact::COMPACT_CBOR;
use cratestack::cose::{CoseAlg, CoseEnvelope, CoseMode, HmacSigner, StaticVerifierResolver};
use cratestack::envelope_layer::EnvelopeMode;
use cratestack::{
    AuthProvider, CodecSet, CratestackContext, CratestackError, InMemoryNonceStore, RequestContext,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;

cratestack::include_server_schema!("../schema.cstack", db = None);

pub use cratestack::axum;
pub use cratestack_schema as schema;

/// The audience the envelope is addressed to; the client names the same one.
pub const AUDIENCE: &str = "payments";

/// DEMO SECRET, FOR LOCAL USE ONLY. It is published in this repository, so
/// anyone can seal requests to a server that trusts it. A real deployment
/// reads a per-client secret (or uses asymmetric keys: `Ed25519Signer`,
/// `P256Signer`) from a secret store and never commits it. 32 bytes, as
/// `Hmac256_64` wants. The client example carries the same bytes.
pub const DEMO_SECRET: &[u8; 32] = &[0x5a; 32];

/// The `Cratestack-Payload-Type` of one call, as seen on the wire (outside
/// the envelope layer). Empty means the header was absent: plain named CBOR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireCall {
    pub request: String,
    pub response: String,
    /// The outer `Content-Type` each way: `application/cose...` when sealed.
    pub request_content_type: String,
    pub response_content_type: String,
}

pub type Observer = Arc<dyn Fn(WireCall) + Send + Sync>;

pub struct Options {
    /// Allow the compact payload types under the seal, in both directions.
    /// `false` is "an old server": the layer lists only plain CBOR.
    pub compact: bool,
    pub observer: Option<Observer>,
}

/// The service, sealed with a shared HMAC `secret` (COSE_Mac0).
pub fn app(secret: &[u8], options: Options) -> Router {
    let hmac = || HmacSigner::new(CoseAlg::Hmac256_64, secret.to_vec()).expect("a 32-byte secret");
    let envelope = CoseEnvelope::server(
        CoseMode::Mac0,
        Arc::new(hmac()),
        Arc::new(StaticVerifierResolver::new().with_key(hmac().verify_key())),
        Arc::new(InMemoryNonceStore::new()),
    )
    .build()
    .expect("server envelope");

    let router = schema::axum::rpc_router(
        schema::Cratestack::builder().build(),
        Procedures::default(),
        (),
        CodecSet::new(CborCodec, JsonCodec),
        Anonymous,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );

    let mut builder = schema::axum::envelope_layer(envelope, EnvelopeMode::Required, AUDIENCE);
    if options.compact {
        // The opt-in, both directions: without the request half a compact
        // request is refused (unsigned 415); without the response half the
        // client is never told compact exists. Plain CBOR stays allowed.
        builder = builder.payload_media_types(
            ["application/cbor", COMPACT_CBOR],
            ["application/cbor", COMPACT_CBOR],
        );
    }
    let router = router.layer(builder.build().expect("layer"));

    match options.observer {
        Some(observer) => router.layer(from_fn(move |req: Request, next: Next| {
            let observer = observer.clone();
            async move {
                let request = header(req.headers(), "cratestack-payload-type");
                let request_content_type = header(req.headers(), "content-type");
                let answer = next.run(req).await;
                let response = header(answer.headers(), "cratestack-payload-type");
                let response_content_type = header(answer.headers(), "content-type");
                observer(WireCall {
                    request,
                    response,
                    request_content_type,
                    response_content_type,
                });
                answer
            }
        })),
        None => router,
    }
}

fn header(headers: &cratestack::axum::http::HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

#[derive(Clone, Default)]
pub struct Procedures {
    next_id: Arc<AtomicI64>,
}

impl schema::procedures::ProcedureRegistry for Procedures {
    async fn transfer_funds(
        &self,
        _db: &schema::Cratestack,
        _ctx: &CratestackContext,
        args: schema::procedures::transfer_funds::Args,
        _authorized: schema::procedures::transfer_funds::Authorized,
    ) -> Result<schema::procedures::transfer_funds::Output, CratestackError> {
        let transfer = args.input;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        Ok(schema::Transaction {
            id: format!("txn_{id:04}"),
            status: "settled".to_owned(),
            fromAccount: transfer.fromAccount,
            toAccount: transfer.toAccount,
            amountMinor: transfer.amountMinor,
            currency: transfer.currency,
            reference: transfer.reference,
            createdAtUnix: 1_700_000_000 + id,
        })
    }
}

/// Authentication is the envelope here (a verified seal is the caller's
/// proof); the handlers see an anonymous context.
#[derive(Clone)]
pub struct Anonymous;

impl AuthProvider for Anonymous {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::anonymous()))
    }
}
