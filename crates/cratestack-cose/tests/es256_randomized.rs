//! ESP256 signatures from a randomized signer (a KMS, WebCrypto) verify
//! like the deterministic ones, once in the low-`s` form the sealer puts
//! every ESP256 signature in. ADR 0006 Q2: the vectors are byte-exact for
//! the in-process RFC 6979 signer and verify-only for everything else.

mod common;

use bytes::Bytes;
use common::fixture::payment_bytes;
use common::forge::{self, TAG_SIGN1};
use common::{CTI_16, rest_request, unhex, unhex32};
use cratestack_cose::{CoseAlg, CoseSigner, external_aad};
use p256::ecdsa::signature::RandomizedSigner;

#[tokio::test]
async fn a_randomized_es256_signature_verifies() {
    let now = common::now();
    let key = p256::ecdsa::SigningKey::from_slice(&unhex32(common::P256_SCALAR)).expect("scalar");
    let aad = external_aad(&rest_request()).expect("aad");
    let payload = payment_bytes();
    let protected = forge::request_protected(
        -9,
        common::p256().kid(),
        u32::try_from(now).expect("u32 until 2106"),
        &unhex(CTI_16),
    );
    let tbs = forge::sign1_tbs(&protected, &aad, &payload);
    let mut rng = rand::rng();
    let first: p256::ecdsa::Signature = key.sign_with_rng(&mut rng, &tbs);
    let second: p256::ecdsa::Signature = key.sign_with_rng(&mut rng, &tbs);
    assert_ne!(first, second, "the signer really is randomized");
    let deterministic = common::p256().sign(&tbs).await.expect("rfc6979");
    assert_ne!(first.to_bytes().to_vec(), deterministic);

    for signature in [first, second] {
        let low = signature.normalize_s();
        let (r, s) = low.split_scalars();
        let high = p256::ecdsa::Signature::from_scalars(r.to_bytes(), (-*s).to_bytes())
            .expect("high-s twin");
        let open = |signature: p256::ecdsa::Signature| {
            let message = forge::assemble(
                TAG_SIGN1,
                &protected,
                &[0xa0],
                &payload,
                &signature.to_bytes(),
            );
            async move {
                common::server(CoseAlg::Esp256, now)
                    .open_request(Bytes::from(message), &rest_request())
                    .await
            }
        };
        open(low)
            .await
            .expect("a randomized ESP256 signature verifies in its low-s form");
        assert!(
            open(high).await.is_err(),
            "its high-s twin is the same signature spelled twice, and is refused"
        );
    }
}
