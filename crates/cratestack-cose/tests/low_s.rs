//! No third party can re-spell an ESP256 message (security review of
//! cratestack#1005, fix 3).
//!
//! ECDSA's `(r, s)` and `(r, n - s)` both verify. Before the fix the
//! opener accepted both, so a hop could rewrite any ESP256 message into a
//! second valid encoding. `request_digest` hashes the bytes, so the server
//! would then bind its response to bytes the client never sent, and every
//! honest response would fail at the client. Now the sealer normalises
//! every ESP256 signature to low-`s`, whatever signer produced it, and the
//! opener rejects a high `s`.

mod common;

use std::sync::Arc;

use bytes::Bytes;
use common::{CTI_16, IAT, rest_request, rpc_request};
use cratestack_core::CratestackError;
use cratestack_cose::{CoseAlg, CoseEnvelope, CoseMode, CoseSigner, P256Signer, UNAUTHENTICATED};

fn signature(body: &[u8]) -> p256::ecdsa::Signature {
    let range = common::forge::layout(body).signature;
    p256::ecdsa::Signature::from_slice(&body[range]).expect("signature")
}

fn is_low_s(signature: &p256::ecdsa::Signature) -> bool {
    signature.normalize_s() == *signature
}

/// `body` with its signature's `s` replaced by `n - s`.
fn twin(body: &[u8]) -> Vec<u8> {
    let range = common::forge::layout(body).signature;
    let (r, s) = signature(body).split_scalars();
    let twin = p256::ecdsa::Signature::from_scalars(r.to_bytes(), (-*s).to_bytes()).expect("twin");
    let mut out = body.to_vec();
    out[range].copy_from_slice(&twin.to_bytes());
    out
}

fn assert_coarse_401<T: std::fmt::Debug>(result: Result<T, CratestackError>, what: &str) {
    match result {
        Err(CratestackError::Unauthorized(message)) => assert_eq!(message, UNAUTHENTICATED),
        other => panic!("{what}: {other:?}"),
    }
}

#[tokio::test]
async fn the_high_s_twin_of_a_request_is_rejected() {
    let now = common::now();
    let sealed = common::sealed_request_at(CoseAlg::Esp256, &rest_request(), now).await;
    assert!(is_low_s(&signature(&sealed)), "sealed low-s");
    let twin = twin(&sealed);
    assert!(!is_low_s(&signature(&twin)));
    assert_coarse_401(
        common::server(CoseAlg::Esp256, now)
            .open_request(Bytes::from(twin), &rest_request())
            .await,
        "high-s twin accepted",
    );
    common::server(CoseAlg::Esp256, now)
        .open_request(sealed, &rest_request())
        .await
        .expect("the original opens");
}

#[tokio::test]
async fn the_high_s_twin_of_a_response_is_rejected() {
    let request = common::sealed_request(CoseAlg::Ed25519, &rpc_request()).await;
    let bind = common::response_to(&rpc_request(), &request, 200);
    let sealed = common::server(CoseAlg::Esp256, IAT)
        .seal_response(&common::fixture::payment_bytes(), &bind)
        .await
        .expect("seal");
    let client = common::client(CoseAlg::Esp256, IAT, CTI_16);
    assert_coarse_401(
        client
            .open_response(Bytes::from(twin(&sealed)), &bind)
            .await,
        "high-s twin of a response accepted",
    );
    client.open_response(sealed, &bind).await.expect("original");
}

/// A signer that only ever returns high-`s` signatures, through `sign`
/// (like a KMS: no `sign_chunks`).
struct HighSSigner(P256Signer);

#[async_trait::async_trait]
impl CoseSigner for HighSSigner {
    fn alg(&self) -> CoseAlg {
        CoseAlg::Esp256
    }
    fn kid(&self) -> &[u8] {
        self.0.kid()
    }
    async fn sign(&self, tbs: &[u8]) -> Result<Vec<u8>, CratestackError> {
        let low = p256::ecdsa::Signature::from_slice(&self.0.sign(tbs).await?)
            .expect("signature")
            .normalize_s();
        let (r, s) = low.split_scalars();
        let high =
            p256::ecdsa::Signature::from_scalars(r.to_bytes(), (-*s).to_bytes()).expect("high");
        Ok(high.to_bytes().to_vec())
    }
}

#[tokio::test]
async fn the_sealer_normalises_whatever_the_signer_returns() {
    let now = common::now();
    let client = CoseEnvelope::client(
        CoseMode::Sign1,
        Arc::new(HighSSigner(common::p256())),
        common::resolver(),
    )
    .clock(move || i64::try_from(now).expect("fits"))
    .build()
    .expect("client");
    let sealed = client
        .seal_request(&common::fixture::payment_bytes(), &rest_request())
        .await
        .expect("seal");
    assert!(is_low_s(&signature(&sealed)), "normalised on the way out");
    common::server(CoseAlg::Esp256, now)
        .open_request(sealed, &rest_request())
        .await
        .expect("and it verifies");
}

/// The in-process signer is RFC 6979: its raw output is high-`s` about
/// half the time (RFC 6979 A.2.5's own "sample" signature is). Whatever it
/// returns, the message carries low-`s`.
#[tokio::test]
async fn the_rfc6979_signer_goes_out_low_s() {
    let raw = common::p256().sign(b"sample").await.expect("sign");
    assert!(
        !is_low_s(&p256::ecdsa::Signature::from_slice(&raw).expect("sig")),
        "the A.2.5 vector is high-s, so this signer does produce them"
    );
    for cti in ["00", "01", "02", "03", "04", "05", "06", "07"] {
        let sealed = common::client(CoseAlg::Esp256, IAT, cti)
            .seal_request(&common::fixture::payment_bytes(), &rest_request())
            .await
            .expect("seal");
        assert!(is_low_s(&signature(&sealed)), "cti {cti}");
    }
}
