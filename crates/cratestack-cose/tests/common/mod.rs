//! Shared test support: fixed keys, bindings, the payment fixture, fake
//! backends, and an independent (`coset`-based) way to sign arbitrary
//! protected bytes for the strict-parsing cases.
//!
//! Every key here is a TEST KEY, published in this repository. None of them
//! may ever be used outside tests.

#![allow(dead_code)]

pub mod backends;
pub mod fixture;
pub mod forge;

use std::borrow::Cow;
use std::sync::Arc;

use bytes::Bytes;
use cratestack_core::{Binding, CratestackError, InMemoryNonceStore, NonceStore, PathParams};
use cratestack_cose::{
    CoseAlg, CoseEnvelope, CoseMode, CoseSigner, CoseVerifierResolver, Ed25519Signer, HmacSigner,
    P256Signer, StaticVerifierResolver,
};

/// Ed25519 seed: the bytes 0x00..=0x1f.
pub const ED25519_SEED: [u8; 32] = seq(0x00);
/// P-256 private scalar: RFC 6979 A.2.5's key.
pub const P256_SCALAR: &str = "c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721";
/// HMAC secret: the bytes 0x40..=0x5f.
pub const HMAC_SECRET: [u8; 32] = seq(0x40);
/// A second Ed25519 seed, for "wrong key" candidates.
pub const OTHER_ED25519_SEED: [u8; 32] = seq(0x80);

/// `iat` of every vector: 2026-09-21T13:46:40Z.
pub const IAT: u64 = 1_790_000_000;
/// The 16-byte random-shaped `cti` (§5 `nonce` mode).
pub const CTI_16: &str = "3c9a5e71d20b48f6a1c7e4029b6d5f83";
/// The 2-byte counter-shaped `cti` (the ADR's §3 measurements).
pub const CTI_2: &str = "002a";

/// The schema SHA of every binding: SHA-256 of the ASCII text
/// `cratestack-cose test schema`.
pub fn schema_sha() -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(b"cratestack-cose test schema").into()
}

const fn seq(start: u8) -> [u8; 32] {
    let mut out = [0; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = start + i as u8;
        i += 1;
    }
    out
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn unhex(text: &str) -> Vec<u8> {
    let text: String = text.split_whitespace().collect();
    assert!(text.len().is_multiple_of(2), "odd hex length");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex digit"))
        .collect()
}

pub fn unhex32(text: &str) -> [u8; 32] {
    unhex(text).try_into().expect("32 bytes")
}

pub fn ed25519() -> Ed25519Signer {
    Ed25519Signer::from_seed(&ED25519_SEED)
}

pub fn p256() -> P256Signer {
    P256Signer::from_scalar(&unhex32(P256_SCALAR)).expect("valid scalar")
}

pub fn hmac(alg: CoseAlg) -> HmacSigner {
    HmacSigner::new(alg, HMAC_SECRET.to_vec()).expect("valid HMAC key")
}

/// The signer for `alg`, as a trait object.
pub fn signer(alg: CoseAlg) -> Arc<dyn CoseSigner> {
    match alg {
        CoseAlg::Ed25519 => Arc::new(ed25519()),
        CoseAlg::Esp256 => Arc::new(p256()),
        _ => Arc::new(hmac(alg)),
    }
}

/// A resolver holding every fixed key.
pub fn resolver() -> Arc<dyn CoseVerifierResolver> {
    Arc::new(
        StaticVerifierResolver::new()
            .with_key(ed25519().verify_key())
            .with_key(p256().verify_key())
            .with_key(hmac(CoseAlg::Hmac256_256).verify_key()),
    )
}

pub fn rpc_request() -> Binding<'static> {
    Binding {
        method: Cow::Borrowed("POST"),
        route: Cow::Borrowed("model.Payment.create"),
        path_params: PathParams::EMPTY,
        query: None,
        schema_sha: schema_sha(),
        payload_media_type: Cow::Borrowed("application/cbor"),
        request_digest: None,
        status: None,
    }
}

pub const REST_PATH_PARAMS: &[&str] = &["acc_42", "pay_7"];

pub fn rest_request() -> Binding<'static> {
    Binding {
        method: Cow::Borrowed("PUT"),
        route: Cow::Borrowed("/accounts/{account_id}/payments/{id}"),
        path_params: PathParams::Borrowed(REST_PATH_PARAMS),
        query: Some(Cow::Borrowed("dry_run=false")),
        ..rpc_request()
    }
}

/// The response binding for `request`, answering `request_body`.
pub fn response_to(
    request: &Binding<'static>,
    request_body: &[u8],
    status: u16,
) -> Binding<'static> {
    Binding {
        request_digest: Some(cratestack_cose::request_digest(request_body)),
        status: Some(status),
        ..request.clone()
    }
}

/// A client envelope with the clock pinned at `iat` and `cti` pinned.
pub fn client(alg: CoseAlg, iat: u64, cti: &str) -> CoseEnvelope {
    let cti = unhex(cti);
    let iat = i64::try_from(iat).expect("iat fits");
    CoseEnvelope::client(alg.mode(), signer(alg), resolver())
        .clock(move || iat)
        .cti_source(move || Ok(cti.clone()))
        .build()
        .expect("client envelope")
}

/// A server envelope whose clock reads `now`, with a fresh nonce store.
pub fn server(alg: CoseAlg, now: u64) -> CoseEnvelope {
    server_with(alg, now, resolver(), Arc::new(InMemoryNonceStore::new()))
}

pub fn server_with(
    alg: CoseAlg,
    now: u64,
    resolver: Arc<dyn CoseVerifierResolver>,
    store: Arc<dyn NonceStore>,
) -> CoseEnvelope {
    let now = i64::try_from(now).expect("now fits");
    CoseEnvelope::server(alg.mode(), signer(alg), resolver, store)
        .clock(move || now)
        .build()
        .expect("server envelope")
}

/// Seal the payment fixture as a request with the fixed `iat`/`cti`.
pub async fn sealed_request(alg: CoseAlg, bind: &Binding<'_>) -> Bytes {
    client(alg, IAT, CTI_16)
        .seal_request(&fixture::payment_bytes(), bind)
        .await
        .expect("seal request")
}

pub fn mode_of(alg: CoseAlg) -> CoseMode {
    alg.mode()
}

/// Render an error as everything a peer or a log could see of it.
pub fn render(error: &CratestackError) -> String {
    format!(
        "{error:?}|{error}|{}|{}|{}|{:?}",
        error.code(),
        error.status_code(),
        error.public_message(),
        error.detail(),
    )
}

/// The real current time. Replay tests run on it rather than on [`IAT`]:
/// `InMemoryNonceStore` expires entries against the wall clock, so a nonce
/// recorded for a days-old `iat` is forgotten at once.
pub fn now() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp()).expect("after 1970")
}

/// Seal the payment fixture as a request with `iat` and the fixed `cti`.
pub async fn sealed_request_at(alg: CoseAlg, bind: &Binding<'_>, iat: u64) -> Bytes {
    client(alg, iat, CTI_16)
        .seal_request(&fixture::payment_bytes(), bind)
        .await
        .expect("seal request")
}
