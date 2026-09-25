//! Strict parsing: only the one encoding the crate emits is accepted.
//!
//! Every malformed message here is *validly signed* over what it carries
//! (the unprotected header and the framing are not covered by the
//! signature, and re-signed protected headers use `coset`'s structure), so
//! each rejection is the strictness check's doing. Each test also opens a
//! control message built the same way that differs only in the checked
//! property.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use common::fixture::payment_bytes;
use common::forge::{self, TAG_SIGN1, bstr, layout, with_unprotected};
use common::{CTI_16, IAT, rest_request, unhex};
use cratestack_core::{CratestackError, InMemoryNonceStore};
use cratestack_cose::{
    CoseAlg, CoseSigner, CoseVerifierResolver, CoseVerifyKey, Opened, UNAUTHENTICATED, external_aad,
};

async fn open(body: Vec<u8>) -> Result<Opened, CratestackError> {
    common::server(CoseAlg::Ed25519, IAT)
        .open_request(Bytes::from(body), &rest_request())
        .await
}

async fn rejects(body: Vec<u8>, what: &str) {
    match open(body).await {
        Err(CratestackError::Unauthorized(message)) => {
            assert_eq!(message, UNAUTHENTICATED, "{what}")
        }
        other => panic!("{what}: expected the coarse 401, got {other:?}"),
    }
}

async fn valid() -> Vec<u8> {
    common::sealed_request(CoseAlg::Ed25519, &rest_request())
        .await
        .to_vec()
}

#[tokio::test]
async fn the_control_message_opens() {
    open(valid().await).await.expect("control");
}

#[tokio::test]
async fn a_non_empty_unprotected_header_rejects() {
    let sealed = valid().await;
    let kid = common::ed25519().kid().to_vec();
    // `kid` in the unprotected header, where most COSE libraries accept it.
    let mut kid_map = vec![0xa1, 0x04];
    kid_map.extend(bstr(&kid));
    rejects(with_unprotected(&sealed, &kid_map), "{4: kid} unprotected").await;
    // An IV, a content type, a counter signature placeholder.
    rejects(
        with_unprotected(&sealed, &unhex("a1 05 41 00")),
        "{5: h'00'}",
    )
    .await;
    rejects(with_unprotected(&sealed, &unhex("a1 03 00")), "{3: 0}").await;
    // The empty map, spelled indefinitely.
    rejects(with_unprotected(&sealed, &unhex("bf ff")), "indefinite {}").await;
    // Not a map at all.
    rejects(with_unprotected(&sealed, &unhex("80")), "[]").await;
    rejects(with_unprotected(&sealed, &unhex("f6")), "null").await;
}

#[tokio::test]
async fn indefinite_lengths_reject() {
    let sealed = valid().await;
    let l = layout(&sealed);
    // Indefinite outer array: 9f … ff.
    let mut array = sealed.clone();
    array[1] = 0x9f;
    array.push(0xff);
    rejects(array, "indefinite array").await;
    // Indefinite (chunked) payload: 5f <chunk> ff, same content bytes.
    let mut chunked = sealed[..l.payload.start - 2].to_vec();
    chunked.push(0x5f);
    chunked.extend(bstr(&sealed[l.payload.clone()]));
    chunked.push(0xff);
    chunked.extend_from_slice(&sealed[l.payload.end..]);
    rejects(chunked, "indefinite payload").await;
}

#[tokio::test]
async fn non_minimal_heads_reject() {
    let sealed = valid().await;
    let l = layout(&sealed);
    // Payload length 112 as a 2-byte argument: 59 00 70 instead of 58 70.
    let mut payload = sealed[..l.payload.start - 2].to_vec();
    payload.extend_from_slice(&[0x59, 0x00, 0x70]);
    payload.extend_from_slice(&sealed[l.payload.start..]);
    rejects(payload, "non-minimal payload head").await;
    // Tag 18 with a 1-byte argument: d8 12 instead of d2.
    let mut tag = vec![0xd8, 0x12];
    tag.extend_from_slice(&sealed[1..]);
    rejects(tag, "non-minimal tag").await;
    // Array of 4 with a 1-byte argument: 98 04.
    let mut array = vec![0xd2, 0x98, 0x04];
    array.extend_from_slice(&sealed[2..]);
    rejects(array, "non-minimal array head").await;
}

#[tokio::test]
async fn framing_errors_reject() {
    let sealed = valid().await;
    let mut trailing = sealed.clone();
    trailing.push(0x00);
    rejects(trailing, "one trailing byte").await;
    let mut twice = sealed.clone();
    twice.extend_from_slice(&sealed);
    rejects(twice, "two messages").await;
    rejects(sealed[1..].to_vec(), "untagged").await;
    rejects(sealed[..sealed.len() - 1].to_vec(), "truncated").await;
    let mut three = sealed.clone();
    three[1] = 0x83;
    rejects(three, "three elements").await;
    let mut five = sealed.clone();
    five[1] = 0x85;
    five.push(0x40);
    rejects(five, "five elements").await;
    rejects(Vec::new(), "empty body").await;
    // A hostile length: a protected bstr claiming 2^64 - 1 bytes.
    rejects(unhex("d2 84 5b ff ff ff ff ff ff ff ff"), "huge length").await;
}

#[tokio::test]
async fn a_detached_payload_rejects() {
    let sealed = valid().await;
    let l = layout(&sealed);
    let mut detached = sealed[..l.payload.start - 2].to_vec();
    detached.push(0xf6);
    detached.extend_from_slice(&sealed[l.payload.end..]);
    rejects(detached, "nil payload").await;
}

/// Sign `protected` properly and open it; `Ok` means accepted.
async fn with_protected(protected: &[u8]) -> Result<Opened, CratestackError> {
    let aad = external_aad(&rest_request()).expect("aad");
    open(forge::ed25519_request(protected, &aad, &payment_bytes())).await
}

async fn protected_rejects(hex: &str, what: &str) {
    match with_protected(&protected_from(hex)).await {
        Err(CratestackError::Unauthorized(message)) => {
            assert_eq!(message, UNAUTHENTICATED, "{what}")
        }
        other => panic!("{what}: expected the coarse 401, got {other:?}"),
    }
}

/// Fill `KID`, `IAT` and `CTI` placeholders in a hex template.
fn protected_from(template: &str) -> Vec<u8> {
    let kid = common::hex(common::ed25519().kid());
    let iat = common::hex(&u32::try_from(IAT).expect("u32").to_be_bytes());
    unhex(
        &template
            .replace("KID", &kid)
            .replace("IAT", &iat)
            .replace("CTI", CTI_16),
    )
}

#[tokio::test]
async fn the_protected_template_control_opens() {
    with_protected(&protected_from(
        "a3 01 32 04 48 KID 0f a2 06 1a IAT 07 50 CTI",
    ))
    .await
    .expect("canonical header opens");
}

#[tokio::test]
async fn duplicate_unknown_or_misordered_labels_reject() {
    protected_rejects(
        "a4 01 32 01 32 04 48 KID 0f a2 06 1a IAT 07 50 CTI",
        "duplicate alg",
    )
    .await;
    protected_rejects(
        "a4 01 32 04 48 KID 04 48 KID 0f a2 06 1a IAT 07 50 CTI",
        "duplicate kid",
    )
    .await;
    protected_rejects(
        "a4 01 32 03 00 04 48 KID 0f a2 06 1a IAT 07 50 CTI",
        "label 3",
    )
    .await;
    protected_rejects(
        "a4 01 32 04 48 KID 05 41 00 0f a2 06 1a IAT 07 50 CTI",
        "label 5",
    )
    .await;
    protected_rejects(
        "a4 01 32 04 48 KID 0f a2 06 1a IAT 07 50 CTI 18 64 00",
        "label 100",
    )
    .await;
    protected_rejects(
        "a3 04 48 KID 01 32 0f a2 06 1a IAT 07 50 CTI",
        "kid before alg",
    )
    .await;
    protected_rejects(
        "a3 01 32 0f a2 06 1a IAT 07 50 CTI 04 48 KID",
        "claims before kid",
    )
    .await;
    protected_rejects("a2 01 32 0f a2 06 1a IAT 07 50 CTI", "no kid").await;
    protected_rejects("a2 04 48 KID 0f a2 06 1a IAT 07 50 CTI", "no alg").await;
}

#[tokio::test]
async fn claims_must_be_exactly_iat_then_cti() {
    protected_rejects(
        "a3 01 32 04 48 KID 0f a3 06 1a IAT 07 50 CTI 04 1a IAT",
        "exp claim",
    )
    .await;
    protected_rejects(
        "a3 01 32 04 48 KID 0f a2 07 50 CTI 06 1a IAT",
        "cti before iat",
    )
    .await;
    protected_rejects("a3 01 32 04 48 KID 0f a1 07 50 CTI", "no iat").await;
    protected_rejects("a3 01 32 04 48 KID 0f a1 06 1a IAT", "no cti").await;
    protected_rejects(
        "a3 01 32 04 48 KID 0f a2 06 1a IAT 06 1a IAT",
        "duplicate iat",
    )
    .await;
    protected_rejects("a2 01 32 04 48 KID", "a request without claims").await;
}

#[tokio::test]
async fn wrong_types_and_sizes_reject() {
    protected_rejects(
        "a3 01 65 45 64 44 53 41 04 48 KID 0f a2 06 1a IAT 07 50 CTI",
        "alg as text",
    )
    .await;
    protected_rejects(
        "a3 01 32 04 68 KID 0f a2 06 1a IAT 07 50 CTI",
        "kid as text",
    )
    .await;
    protected_rejects(
        "a3 01 32 04 47 KID 0f a2 06 1a IAT 07 50 CTI",
        "kid of 7 bytes",
    )
    .await;
    protected_rejects(
        "a3 01 32 04 49 KID 00 0f a2 06 1a IAT 07 50 CTI",
        "kid of 9 bytes",
    )
    .await;
    protected_rejects(
        "a3 01 32 04 48 KID 0f a2 06 3a IAT 07 50 CTI",
        "negative iat",
    )
    .await;
    protected_rejects(
        "a3 01 32 04 48 KID 0f a2 06 44 IAT 07 50 CTI",
        "iat as bytes",
    )
    .await;
    protected_rejects(
        "a3 01 32 04 48 KID 0f a2 06 1a IAT 07 70 CTI",
        "cti as text",
    )
    .await;
    protected_rejects("a3 01 32 04 48 KID 0f a2 06 1a IAT 07 40", "empty cti").await;
    protected_rejects(
        "a3 01 32 04 48 KID 0f a2 06 1a IAT 07 45 0102030405",
        "5-byte cti",
    )
    .await;
    protected_rejects(
        "a3 01 32 04 48 KID 0f a2 06 1a IAT 07 51 CTI 00",
        "17-byte cti",
    )
    .await;
    protected_rejects("a3 01 32 04 48 KID 0f 80", "claims as array").await;
    protected_rejects(
        "a3 01 38 12 04 48 KID 0f a2 06 1a IAT 07 50 CTI",
        "non-minimal alg",
    )
    .await;
    protected_rejects(
        "a3 01 32 04 58 08 KID 0f a2 06 1a IAT 07 50 CTI",
        "non-minimal kid head",
    )
    .await;
    protected_rejects(
        "a3 01 32 04 48 KID 0f a2 06 1b 00000000 IAT 07 50 CTI",
        "non-minimal iat",
    )
    .await;
    protected_rejects(
        "bf 01 32 04 48 KID 0f a2 06 1a IAT 07 50 CTI ff",
        "indefinite map",
    )
    .await;
    protected_rejects(
        "a3 01 32 04 48 KID 0f a2 06 1a IAT 07 50 CTI 00",
        "trailing byte",
    )
    .await;
}

#[tokio::test]
async fn accepted_cti_shapes_are_one_to_four_or_sixteen_bytes() {
    for cti in ["01", "0102", "010203", "01020304", CTI_16] {
        let len = unhex(cti).len();
        let hex = format!(
            "a3 01 32 04 48 KID 0f a2 06 1a IAT 07 {:02x} {cti}",
            0x40 + len
        );
        with_protected(&protected_from(&hex))
            .await
            .unwrap_or_else(|e| panic!("{len}-byte cti: {e:?}"));
    }
}

#[tokio::test]
async fn a_request_does_not_open_as_a_response_nor_the_reverse() {
    let request = valid().await;
    let response = common::response_to(&rest_request(), &request, 200);
    let client = common::client(CoseAlg::Ed25519, IAT, CTI_16);
    // A request carries claims, which a response must not.
    assert!(matches!(
        client
            .open_response(Bytes::from(request.clone()), &response)
            .await,
        Err(CratestackError::Unauthorized(_))
    ));
    // A response (no claims) offered as a request, with a signature that is
    // valid over the request binding.
    let aad = external_aad(&rest_request()).expect("aad");
    let bare = protected_from("a2 01 32 04 48 KID");
    let message = forge::assemble(
        TAG_SIGN1,
        &bare,
        &[0xa0],
        &payment_bytes(),
        &forge::ed25519_sign(&forge::sign1_tbs(&bare, &aad, &payment_bytes())),
    );
    rejects(message, "claims-less request").await;
}

/// A resolver that ignores `kid` and `alg` and counts its calls.
struct CountingResolver(Arc<AtomicUsize>);

#[async_trait::async_trait]
impl CoseVerifierResolver for CountingResolver {
    async fn resolve(
        &self,
        _kid: &[u8],
        _alg: CoseAlg,
    ) -> Result<Vec<CoseVerifyKey>, CratestackError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(vec![common::ed25519().verify_key()])
    }
}

/// A `kid` is exactly 8 bytes, enforced by the header parser before any
/// key is looked up (security review of cratestack#1005, fix 4). The
/// resolver here is kid-blind, so it would hand over the signing key for
/// any `kid`, and the messages are correctly signed by that key: only the
/// length rule refuses them. It must also refuse them before the resolver
/// sees attacker-sized input, which is what makes this test fail if the
/// length check is deleted even though the later "the candidate's own kid
/// is the header's" check would also refuse the message.
#[tokio::test]
async fn a_7_or_9_byte_kid_is_refused_before_the_resolver() {
    let now = common::now();
    let aad = external_aad(&rest_request()).expect("aad");
    let payload = payment_bytes();
    for len in [7, 9, 0, 32] {
        let protected = forge::request_protected(
            -19,
            &vec![0x01; len],
            u32::try_from(now).expect("u32"),
            &unhex(CTI_16),
        );
        let body = forge::ed25519_request(&protected, &aad, &payload);
        let calls = Arc::new(AtomicUsize::new(0));
        let server = common::server_with(
            CoseAlg::Ed25519,
            now,
            Arc::new(CountingResolver(calls.clone())),
            Arc::new(InMemoryNonceStore::new()),
        );
        match server
            .open_request(Bytes::from(body), &rest_request())
            .await
        {
            Err(CratestackError::Unauthorized(message)) => assert_eq!(message, UNAUTHENTICATED),
            other => panic!("a {len}-byte kid: {other:?}"),
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a {len}-byte kid reached the resolver"
        );
    }
    // Control: the same construction with the real 8-byte kid opens, and
    // does consult the resolver.
    let kid = common::ed25519().verify_key().kid();
    let protected =
        forge::request_protected(-19, &kid, u32::try_from(now).expect("u32"), &unhex(CTI_16));
    let calls = Arc::new(AtomicUsize::new(0));
    common::server_with(
        CoseAlg::Ed25519,
        now,
        Arc::new(CountingResolver(calls.clone())),
        Arc::new(InMemoryNonceStore::new()),
    )
    .open_request(
        Bytes::from(forge::ed25519_request(&protected, &aad, &payload)),
        &rest_request(),
    )
    .await
    .expect("control opens");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
