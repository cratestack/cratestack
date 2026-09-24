//! The principal is the key that verified, and a key verifies only under
//! its own `kid` (security review of cratestack#1005, fix 1).
//!
//! Before the fix, the opener tried every candidate the resolver returned
//! and recorded the header's `kid`. With a resolver that returns more than
//! the exact `kid` match (every active key of a tenant, say), any enrolled
//! key could sign a message under another key's `kid`, and the context
//! recorded the victim as the signer.

mod common;

use std::sync::Arc;

use common::backends::FixedResolver;
use common::{CTI_16, rest_request, rpc_request};
use cratestack_core::{CratestackContext, CratestackEnvelope, CratestackError, InMemoryNonceStore};
use cratestack_cose::{
    CoseAlg, CoseEnvelope, CoseMode, CoseSigner, Ed25519Signer, StaticVerifierResolver,
    UNAUTHENTICATED,
};

/// A real key that puts another key's `kid` in its header.
struct KidLiar {
    key: Ed25519Signer,
    kid: [u8; 8],
}

#[async_trait::async_trait]
impl CoseSigner for KidLiar {
    fn alg(&self) -> CoseAlg {
        CoseAlg::Ed25519
    }
    fn kid(&self) -> &[u8] {
        &self.kid
    }
    async fn sign(&self, tbs: &[u8]) -> Result<Vec<u8>, CratestackError> {
        self.key.sign(tbs).await
    }
}

#[tokio::test]
async fn a_key_cannot_sign_under_another_keys_kid() {
    let now = common::now();
    let victim = common::ed25519().verify_key();
    let attacker = Ed25519Signer::from_seed(&common::OTHER_ED25519_SEED);
    // A resolver that over-returns: both keys, whatever the kid.
    let resolver = Arc::new(FixedResolver(vec![victim.clone(), attacker.verify_key()]));
    let server = common::server_with(
        CoseAlg::Ed25519,
        now,
        resolver,
        Arc::new(InMemoryNonceStore::new()),
    );
    let liar = CoseEnvelope::client(
        CoseMode::Sign1,
        Arc::new(KidLiar {
            key: attacker,
            kid: victim.kid(),
        }),
        Arc::new(StaticVerifierResolver::new()),
    )
    .clock(move || i64::try_from(now).expect("fits"))
    .build()
    .expect("client");
    let sealed = liar
        .seal_request(&common::fixture::payment_bytes(), &rpc_request())
        .await
        .expect("seal");

    let mut ctx = CratestackContext::anonymous();
    let result = CratestackEnvelope::open(&server, sealed, &rpc_request(), &mut ctx).await;
    match result {
        Err(CratestackError::Unauthorized(message)) => assert_eq!(message, UNAUTHENTICATED),
        other => panic!("an attacker-signed message under the victim's kid: {other:?}"),
    }
    assert!(ctx.verified_signer().is_none(), "no signer on failure");
}

/// Through the trait, the context names the verifying key: its full
/// thumbprint, its algorithm, and the header `kid`, which is that key's.
#[tokio::test]
async fn the_context_records_the_verifying_keys_thumbprint_and_alg() {
    let now = common::now();
    for &alg in CoseAlg::ALL {
        let client = common::client(alg, now, CTI_16);
        let server = common::server(alg, now);
        let sealed = client
            .seal(
                bytes::Bytes::from(common::fixture::payment_bytes()),
                &rest_request(),
            )
            .await
            .expect("seal");
        let mut ctx = CratestackContext::anonymous();
        server
            .open(sealed, &rest_request(), &mut ctx)
            .await
            .expect("open");
        let signer = ctx.verified_signer().expect("recorded");
        let key = match alg {
            CoseAlg::Ed25519 => common::ed25519().verify_key(),
            CoseAlg::Esp256 => common::p256().verify_key(),
            _ => common::hmac(alg).verify_key(),
        };
        assert_eq!(signer.thumbprint(), &key.thumbprint(), "{alg:?}");
        assert_eq!(signer.kid(), key.kid().as_slice(), "{alg:?}");
        assert_eq!(signer.alg(), alg.id(), "{alg:?}");
    }
}
