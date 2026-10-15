//! `CoseAuthProvider` (cratestack#1077), rule by rule, against a fake device
//! registry (with a call counter and a revoke switch) and a real
//! `RegistryVerifierResolver` as the service registry. The same rules through
//! the envelope layer and a generated router are in cratestack-api's
//! `cose_auth_provider_{rest,rpc}.rs`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cratestack_auth::{AuthError, DeviceKeyResolver};
use cratestack_core::{
    AuthProvider, CratestackContext, CratestackError, RequestContext, Value, VerifiedSigner,
};
use cratestack_cose::auth::CoseAuthProvider;
use cratestack_cose::thumbprint::okp_ed25519_thumbprint;
use cratestack_cose::{CoseVerifierResolver, Ed25519Signer, RegistryVerifierResolver};
use ed25519_dalek::{SigningKey, VerifyingKey};
use http::{Extensions, HeaderMap, HeaderValue};

const ED25519: i64 = -19;
const ESP256: i64 = -9;

/// A device registry that answers with the keys it holds, whose thumbprint
/// prefix matches, and counts what it was asked.
#[derive(Default)]
struct Devices {
    keys: Mutex<Vec<VerifyingKey>>,
    fail: AtomicBool,
    calls: AtomicUsize,
}

impl Devices {
    fn holding(keys: &[VerifyingKey]) -> Arc<Self> {
        Arc::new(Self {
            keys: Mutex::new(keys.to_vec()),
            ..Self::default()
        })
    }

    fn revoke_all(&self) {
        self.keys.lock().expect("lock").clear();
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl DeviceKeyResolver for Devices {
    async fn lookup_device_verifying_key(
        &self,
        _key_id: &str,
    ) -> Result<Option<VerifyingKey>, AuthError> {
        Ok(None)
    }

    async fn lookup_device_verifying_keys_by_thumbprint(
        &self,
        kid_prefix: &[u8],
    ) -> Result<Vec<VerifyingKey>, AuthError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return Err(AuthError::DeviceKeyLookup(
                "device-db.internal:5432 refused".to_owned(),
            ));
        }
        Ok(self
            .keys
            .lock()
            .expect("lock")
            .iter()
            .filter(|key| &okp_ed25519_thumbprint(key.as_bytes())[..8] == kid_prefix)
            .cloned()
            .collect())
    }
}

/// The header-based provider the adapter wraps, in the three answers it can
/// give.
#[derive(Clone, Copy)]
enum Inner {
    Anonymous,
    Authenticated,
    Refuses,
}

impl AuthProvider for Inner {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        match self {
            Self::Anonymous => Ok(CratestackContext::anonymous()),
            Self::Authenticated => Ok(CratestackContext::authenticated([(
                "id".to_owned(),
                Value::String("bearer-user".to_owned()),
            )])),
            Self::Refuses => Err(CratestackError::Unauthorized("bad token".to_owned())),
        }
    }
}

fn device_key() -> VerifyingKey {
    SigningKey::from_bytes(&[0x11; 32]).verifying_key()
}

/// The verified signer the envelope layer would leave for `key`: the kid is
/// the thumbprint's prefix, the thumbprint is the key's.
fn signer_for(key: &VerifyingKey, alg: i64) -> VerifiedSigner {
    let thumbprint = okp_ed25519_thumbprint(key.as_bytes());
    VerifiedSigner::new(thumbprint[..8].to_vec(), thumbprint, alg)
}

async fn authenticate(
    provider: &CoseAuthProvider<Inner>,
    signer: Option<VerifiedSigner>,
    headers: &HeaderMap,
) -> Result<CratestackContext, CratestackError> {
    let mut extensions = Extensions::new();
    if let Some(signer) = signer {
        extensions.insert(signer);
    }
    let request = RequestContext {
        method: "POST",
        path: "/$procs/ping",
        query: None,
        headers,
        body: b"",
        extensions: &extensions,
    };
    provider.authenticate(&request).await
}

fn no_headers() -> HeaderMap {
    HeaderMap::new()
}

fn field(ctx: &CratestackContext, name: &str) -> Option<Value> {
    ctx.auth.as_ref()?.fields.get(name).cloned()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[tokio::test]
async fn a_registered_device_authenticates_as_its_thumbprint() {
    let key = device_key();
    let devices = Devices::holding(std::slice::from_ref(&key));
    let provider = CoseAuthProvider::new(Inner::Anonymous, devices.clone());

    let ctx = authenticate(&provider, Some(signer_for(&key, ED25519)), &no_headers())
        .await
        .expect("no error");

    assert!(ctx.is_authenticated());
    let thumbprint = hex(&okp_ed25519_thumbprint(key.as_bytes()));
    assert_eq!(
        field(&ctx, "id"),
        Some(Value::String(format!("cose:{thumbprint}")))
    );
    assert_eq!(
        field(&ctx, "kind"),
        Some(Value::String("device".to_owned()))
    );
    assert_eq!(devices.calls(), 1);
}

#[tokio::test]
async fn an_unknown_device_is_anonymous_not_an_error() {
    let devices = Devices::holding(&[]);
    let provider = CoseAuthProvider::new(Inner::Anonymous, devices.clone());

    let ctx = authenticate(
        &provider,
        Some(signer_for(&device_key(), ED25519)),
        &no_headers(),
    )
    .await
    .expect("an unknown key is not a backend failure");

    assert!(!ctx.is_authenticated());
    assert_eq!(devices.calls(), 1);
}

#[tokio::test]
async fn a_revoked_device_is_anonymous_on_the_next_request() {
    let key = device_key();
    let devices = Devices::holding(std::slice::from_ref(&key));
    let provider = CoseAuthProvider::new(Inner::Anonymous, devices.clone());
    let signer = signer_for(&key, ED25519);
    assert!(
        authenticate(&provider, Some(signer.clone()), &no_headers())
            .await
            .expect("ok")
            .is_authenticated()
    );

    devices.revoke_all();

    let ctx = authenticate(&provider, Some(signer), &no_headers())
        .await
        .expect("ok");
    assert!(
        !ctx.is_authenticated(),
        "a revoked key must not authenticate"
    );
}

#[tokio::test]
async fn without_a_verified_signer_nothing_is_looked_up_and_nothing_authenticates() {
    let key = device_key();
    let devices = Devices::holding(std::slice::from_ref(&key));
    let provider = CoseAuthProvider::new(Inner::Anonymous, devices.clone());

    let ctx = authenticate(&provider, None, &no_headers())
        .await
        .expect("ok");

    assert!(!ctx.is_authenticated());
    assert_eq!(devices.calls(), 0);
}

#[tokio::test]
async fn a_signer_is_never_read_from_headers() {
    // A forged signer on the wire: the real key's thumbprint in a header, in
    // hex, and a principal string. Neither is a `VerifiedSigner` extension.
    let key = device_key();
    let devices = Devices::holding(std::slice::from_ref(&key));
    let provider = CoseAuthProvider::new(Inner::Anonymous, devices.clone());
    let mut headers = HeaderMap::new();
    let thumbprint = hex(&okp_ed25519_thumbprint(key.as_bytes()));
    headers.insert(
        "cratestack-verified-signer",
        HeaderValue::from_str(&thumbprint).expect("ascii"),
    );
    headers.insert(
        "x-cose-principal",
        HeaderValue::from_str(&format!("cose:{thumbprint}")).expect("ascii"),
    );

    let ctx = authenticate(&provider, None, &headers).await.expect("ok");

    assert!(!ctx.is_authenticated(), "a header is not a verified signer");
    assert_eq!(devices.calls(), 0, "nothing forged reaches the registry");
}

#[tokio::test]
async fn a_signer_whose_kid_is_not_its_thumbprint_prefix_names_no_key() {
    let key = device_key();
    let devices = Devices::holding(std::slice::from_ref(&key));
    let provider = CoseAuthProvider::new(Inner::Anonymous, devices.clone());
    let honest = signer_for(&key, ED25519);
    let mismatched = VerifiedSigner::new(vec![0; 8], *honest.thumbprint(), ED25519);

    let ctx = authenticate(&provider, Some(mismatched), &no_headers())
        .await
        .expect("ok");

    assert!(!ctx.is_authenticated());
    assert_eq!(devices.calls(), 0);
}

#[tokio::test]
async fn a_non_ed25519_signer_is_not_looked_up_as_a_device() {
    let key = device_key();
    let devices = Devices::holding(std::slice::from_ref(&key));
    let provider = CoseAuthProvider::new(Inner::Anonymous, devices.clone());

    let ctx = authenticate(&provider, Some(signer_for(&key, ESP256)), &no_headers())
        .await
        .expect("ok");

    assert!(!ctx.is_authenticated());
    assert_eq!(devices.calls(), 0, "a device key is Ed25519 only");
}

#[tokio::test]
async fn an_unknown_algorithm_is_anonymous_without_a_lookup() {
    let key = device_key();
    let devices = Devices::holding(std::slice::from_ref(&key));
    let provider = CoseAuthProvider::new(Inner::Anonymous, devices.clone());

    let ctx = authenticate(&provider, Some(signer_for(&key, 9_999)), &no_headers())
        .await
        .expect("ok");

    assert!(!ctx.is_authenticated());
    assert_eq!(devices.calls(), 0);
}

#[tokio::test]
async fn a_backend_failure_is_an_error_not_anonymous() {
    let key = device_key();
    let devices = Devices::holding(std::slice::from_ref(&key));
    devices.fail.store(true, Ordering::SeqCst);
    let provider = CoseAuthProvider::new(Inner::Anonymous, devices);

    let error = authenticate(&provider, Some(signer_for(&key, ED25519)), &no_headers())
        .await
        .expect_err("an outage is not an unknown key");

    assert!(
        matches!(error, CratestackError::Internal(_)),
        "a backend failure is a 500: {error:?}"
    );
    assert!(!error.public_message().contains("device-db"));
}

#[tokio::test]
async fn an_authorization_identity_wins_and_the_registries_are_not_consulted() {
    let key = device_key();
    let devices = Devices::holding(std::slice::from_ref(&key));
    let provider = CoseAuthProvider::new(Inner::Authenticated, devices.clone());

    let ctx = authenticate(&provider, Some(signer_for(&key, ED25519)), &no_headers())
        .await
        .expect("ok");

    assert_eq!(
        field(&ctx, "id"),
        Some(Value::String("bearer-user".to_owned()))
    );
    assert_eq!(devices.calls(), 0);
}

#[tokio::test]
async fn a_refused_authorization_header_is_not_replaced_by_the_signer() {
    let key = device_key();
    let devices = Devices::holding(std::slice::from_ref(&key));
    let provider = CoseAuthProvider::new(Inner::Refuses, devices.clone());

    let error = authenticate(&provider, Some(signer_for(&key, ED25519)), &no_headers())
        .await
        .expect_err("a bad header is refused");

    assert!(matches!(error, CratestackError::Unauthorized(_)));
    assert_eq!(devices.calls(), 0);
}

#[tokio::test]
async fn a_service_key_authenticates_as_a_service_and_a_revoked_one_does_not() {
    let service = Ed25519Signer::from_seed(&[0x33; 32]).verify_key();
    let registry = Arc::new(RegistryVerifierResolver::new());
    let kid = registry.register(service.clone()).expect("register");
    let devices = Devices::holding(&[]);
    let provider = CoseAuthProvider::new(Inner::Anonymous, devices)
        .with_service_keys(registry.clone() as Arc<dyn CoseVerifierResolver>);
    let signer = VerifiedSigner::new(kid.to_vec(), service.thumbprint(), ED25519);

    let ctx = authenticate(&provider, Some(signer.clone()), &no_headers())
        .await
        .expect("ok");
    assert_eq!(
        field(&ctx, "kind"),
        Some(Value::String("service".to_owned()))
    );

    assert_eq!(registry.revoke(&kid), 1);
    let ctx = authenticate(&provider, Some(signer), &no_headers())
        .await
        .expect("ok");
    assert!(
        !ctx.is_authenticated(),
        "a revoked service key is anonymous"
    );
}
