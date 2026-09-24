//! `CoseEnvelope` through `cratestack_core::CratestackEnvelope`, and the
//! local-misuse (`500`) paths.

mod common;

use std::sync::Arc;

use bytes::Bytes;
use common::fixture::payment_bytes;
use common::{CTI_16, rest_request, rpc_request};
use cratestack_core::{
    BodyShape, CratestackContext, CratestackEnvelope, CratestackError, InMemoryNonceStore,
};
use cratestack_cose::{CoseAlg, CoseEnvelope, CoseMode, CoseRole, CoseSigner};

#[test]
fn media_types() {
    let sign1 = common::client(CoseAlg::Ed25519, 0, CTI_16);
    let mac0 = common::client(CoseAlg::Hmac256_64, 0, CTI_16);
    assert_eq!(
        sign1.media_type(BodyShape::Unary),
        Some("application/cose; cose-type=\"cose-sign1\"")
    );
    assert_eq!(
        mac0.media_type(BodyShape::Unary),
        Some("application/cose; cose-type=\"cose-mac0\"")
    );
    // P0 signs no streams, and the trait contract ties the two together.
    assert_eq!(sign1.media_type(BodyShape::Stream), None);
    assert!(sign1.stream_sealer(rpc_request()).is_none());
    assert!(sign1.stream_opener(rpc_request()).is_none());
    assert_eq!(sign1.role(), CoseRole::Client);
    assert_eq!(sign1.mode(), CoseMode::Sign1);
}

#[tokio::test]
async fn client_to_server_and_back_through_the_trait() {
    let now = common::now();
    for alg in CoseAlg::ALL {
        let client = common::client(alg, now, CTI_16);
        let server = common::server(alg, now);
        let request = rest_request();

        let body = client
            .seal(Bytes::from(payment_bytes()), &request)
            .await
            .expect("seal");
        let mut ctx = CratestackContext::anonymous();
        let payload = server
            .open(body.clone(), &request, &mut ctx)
            .await
            .expect("open");
        assert_eq!(payload.as_ref(), payment_bytes().as_slice());
        // Zero-copy: the payload lies inside the received body's buffer.
        let range = body.as_ptr() as usize..body.as_ptr() as usize + body.len();
        assert!(range.contains(&(payload.as_ptr() as usize)));
        assert_eq!(
            ctx.verified_signer().map(|signer| signer.kid().to_vec()),
            Some(common::signer(alg).kid().to_vec()),
            "{alg:?}"
        );
        assert!(
            !ctx.is_authenticated(),
            "a verified signer is not an identity"
        );

        let response = common::response_to(&request, &body, 200);
        let sealed = server
            .seal(Bytes::from(payment_bytes()), &response)
            .await
            .expect("seal");
        let mut client_ctx = CratestackContext::anonymous();
        let payload = client
            .open(sealed, &response, &mut client_ctx)
            .await
            .expect("open");
        assert_eq!(payload.as_ref(), payment_bytes().as_slice());
    }
}

#[tokio::test]
async fn a_failed_open_records_no_signer() {
    let server = common::server(CoseAlg::Ed25519, common::now());
    let mut ctx = CratestackContext::anonymous();
    let result = server
        .open(Bytes::from_static(b"\xd2"), &rest_request(), &mut ctx)
        .await;
    assert!(matches!(result, Err(CratestackError::Unauthorized(_))));
    assert!(ctx.verified_signer().is_none());
}

fn is_internal<T: std::fmt::Debug>(result: Result<T, CratestackError>) -> bool {
    matches!(result, Err(CratestackError::Internal(_)))
}

/// The role, not the binding, decides what the trait does: a server handed
/// a request binding for `seal`, or a response binding for `open`, is a
/// router bug, reported as a `500` instead of skipping the replay checks.
#[tokio::test]
async fn a_binding_of_the_wrong_shape_is_local_misuse() {
    let server = common::server(CoseAlg::Ed25519, common::now());
    let client = common::client(CoseAlg::Ed25519, common::now(), CTI_16);
    let request = rest_request();
    let response = common::response_to(&request, b"body", 200);
    let payload = Bytes::from(payment_bytes());
    let mut ctx = CratestackContext::anonymous();

    assert!(is_internal(server.seal(payload.clone(), &request).await));
    assert!(is_internal(client.seal(payload.clone(), &response).await));
    let sealed = client.seal(payload.clone(), &request).await.expect("seal");
    assert!(is_internal(
        server.open(sealed.clone(), &response, &mut ctx).await
    ));
    assert!(is_internal(client.open(sealed, &request, &mut ctx).await));

    // Half a response binding.
    let half = cratestack_core::Binding {
        status: None,
        ..response.clone()
    };
    assert!(is_internal(server.seal(payload.clone(), &half).await));
    assert!(cratestack_cose::external_aad(&half).is_err());
}

#[tokio::test]
async fn a_client_without_a_nonce_store_cannot_open_requests() {
    let client = common::client(CoseAlg::Ed25519, common::now(), CTI_16);
    let sealed = client
        .seal_request(&payment_bytes(), &rest_request())
        .await
        .expect("seal");
    assert!(is_internal(
        client.open_request(sealed, &rest_request()).await
    ));
}

#[tokio::test]
async fn malformed_cti_or_clock_is_refused_at_seal_time() {
    for bad in [Vec::new(), vec![0; 5], vec![0; 17]] {
        let client = CoseEnvelope::client(
            CoseMode::Sign1,
            common::signer(CoseAlg::Ed25519),
            common::resolver(),
        )
        .cti_source(move || Ok(bad.clone()))
        .build()
        .expect("build");
        assert!(is_internal(
            client.seal_request(&payment_bytes(), &rest_request()).await
        ));
    }
    let client = CoseEnvelope::client(
        CoseMode::Sign1,
        common::signer(CoseAlg::Ed25519),
        common::resolver(),
    )
    .clock(|| -1)
    .build()
    .expect("build");
    assert!(is_internal(
        client.seal_request(&payment_bytes(), &rest_request()).await
    ));
}

/// A signer that returns the wrong length (Q5: sizes come from `alg`).
struct ShortSigner;

#[async_trait::async_trait]
impl CoseSigner for ShortSigner {
    fn alg(&self) -> CoseAlg {
        CoseAlg::Ed25519
    }
    fn kid(&self) -> &[u8] {
        &[1, 2, 3, 4, 5, 6, 7, 8]
    }
    async fn sign(&self, _to_be_signed: &[u8]) -> Result<Vec<u8>, CratestackError> {
        Ok(vec![0; 63])
    }
}

#[tokio::test]
async fn a_signature_of_the_wrong_length_never_reaches_the_wire() {
    let client = CoseEnvelope::client(CoseMode::Sign1, Arc::new(ShortSigner), common::resolver())
        .build()
        .expect("build");
    assert!(is_internal(
        client.seal_request(&payment_bytes(), &rest_request()).await
    ));
}

#[test]
fn the_builder_refuses_inconsistent_configuration() {
    // A Mac0 signer in a Sign1 envelope.
    let wrong_mode = CoseEnvelope::server(
        CoseMode::Sign1,
        common::signer(CoseAlg::Hmac256_256),
        common::resolver(),
        Arc::new(InMemoryNonceStore::new()),
    )
    .build();
    assert!(wrong_mode.is_err());
}

#[tokio::test]
async fn the_default_cti_is_sixteen_fresh_random_bytes() {
    let client = CoseEnvelope::client(
        CoseMode::Sign1,
        common::signer(CoseAlg::Ed25519),
        common::resolver(),
    )
    .build()
    .expect("build");
    let server = CoseEnvelope::server(
        CoseMode::Sign1,
        common::signer(CoseAlg::Ed25519),
        common::resolver(),
        Arc::new(InMemoryNonceStore::new()),
    )
    .build()
    .expect("build");
    let mut ctis = Vec::new();
    for _ in 0..2 {
        let sealed = client
            .seal_request(&payment_bytes(), &rest_request())
            .await
            .expect("seal");
        let opened = server
            .open_request(sealed, &rest_request())
            .await
            .expect("open");
        ctis.push(opened.cti.expect("request cti"));
        assert!(opened.iat.is_some());
    }
    assert_eq!(ctis[0].len(), 16);
    assert_ne!(ctis[0], ctis[1]);
}
