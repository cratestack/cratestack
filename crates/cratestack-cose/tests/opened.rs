//! What a verified message hands back: nothing that keeps the received
//! body alive longer than the payload does, and nothing a log line could
//! leak (security and API review of cratestack#1005, round 2).

mod common;

use common::{CTI_16, IAT, rpc_request};
use cratestack_core::{CratestackContext, CratestackEnvelope};
use cratestack_cose::CoseAlg;

/// `Opened`'s `Debug` prints the payload's length, never its bytes.
#[tokio::test]
async fn debug_does_not_print_the_payload() {
    let payload = b"\xa1fsecretx\x18do-not-log-this-account-id";
    let sealed = common::client(CoseAlg::Ed25519, IAT, CTI_16)
        .seal_request(payload, &rpc_request())
        .await
        .expect("seal");
    let opened = common::server(CoseAlg::Ed25519, IAT)
        .open_request(sealed, &rpc_request())
        .await
        .expect("open");
    let shown = format!("{opened:?}");
    assert!(!shown.contains("do-not-log"), "{shown}");
    assert!(
        shown.contains(&format!("payload_len: {}", payload.len())),
        "{shown}"
    );
    assert_eq!(opened.kid, common::ed25519().verify_key().kid());
    assert_eq!(
        opened.thumbprint,
        common::ed25519().verify_key().thumbprint()
    );
}

/// The `kid` a context records is a copy: a slice of the body would keep
/// the whole body alive for as long as the context lives.
#[tokio::test]
async fn the_recorded_kid_does_not_point_into_the_body() {
    let sealed = common::client(CoseAlg::Ed25519, IAT, CTI_16)
        .seal_request(&common::fixture::payment_bytes(), &rpc_request())
        .await
        .expect("seal");
    let body = sealed.as_ptr_range();
    let body = body.start as usize..body.end as usize;
    let mut ctx = CratestackContext::anonymous();
    let payload = common::server(CoseAlg::Ed25519, IAT)
        .open(sealed, &rpc_request(), &mut ctx)
        .await
        .expect("open");
    // Control: the payload is a zero-copy slice of the body.
    assert!(body.contains(&(payload.as_ptr() as usize)));
    let signer = ctx.verified_signer().expect("recorded");
    assert_eq!(signer.kid(), common::ed25519().verify_key().kid());
    assert!(
        !body.contains(&(signer.kid().as_ptr() as usize)),
        "the recorded kid is a slice of the received body"
    );
}
