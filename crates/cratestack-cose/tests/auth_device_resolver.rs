//! `DeviceKeyCoseResolver`: a `cratestack_auth::DeviceKeyResolver` as the
//! COSE opener's key resolver (`auth` feature, cratestack#1005 part B).

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use common::{ED25519_SEED, OTHER_ED25519_SEED, rpc_request};
use cratestack_auth::{AuthError, DeviceKeyResolver};
use cratestack_core::{CratestackError, InMemoryNonceStore};
use cratestack_cose::auth::DeviceKeyCoseResolver;
use cratestack_cose::{
    CoseAlg, CoseEnvelope, CoseSigner, CoseVerifierResolver, Ed25519Signer, StaticVerifierResolver,
    UNAUTHENTICATED,
};
use ed25519_dalek::{SigningKey, VerifyingKey};

const BACKEND_DETAIL: &str = "device-db.internal:5432 refused";

/// A device registry that answers every prefix with `keys` (or fails), and
/// records what it was asked.
#[derive(Default)]
struct Devices {
    keys: Vec<VerifyingKey>,
    fail: bool,
    calls: AtomicUsize,
    prefixes: Mutex<Vec<Vec<u8>>>,
}

#[async_trait::async_trait]
impl DeviceKeyResolver for Devices {
    async fn lookup_device_verifying_key(
        &self,
        _key_id: &str,
    ) -> Result<Option<VerifyingKey>, AuthError> {
        panic!("the COSE adapter must resolve by thumbprint, never by keyId");
    }

    async fn lookup_device_verifying_keys_by_thumbprint(
        &self,
        kid_prefix: &[u8],
    ) -> Result<Vec<VerifyingKey>, AuthError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.prefixes
            .lock()
            .expect("lock")
            .push(kid_prefix.to_vec());
        if self.fail {
            return Err(AuthError::DeviceKeyLookup(BACKEND_DETAIL.to_owned()));
        }
        Ok(self.keys.clone())
    }
}

fn public(seed: &[u8; 32]) -> VerifyingKey {
    SigningKey::from_bytes(seed).verifying_key()
}

fn server(alg: CoseAlg, devices: Arc<Devices>, now: u64) -> CoseEnvelope {
    common::server_with(
        alg,
        now,
        Arc::new(DeviceKeyCoseResolver::new(devices)),
        Arc::new(InMemoryNonceStore::new()),
    )
}

/// Seal the payment fixture as a request from `signer`.
async fn sealed_by(signer: Arc<dyn CoseSigner>, now: u64) -> bytes::Bytes {
    let now = i64::try_from(now).expect("fits");
    CoseEnvelope::client(
        signer.alg().mode(),
        signer,
        Arc::new(StaticVerifierResolver::new()),
    )
    .clock(move || now)
    .build()
    .expect("client")
    .seal_request(&common::fixture::payment_bytes(), &rpc_request())
    .await
    .expect("seal")
}

fn assert_coarse_401(result: Result<cratestack_cose::Opened, CratestackError>) {
    match result {
        Err(CratestackError::Unauthorized(message)) => assert_eq!(message, UNAUTHENTICATED),
        other => panic!("expected the coarse 401, got {other:?}"),
    }
}

#[tokio::test]
async fn a_known_device_key_opens_by_its_thumbprint_prefix() {
    let now = common::now();
    let device = Ed25519Signer::from_seed(&ED25519_SEED);
    let devices = Arc::new(Devices {
        keys: vec![public(&ED25519_SEED)],
        ..Devices::default()
    });
    let sealed = sealed_by(Arc::new(device.clone()), now).await;

    let opened = server(CoseAlg::Ed25519, devices.clone(), now)
        .open_request(sealed, &rpc_request())
        .await
        .expect("opens");
    assert_eq!(opened.thumbprint, device.verify_key().thumbprint());
    assert_eq!(
        *devices.prefixes.lock().expect("lock"),
        vec![device.kid().to_vec()]
    );
}

#[tokio::test]
async fn an_unknown_or_revoked_device_is_the_coarse_401() {
    let now = common::now();
    let devices = Arc::new(Devices::default());
    let sealed = sealed_by(Arc::new(Ed25519Signer::from_seed(&ED25519_SEED)), now).await;

    assert_coarse_401(
        server(CoseAlg::Ed25519, devices.clone(), now)
            .open_request(sealed, &rpc_request())
            .await,
    );
    assert_eq!(devices.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_backend_error_is_a_500_that_keeps_its_detail_server_side() {
    let now = common::now();
    let devices = Arc::new(Devices {
        fail: true,
        ..Devices::default()
    });
    let sealed = sealed_by(Arc::new(Ed25519Signer::from_seed(&ED25519_SEED)), now).await;

    let error = server(CoseAlg::Ed25519, devices, now)
        .open_request(sealed, &rpc_request())
        .await
        .expect_err("a failing registry never opens");
    assert!(matches!(error, CratestackError::Internal(_)), "{error:?}");
    assert_eq!(error.status_code(), 500);
    assert!(!error.public_message().contains("device-db"), "{error:?}");
    assert!(
        format!("{error:?}").contains(BACKEND_DETAIL),
        "kept for the log"
    );
}

/// Device keys are Ed25519. Any other algorithm gets no candidate, and the
/// registry is not even asked.
#[tokio::test]
async fn only_ed25519_is_resolved_for_devices() {
    let devices = Arc::new(Devices {
        keys: vec![public(&ED25519_SEED)],
        ..Devices::default()
    });
    let resolver = DeviceKeyCoseResolver::new(devices.clone());
    let kid = Ed25519Signer::from_seed(&ED25519_SEED).kid().to_vec();
    for &alg in CoseAlg::ALL {
        let keys = resolver.resolve(&kid, alg).await.expect("resolve");
        let expected = usize::from(alg == CoseAlg::Ed25519);
        assert_eq!(keys.len(), expected, "{alg:?}");
    }
    assert_eq!(devices.calls.load(Ordering::SeqCst), 1, "Ed25519 only");

    // End to end: an ESP256 request to a device-key server is the 401,
    // without a registry lookup.
    let now = common::now();
    let before = devices.calls.load(Ordering::SeqCst);
    let sealed = sealed_by(Arc::new(common::p256()), now).await;
    assert_coarse_401(
        server(CoseAlg::Esp256, devices.clone(), now)
            .open_request(sealed, &rpc_request())
            .await,
    );
    assert_eq!(devices.calls.load(Ordering::SeqCst), before);
}

/// A real device key that writes another device's `kid` into its header.
struct KidLiar {
    key: Ed25519Signer,
    kid: Vec<u8>,
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

/// A registry that returns more than the exact match (both devices, for
/// any prefix) cannot let one device sign as the other: the opener's
/// per-candidate `kid` check still applies. Each device's honest request
/// opens as itself.
#[tokio::test]
async fn extra_keys_from_the_registry_cannot_sign_as_another_device() {
    let now = common::now();
    let victim = Ed25519Signer::from_seed(&ED25519_SEED);
    let attacker = Ed25519Signer::from_seed(&OTHER_ED25519_SEED);
    let devices = Arc::new(Devices {
        keys: vec![public(&OTHER_ED25519_SEED), public(&ED25519_SEED)],
        ..Devices::default()
    });
    let server = server(CoseAlg::Ed25519, devices, now);

    let forged = sealed_by(
        Arc::new(KidLiar {
            key: attacker.clone(),
            kid: victim.kid().to_vec(),
        }),
        now,
    )
    .await;
    assert_coarse_401(server.open_request(forged, &rpc_request()).await);

    for device in [&victim, &attacker] {
        let sealed = sealed_by(Arc::new(device.clone()), now).await;
        let opened = server
            .open_request(sealed, &rpc_request())
            .await
            .expect("opens");
        assert_eq!(opened.thumbprint, device.verify_key().thumbprint());
    }
}
