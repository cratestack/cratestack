//! The response AAD binds the kind of request it answers (maintainer
//! decision on cratestack#1005, 2026-09-25).
//!
//! The two request digests are not domain-separated: a signed request's
//! COSE bytes `C` hash exactly like an unsigned request whose
//! `Cratestack-Nonce` is `C[..16]` and whose body is `C[16..]`. Anyone who
//! saw `C` can send that unsigned twin. Before `request_kind`, the server's
//! signed answer to the twin verified at the client as the answer to `C`.

mod common;

use common::{CTI_16, IAT, rpc_request};
use cratestack_core::{CratestackError, RequestKind};
use cratestack_cose::{
    CoseAlg, RequestNonce, UNAUTHENTICATED, external_aad, request_digest, request_digest_unsigned,
};

fn is_coarse_401<T: std::fmt::Debug>(result: &Result<T, CratestackError>) -> bool {
    matches!(result, Err(CratestackError::Unauthorized(message)) if message == UNAUTHENTICATED)
}

/// The reviewer's probe `nonce_digest_is_not_domain_separated_from_the_signed_digest`,
/// inverted: it showed the answer to the twin opening, and must now be
/// refused. It opened on c5c3f7a0.
#[tokio::test]
async fn the_answer_to_an_unsigned_twin_is_not_the_answer_to_the_signed_request() {
    for &alg in CoseAlg::ALL {
        let signed = common::sealed_request(alg, &rpc_request()).await;
        let twin_nonce = RequestNonce::from_bytes(signed[..16].try_into().expect("16 bytes"));
        let twin = request_digest_unsigned(&twin_nonce, &signed[16..]);
        let genuine = request_digest(&signed);
        // Still true, by construction: the fix is in the AAD, not here.
        assert_eq!(twin.digest, genuine.digest, "{alg:?}");
        assert_eq!(
            (twin.kind, genuine.kind),
            (RequestKind::Unsigned, RequestKind::Signed)
        );

        let answer_to_twin = common::server(alg, IAT)
            .seal_response(
                b"\xa1eerrorkbad request",
                &common::answering(&rpc_request(), twin, 400),
            )
            .await
            .expect("seal");
        let client = common::client(alg, IAT, CTI_16);
        let as_answer_to_signed = client
            .open_response(
                answer_to_twin.clone(),
                &common::response_to(&rpc_request(), &signed, 400),
            )
            .await;
        assert!(
            is_coarse_401(&as_answer_to_signed),
            "{alg:?}: digest-form confusion: {as_answer_to_signed:?}"
        );
        // Control: it is a valid answer to the twin itself.
        client
            .open_response(
                answer_to_twin,
                &common::answering(&rpc_request(), twin, 400),
            )
            .await
            .expect("the twin's own answer opens");
    }
}

/// The kind is the 10th AAD element (after `bound_headers`, cratestack#1006
/// S1), before the digest, `0` or `1`.
#[test]
fn the_kind_sits_before_the_digest_in_the_aad() {
    let signed = common::answering(&rpc_request(), request_digest(b"request"), 200);
    let unsigned = common::answering(
        &rpc_request(),
        request_digest_unsigned(&RequestNonce::from_bytes([0; 16]), b"request"),
        200,
    );
    for (bind, code) in [(signed, 0x01), (unsigned, 0x00)] {
        let aad = external_aad(&bind).expect("aad");
        let digest = bind.response.expect("response").request.digest;
        assert_eq!(aad[0], 0x8c, "a 12-element array");
        // ... bound_headers [null, null], kind, bstr(32) digest, uint status (200 = 0x18 0xc8).
        let tail = [
            &[0x82, 0xf6, 0xf6, code, 0x58, 0x20][..],
            &digest,
            &[0x18, 0xc8],
        ]
        .concat();
        assert!(aad.ends_with(&tail), "{aad:02x?}");
    }
    assert_eq!(external_aad(&rpc_request()).expect("aad")[0], 0x89);
}
