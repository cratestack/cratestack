//! The audience in the AAD (maintainer decision on cratestack#1005,
//! 2026-09-24): a configured logical id of the receiving service, bound
//! into every message at 0 wire bytes.

mod common;

use std::borrow::Cow;
use std::sync::Arc;

use bytes::Bytes;
use common::rpc_request;
use cratestack_core::{Binding, CratestackError, InMemoryNonceStore};
use cratestack_cose::{
    CoseAlg, CoseEnvelope, CoseMode, HmacSigner, StaticVerifierResolver, UNAUTHENTICATED,
    external_aad,
};

fn addressed_to(audience: &'static str) -> Binding<'static> {
    Binding {
        audience: Cow::Borrowed(audience),
        ..rpc_request()
    }
}

fn is_coarse_401<T: std::fmt::Debug>(result: &Result<T, CratestackError>) -> bool {
    matches!(result, Err(CratestackError::Unauthorized(message)) if message == UNAUTHENTICATED)
}

#[test]
fn the_audience_is_the_second_aad_element() {
    let aad = external_aad(&addressed_to("svc-a")).expect("aad");
    // [1, "svc-a", "POST", ...]: array(8), 1, tstr(5) "svc-a", tstr(4) "POST".
    assert_eq!(&aad[..13], b"\x88\x01\x65svc-a\x64POST");
    assert_ne!(aad, external_aad(&addressed_to("svc-b")).expect("aad"));
}

/// Two services share a schema, a route and the client's key. A request
/// sealed for one is refused by the other.
#[tokio::test]
async fn a_request_for_one_service_is_refused_by_another() {
    let now = common::now();
    for &alg in CoseAlg::ALL {
        let sealed = common::sealed_request_at(alg, &addressed_to("payments"), now).await;
        let at_ledger = common::server(alg, now)
            .open_request(sealed.clone(), &addressed_to("ledger"))
            .await;
        assert!(is_coarse_401(&at_ledger), "{alg:?}: {at_ledger:?}");
        common::server(alg, now)
            .open_request(sealed, &addressed_to("payments"))
            .await
            .expect("the addressed service opens it");
    }
}

/// Services A and B trust each other through one COSE_Mac0 secret. Each
/// seals its outgoing requests for the other's audience and opens incoming
/// requests with its own. A request A sends to B, captured and replayed
/// back at A itself, is A's own message under the shared key: without the
/// audience A could not tell it from one of B's. With it, A refuses it.
#[tokio::test]
async fn a_mac0_request_reflected_back_to_its_sender_is_refused() {
    let now = common::now();
    let service = |outbound: HmacSigner| {
        let clock = move || i64::try_from(now).expect("fits");
        let resolver = Arc::new(
            StaticVerifierResolver::new().with_key(common::hmac(CoseAlg::Hmac256_256).verify_key()),
        );
        let client =
            CoseEnvelope::client(CoseMode::Mac0, Arc::new(outbound.clone()), resolver.clone())
                .clock(clock)
                .build()
                .expect("client side");
        let server = CoseEnvelope::server(
            CoseMode::Mac0,
            Arc::new(outbound),
            resolver,
            Arc::new(InMemoryNonceStore::new()),
        )
        .clock(clock)
        .build()
        .expect("server side");
        (client, server)
    };
    let (a_out, a_in) = service(common::hmac(CoseAlg::Hmac256_256));
    let (_b_out, b_in) = service(common::hmac(CoseAlg::Hmac256_256));
    // A's inbound audience is "svc-a"; its outbound target is "svc-b".
    let a_inbound = addressed_to("svc-a");
    let b_inbound = addressed_to("svc-b");

    let a_to_b = a_out
        .seal_request(&common::fixture::payment_bytes(), &b_inbound)
        .await
        .expect("A seals for B");
    let reflected = a_in.open_request(Bytes::clone(&a_to_b), &a_inbound).await;
    assert!(is_coarse_401(&reflected), "reflected to A: {reflected:?}");
    b_in.open_request(a_to_b, &b_inbound)
        .await
        .expect("B, the addressee, opens it");
}

/// Responses carry the audience too: the answer one service gave is not
/// the answer of another.
#[tokio::test]
async fn a_response_from_one_service_does_not_verify_as_anothers() {
    let request = common::sealed_request(CoseAlg::Ed25519, &addressed_to("payments")).await;
    let from_payments = common::response_to(&addressed_to("payments"), &request, 200);
    let sealed = common::server(CoseAlg::Ed25519, common::IAT)
        .seal_response(&common::fixture::payment_bytes(), &from_payments)
        .await
        .expect("seal");
    let as_ledger = Binding {
        audience: Cow::Borrowed("ledger"),
        ..from_payments.clone()
    };
    let client = common::client(CoseAlg::Ed25519, common::IAT, common::CTI_16);
    assert!(is_coarse_401(
        &client.open_response(sealed.clone(), &as_ledger).await
    ));
    client
        .open_response(sealed, &from_payments)
        .await
        .expect("control");
}

fn is_misuse<T: std::fmt::Debug>(result: &Result<T, CratestackError>) -> bool {
    matches!(result, Err(CratestackError::Internal(message)) if message.contains("audience"))
}

/// An empty audience binds no recipient, so it would silently give up both
/// protections above. It is local misuse, a `500` (maintainer decision on
/// cratestack#1005, 2026-09-25), when sealing and when opening, for
/// requests and responses, and `external_aad` refuses to encode it. It
/// sealed and opened on c5c3f7a0.
#[tokio::test]
async fn an_empty_audience_is_local_misuse() {
    let empty = addressed_to("");
    assert!(is_misuse(&external_aad(&empty)));
    for &alg in CoseAlg::ALL {
        let client = common::client(alg, common::IAT, common::CTI_16);
        let server = common::server(alg, common::IAT);
        let payload = common::fixture::payment_bytes();
        assert!(is_misuse(&client.seal_request(&payload, &empty).await));

        let request = common::sealed_request(alg, &rpc_request()).await;
        // Refused before the body is looked at: even a valid message is a
        // 500, so the outcome depends on local state only.
        assert!(is_misuse(
            &server.open_request(request.clone(), &empty).await
        ));
        assert!(is_misuse(
            &server
                .open_request(Bytes::from_static(b"\xd2"), &empty)
                .await
        ));

        let response = common::response_to(&empty, &request, 200);
        assert!(is_misuse(&server.seal_response(&payload, &response).await));
        let sealed = server
            .seal_response(
                &payload,
                &common::response_to(&rpc_request(), &request, 200),
            )
            .await
            .expect("seal");
        assert!(is_misuse(&client.open_response(sealed, &response).await));
    }
}

/// Why the inbound audience must differ from the outbound one: a service
/// that uses one name (`"internal"`) both for itself and for its peers
/// accepts its own reflected Mac0 request. This pins the documented
/// hazard; it is configuration the envelope cannot see.
#[tokio::test]
async fn a_shared_audience_name_gives_up_reflection_protection() {
    let now = common::now();
    let shared = addressed_to("internal");
    let sealed = common::sealed_request_at(CoseAlg::Hmac256_256, &shared, now).await;
    common::server(CoseAlg::Hmac256_256, now)
        .open_request(sealed, &shared)
        .await
        .expect("a sender that is also the addressee cannot tell its own message apart");
}
