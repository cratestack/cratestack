//! `ServiceKeySigner`: a `cratestack_auth::ServiceSigningKey` as a COSE
//! signer (`auth` feature, cratestack#1005 part B).

mod common;

use std::sync::Arc;

use common::{ED25519_SEED, OTHER_ED25519_SEED, rpc_request};
use cratestack_auth::ServiceSigningKey;
use cratestack_core::{CratestackError, InMemoryNonceStore};
use cratestack_cose::auth::ServiceKeySigner;
use cratestack_cose::thumbprint::{kid_from_thumbprint, okp_ed25519_thumbprint};
use cratestack_cose::{
    CoseAlg, CoseEnvelope, CoseMode, CoseSigner, Ed25519Signer, StaticVerifierResolver,
    UNAUTHENTICATED,
};
use ed25519_dalek::SigningKey;

const JWKS_LABEL: &str = "vendor-service-v1";

fn service_key(seed: &[u8; 32]) -> ServiceSigningKey {
    ServiceSigningKey::new(
        "https://vendor.internal",
        JWKS_LABEL,
        SigningKey::from_bytes(seed),
    )
}

#[test]
fn the_cose_kid_is_the_thumbprint_prefix_not_the_jwks_label() {
    let signer = ServiceKeySigner::new(service_key(&ED25519_SEED));
    let public = SigningKey::from_bytes(&ED25519_SEED).verifying_key();
    let expected = kid_from_thumbprint(&okp_ed25519_thumbprint(public.as_bytes()));

    assert_eq!(signer.kid(), expected.as_slice());
    assert_eq!(signer.kid(), Ed25519Signer::from_seed(&ED25519_SEED).kid());
    assert_ne!(signer.kid(), &JWKS_LABEL.as_bytes()[..8]);
    assert_eq!(signer.service_key().kid(), JWKS_LABEL, "the label is kept");
    assert_eq!(signer.alg(), CoseAlg::Ed25519);
    assert_eq!(
        signer.verify_key(),
        Ed25519Signer::from_seed(&ED25519_SEED).verify_key()
    );
}

/// `sign` and the streamed `sign_chunks` both give exactly what the
/// crate's own `Ed25519Signer` gives for the same seed.
#[tokio::test]
async fn signatures_are_byte_identical_to_ed25519_signer() {
    let service = ServiceKeySigner::new(service_key(&ED25519_SEED));
    let reference = Ed25519Signer::from_seed(&ED25519_SEED);
    for len in [0usize, 1, 63, 64, 65, 1000, 70_003] {
        let message: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
        let expected = reference.sign(&message).await.expect("reference");
        assert_eq!(service.sign(&message).await.expect("sign"), expected);

        let (a, rest) = message.split_at(len / 3);
        let (b, c) = rest.split_at(rest.len() / 2);
        let streamed = service
            .sign_chunks(&[a, b, c])
            .expect("in-process signer streams")
            .expect("sign_chunks");
        assert_eq!(streamed, expected, "len {len}");
    }
}

fn server(resolver: StaticVerifierResolver, now: u64) -> CoseEnvelope {
    let now = i64::try_from(now).expect("fits");
    CoseEnvelope::server(
        CoseMode::Sign1,
        Arc::new(Ed25519Signer::from_seed(&OTHER_ED25519_SEED)),
        Arc::new(resolver),
        Arc::new(InMemoryNonceStore::new()),
    )
    .clock(move || now)
    .build()
    .expect("server")
}

async fn sealed_by_service(now: u64) -> (ServiceKeySigner, bytes::Bytes) {
    let signer = ServiceKeySigner::new(service_key(&ED25519_SEED));
    let now_i = i64::try_from(now).expect("fits");
    let client = CoseEnvelope::client(
        CoseMode::Sign1,
        Arc::new(signer.clone()),
        Arc::new(StaticVerifierResolver::new()),
    )
    .clock(move || now_i)
    .build()
    .expect("client");
    let sealed = client
        .seal_request(&common::fixture::payment_bytes(), &rpc_request())
        .await
        .expect("seal");
    (signer, sealed)
}

#[tokio::test]
async fn a_sealed_request_opens_with_a_resolver_built_from_its_public_key() {
    let now = common::now();
    let (signer, sealed) = sealed_by_service(now).await;
    let server = server(
        StaticVerifierResolver::new().with_key(signer.verify_key()),
        now,
    );

    let opened = server
        .open_request(sealed, &rpc_request())
        .await
        .expect("opens");
    assert_eq!(opened.kid.as_slice(), signer.kid());
    assert_eq!(opened.thumbprint, signer.verify_key().thumbprint());
    assert_eq!(opened.alg, CoseAlg::Ed25519);
    assert_eq!(&opened.payload[..], &common::fixture::payment_bytes()[..]);
}

/// The control for the test above: the same message, with a resolver that
/// holds some other key, is the coarse `401`.
#[tokio::test]
async fn it_does_not_open_with_another_keys_resolver() {
    let now = common::now();
    let (_, sealed) = sealed_by_service(now).await;
    let other = Ed25519Signer::from_seed(&OTHER_ED25519_SEED).verify_key();
    let server = server(StaticVerifierResolver::new().with_key(other), now);

    match server.open_request(sealed, &rpc_request()).await {
        Err(CratestackError::Unauthorized(message)) => assert_eq!(message, UNAUTHENTICATED),
        other => panic!("expected the coarse 401, got {other:?}"),
    }
}

/// `Debug` names the issuer, the JWKS label and the COSE `kid`, and never
/// the secret, in any encoding a log line might carry it in. The seed is
/// high-entropy on purpose, so an accidental match is not a small number.
#[test]
fn debug_output_carries_no_secret_material() {
    use base64::Engine as _;
    use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};

    let seed: [u8; 32] =
        std::array::from_fn(|i| u8::try_from(i).expect("fits").wrapping_mul(73) ^ 0x9d);
    let signing_key = SigningKey::from_bytes(&seed);
    let signer = ServiceKeySigner::new(service_key(&seed));

    let secrets = [
        format!("{seed:?}"),
        format!("{seed:#?}"),
        format!("{:?}", &seed[..16]),
        common::hex(&seed),
        common::hex(&seed).to_uppercase(),
        STANDARD.encode(seed),
        URL_SAFE_NO_PAD.encode(seed),
        cratestack_auth::encode_signing_key(&signing_key),
        common::hex(&signing_key.to_keypair_bytes()),
    ];
    for rendered in [format!("{signer:?}"), format!("{signer:#?}")] {
        assert!(rendered.contains(JWKS_LABEL), "{rendered}");
        for secret in &secrets {
            assert!(
                !rendered.contains(secret.as_str()),
                "Debug leaks the seed as {secret}: {rendered}"
            );
        }
    }
}
