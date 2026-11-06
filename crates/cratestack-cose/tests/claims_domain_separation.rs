//! A claim object never opens as a transport envelope, and an envelope
//! never opens as a claim object.
//!
//! The separator is the protected header: the envelope's parser accepts
//! `{1, 4, ? 15}` and refuses label 16 (RFC 9596 `typ`), the claim opener
//! requires it. The attacker's best case is tested: the signature is valid,
//! the key is known to both sides, and the AAD is the one the other format
//! would have used.

mod common;

use bytes::Bytes;
use common::backends::RecordingNonceStore;
use common::{IAT, rest_request, rpc_request};
use cratestack_core::CratestackError;
use cratestack_cose::{
    ClaimsError, CoseAlg, CwtClaims, UNAUTHENTICATED, external_aad, open_claims, sign_claims,
};
use std::sync::Arc;

const TYP: &str = "application/vnd.cratestack.test-grant+cwt";

#[tokio::test]
async fn an_envelope_is_never_a_claim_object() {
    let resolver = common::resolver();
    for alg in [CoseAlg::Ed25519, CoseAlg::Esp256] {
        let request = common::sealed_request(alg, &rest_request()).await;
        let response_bind = common::response_to(&rpc_request(), &request, 200);
        let response = common::server(alg, IAT)
            .seal_response(&common::fixture::payment_bytes(), &response_bind)
            .await
            .expect("seal response");
        for (message, bind) in [(&request, rest_request()), (&response, response_bind)] {
            let aad = external_aad(&bind).expect("aad");
            for typ in [TYP, "", "application/cbor", "cose"] {
                for aad in [aad.as_slice(), b""] {
                    let opened = open_claims(resolver.as_ref(), message, typ, aad).await;
                    assert!(
                        matches!(opened, Err(ClaimsError::Malformed)),
                        "{alg:?} {typ:?}: {opened:?}"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn a_claim_object_is_never_a_transport_envelope() {
    let bind = rest_request();
    let aad = external_aad(&bind).expect("aad");
    // The shape closest to a request: `iat` and a 16-byte `cti` in the claims.
    let envelope_like = CwtClaims {
        iat: Some(IAT),
        cti: Some(common::unhex(common::CTI_16)),
        ..CwtClaims::default()
    };
    for alg in [CoseAlg::Ed25519, CoseAlg::Esp256] {
        let signer = common::signer(alg);
        for claims in [CwtClaims::default(), envelope_like.clone()] {
            let object = sign_claims(
                signer.as_ref(),
                TYP,
                &claims,
                &common::fixture::payment_bytes(),
                &aad,
            )
            .await
            .expect("sign");
            let store = Arc::new(RecordingNonceStore::default());
            let server = common::server_with(alg, IAT, common::resolver(), store.clone());

            let as_request = server
                .open_request(Bytes::from(object.clone()), &bind)
                .await;
            assert_coarse_401(as_request, "claim object opened as a request");
            let response_bind =
                common::answering(&bind, cratestack_cose::request_digest(b"request"), 200);
            let as_response = server
                .open_response(Bytes::from(object), &response_bind)
                .await;
            assert_coarse_401(as_response, "claim object opened as a response");
            assert_eq!(store.calls(), 0, "no nonce is recorded for a claim object");
        }
    }
}

fn assert_coarse_401<T: std::fmt::Debug>(result: Result<T, CratestackError>, what: &str) {
    match result {
        Err(CratestackError::Unauthorized(message)) => assert_eq!(message, UNAUTHENTICATED),
        other => panic!("{what}: {other:?}"),
    }
}
