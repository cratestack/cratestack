//! `bound_headers` in the AAD (maintainer decision S1 after the
//! cratestack#1006 security review): `Idempotency-Key` and `If-Match` are
//! bound exactly as sent, so an on-path party can neither strip, add, alter
//! nor swap them on a signed request or response.

mod common;

use std::borrow::Cow;

use common::{IAT, rest_request, rpc_request};
use cratestack_core::{Binding, BoundHeaders, CratestackError};
use cratestack_cose::{CoseAlg, UNAUTHENTICATED, external_aad};

fn with(idempotency_key: Option<&'static str>, if_match: Option<&'static str>) -> Binding<'static> {
    Binding {
        bound_headers: BoundHeaders {
            idempotency_key: idempotency_key.map(Cow::Borrowed),
            if_match: if_match.map(Cow::Borrowed),
        },
        ..rpc_request()
    }
}

/// The array sits right after `payload_type`, fixed-length and in order,
/// `null` for an absent header. The bytes a port must reproduce.
#[test]
fn the_bound_headers_are_a_two_element_array_after_the_payload_type() {
    let aad = external_aad(&with(Some("k1"), None)).expect("aad");
    assert_eq!(aad[0], 0x89, "array(9) for a request");
    // ... tstr(16) "application/cbor", array(2), tstr(2) "k1", null.
    assert!(
        aad.ends_with(b"\x70application/cbor\x82\x62k1\xf6"),
        "{aad:02x?}"
    );
    let neither = external_aad(&with(None, None)).expect("aad");
    assert!(neither.ends_with(b"\x70application/cbor\x82\xf6\xf6"));
    let both = external_aad(&with(Some("k1"), Some("\"3\""))).expect("aad");
    assert!(both.ends_with(b"\x82\x62k1\x63\"3\""));
    // An empty value is bound as `""`, not as an absent header.
    let empty = external_aad(&with(Some(""), None)).expect("aad");
    assert!(empty.ends_with(b"\x82\x60\xf6"));
}

fn assert_rejected<T: std::fmt::Debug>(result: Result<T, CratestackError>, what: &str) {
    match result {
        Err(CratestackError::Unauthorized(message)) => {
            assert_eq!(message, UNAUTHENTICATED, "{what}")
        }
        other => panic!("{what}: expected the coarse 401, got {other:?}"),
    }
}

/// Sealed with the REST request's `Idempotency-Key` and `If-Match`, opened
/// with each tampering a proxy could try.
#[tokio::test]
async fn stripping_adding_altering_or_swapping_a_bound_header_is_refused() {
    let signed = rest_request();
    let tampered: [(&str, BoundHeaders<'static>); 5] = [
        (
            "key stripped",
            BoundHeaders {
                idempotency_key: None,
                ..signed.bound_headers.clone()
            },
        ),
        (
            "if-match stripped",
            BoundHeaders {
                if_match: None,
                ..signed.bound_headers.clone()
            },
        ),
        (
            "key altered",
            BoundHeaders {
                idempotency_key: Some(Cow::Borrowed("idem-7f3b")),
                ..signed.bound_headers.clone()
            },
        ),
        (
            "key re-spelled with a space",
            BoundHeaders {
                idempotency_key: Some(Cow::Borrowed(" idem-7f3a")),
                ..signed.bound_headers.clone()
            },
        ),
        (
            "swapped",
            BoundHeaders {
                idempotency_key: signed.bound_headers.if_match.clone(),
                if_match: signed.bound_headers.idempotency_key.clone(),
            },
        ),
    ];
    for &alg in CoseAlg::ALL {
        let sealed = common::sealed_request(alg, &signed).await;
        for (what, headers) in &tampered {
            let other = Binding {
                bound_headers: headers.clone(),
                ..rest_request()
            };
            let opened = common::server(alg, IAT)
                .open_request(sealed.clone(), &other)
                .await;
            assert_rejected(opened, &format!("{what} {alg:?}"));
        }
        // Added to a request that carried neither.
        let bare = rpc_request();
        let sealed = common::sealed_request(alg, &bare).await;
        let opened = common::server(alg, IAT)
            .open_request(sealed.clone(), &with(Some("k1"), None))
            .await;
        assert_rejected(opened, &format!("key added {alg:?}"));
        common::server(alg, IAT)
            .open_request(sealed, &bare)
            .await
            .expect("the control opens");
    }
}

/// A response repeats its request's bound headers: one stored under a key
/// does not verify as the answer to the same request without it.
#[tokio::test]
async fn a_response_is_bound_to_its_requests_headers() {
    for &alg in CoseAlg::ALL {
        let request = rest_request();
        let body = common::sealed_request(alg, &request).await;
        let response = common::response_to(&request, &body, 200);
        let sealed = common::server(alg, IAT)
            .seal_response(b"\xa0", &response)
            .await
            .expect("seal");
        let unkeyed = Binding {
            bound_headers: BoundHeaders::NONE,
            ..response.clone()
        };
        assert_rejected(
            common::client(alg, IAT, common::CTI_16)
                .open_response(sealed.clone(), &unkeyed)
                .await,
            &format!("{alg:?}"),
        );
        common::client(alg, IAT, common::CTI_16)
            .open_response(sealed, &response)
            .await
            .expect("the control opens");
    }
}
