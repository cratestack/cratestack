//! Shared by `cose_client_{rest,rpc,failures}.rs` (cratestack#1007): the
//! keys and envelopes of a signed exchange, in every mode the client
//! supports, and a tampering proxy to put between the generated client and
//! the generated server. The keys are TEST KEYS, published in this
//! repository: never use them elsewhere.

// Each test binary uses a different subset of these helpers.
#![allow(dead_code, unused_imports)]

mod proxy;

use std::net::SocketAddr;
use std::sync::Arc;

use cratestack::InMemoryNonceStore;
use cratestack::axum::Router;
use cratestack::cose::{
    CoseAlg, CoseEnvelope, CoseMode, CoseSigner, Ed25519Signer, ExternalSigner, HmacSigner,
    P256Signer, StaticVerifierResolver,
};
use cratestack::envelope_layer::{EnvelopeLayer, EnvelopeLayerBuilder, EnvelopeMode};
use cratestack_client_rust::{CborCodec, ClientConfig, ClientEnvelope, CratestackClient};
use p256::ecdsa::signature::Signer as _;

pub use proxy::{Proxy, Tamper, proxy};

pub const AUDIENCE: &str = "payments";

/// What the client signs with. Every one of these must round-trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// COSE_Sign1 with an in-process Ed25519 key.
    Ed25519,
    /// COSE_Sign1 with a P-256 key behind an async callback that answers in
    /// DER, the way a platform keystore does (the vaam mobile core).
    Es256Der,
    /// COSE_Mac0 with a shared HMAC secret.
    Mac0,
}

pub const KINDS: [Kind; 3] = [Kind::Ed25519, Kind::Es256Der, Kind::Mac0];

fn seed(start: u8) -> [u8; 32] {
    std::array::from_fn(|index| start + index as u8)
}

const HMAC_SECRET: [u8; 32] = [0x5a; 32];

fn server_signer() -> Ed25519Signer {
    Ed25519Signer::from_seed(&seed(0x80))
}

fn p256_scalar() -> [u8; 32] {
    seed(0x40)
}

fn hmac() -> HmacSigner {
    HmacSigner::new(CoseAlg::Hmac256_64, HMAC_SECRET.to_vec()).expect("hmac key")
}

fn es256_external() -> ExternalSigner {
    let key = p256::ecdsa::SigningKey::from_slice(&p256_scalar()).expect("scalar");
    let public = P256Signer::from_scalar(&p256_scalar())
        .expect("scalar")
        .verify_key()
        .p256_sec1_uncompressed()
        .expect("sec1");
    ExternalSigner::esp256(&public, move |tbs| {
        let key = key.clone();
        async move {
            let signature: p256::ecdsa::Signature = key.sign(&tbs);
            Ok(signature.to_der().as_bytes().to_vec())
        }
    })
    .expect("external signer")
}

/// The server's envelope, trusting the client's key.
pub fn server_envelope(kind: Kind) -> CoseEnvelope {
    let (mode, signer, resolver): (_, Arc<dyn CoseSigner>, _) = match kind {
        Kind::Ed25519 => (
            CoseMode::Sign1,
            Arc::new(server_signer()),
            StaticVerifierResolver::new().with_key(Ed25519Signer::from_seed(&seed(0)).verify_key()),
        ),
        Kind::Es256Der => (
            CoseMode::Sign1,
            Arc::new(server_signer()),
            StaticVerifierResolver::new().with_key(
                P256Signer::from_scalar(&p256_scalar())
                    .expect("scalar")
                    .verify_key(),
            ),
        ),
        Kind::Mac0 => (
            CoseMode::Mac0,
            Arc::new(hmac()),
            StaticVerifierResolver::new().with_key(hmac().verify_key()),
        ),
    };
    CoseEnvelope::server(
        mode,
        signer,
        Arc::new(resolver),
        Arc::new(InMemoryNonceStore::new()),
    )
    .build()
    .expect("server envelope")
}

/// The client's envelope, addressed to `audience`.
pub fn client_envelope(kind: Kind, audience: &'static str) -> ClientEnvelope {
    let (mode, signer, resolver): (_, Arc<dyn CoseSigner>, _) = match kind {
        Kind::Ed25519 => (
            CoseMode::Sign1,
            Arc::new(Ed25519Signer::from_seed(&seed(0))),
            StaticVerifierResolver::new().with_key(server_signer().verify_key()),
        ),
        Kind::Es256Der => (
            CoseMode::Sign1,
            Arc::new(es256_external()),
            StaticVerifierResolver::new().with_key(server_signer().verify_key()),
        ),
        Kind::Mac0 => (
            CoseMode::Mac0,
            Arc::new(hmac()),
            StaticVerifierResolver::new().with_key(hmac().verify_key()),
        ),
    };
    let cose = CoseEnvelope::client(mode, signer, Arc::new(resolver))
        .build()
        .expect("client envelope");
    ClientEnvelope::new(cose, audience).expect("client envelope")
}

/// The generated `envelope_layer(..)` builder's inputs, in `Required` mode.
pub fn required(builder: EnvelopeLayerBuilder) -> EnvelopeLayerBuilder {
    builder.policy(EnvelopeMode::Required)
}

/// The layer for `kind`, from a schema's generated `envelope_layer`.
pub fn layer(
    kind: Kind,
    generated: impl FnOnce(CoseEnvelope, EnvelopeMode, &'static str) -> EnvelopeLayerBuilder,
) -> EnvelopeLayer {
    generated(server_envelope(kind), EnvelopeMode::Required, AUDIENCE)
        .build()
        .expect("layer")
}

/// A generated router on a real loopback listener.
pub async fn serve(router: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        cratestack::axum::serve(listener, router).await.unwrap();
    });
    addr
}

/// A signing client for `addr`; the caller wraps it in the generated `Client`.
pub fn runtime(addr: SocketAddr, envelope: ClientEnvelope) -> CratestackClient {
    runtime_at(addr, "", envelope)
}

/// As [`runtime`], for a client whose codec is `codec` (cratestack#1168: the
/// envelope carries the codec's own payload type).
pub fn runtime_with<C: cratestack_client_rust::HttpClientCodec>(
    addr: SocketAddr,
    codec: C,
    envelope: ClientEnvelope,
) -> CratestackClient<C> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let base = reqwest::Url::parse(&format!("http://{addr}")).unwrap();
    CratestackClient::new(ClientConfig::new(base), codec)
        .with_envelope(envelope)
        .expect("this codec's payload type can be sealed")
}

/// The payload types a layer that opted in to JSON allows, both ways.
pub const CBOR_AND_JSON: [&str; 2] = ["application/cbor", "application/json"];

/// A signing client for `addr` whose base URL carries `path` (a mount).
pub fn runtime_at(addr: SocketAddr, path: &str, envelope: ClientEnvelope) -> CratestackClient {
    // `reqwest`'s `rustls-no-provider` feature needs a provider installed
    // before the first `Client` is built, even for plain `http://`.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let base = reqwest::Url::parse(&format!("http://{addr}{path}")).unwrap();
    CratestackClient::new(ClientConfig::new(base), CborCodec)
        .with_envelope(envelope)
        .expect("CBOR client takes an envelope")
}

/// A signing client for `addr` that goes through a caller-supplied
/// `reqwest::Client`, which follows redirects (reqwest's default policy).
pub fn runtime_following_redirects(addr: SocketAddr, envelope: ClientEnvelope) -> CratestackClient {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let base = reqwest::Url::parse(&format!("http://{addr}")).unwrap();
    CratestackClient::with_http_client(ClientConfig::new(base), CborCodec, reqwest::Client::new())
        .with_envelope(envelope)
        .expect("CBOR client takes an envelope")
}

/// The COSE algorithm id the server records as the verified signer's.
pub fn alg_of(kind: Kind) -> i64 {
    match kind {
        Kind::Ed25519 => -19,
        Kind::Es256Der => -9,
        Kind::Mac0 => 4,
    }
}

/// How a signed call ended, in the terms the failure tests assert on.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok(String),
    /// The answer was not sealed; nothing in its body was read.
    Unsigned(u16),
    /// The answer was sealed, and did not verify for this request.
    Unverified,
    StreamsUnsupported,
    Other(String),
}

impl From<cratestack_client_rust::EnvelopeError> for Outcome {
    fn from(error: cratestack_client_rust::EnvelopeError) -> Self {
        use cratestack_client_rust::EnvelopeError as E;
        match error {
            E::Unsigned { status } => Outcome::Unsigned(status),
            E::Unverified => Outcome::Unverified,
            E::StreamsUnsupported => Outcome::StreamsUnsupported,
            other => Outcome::Other(other.to_string()),
        }
    }
}
