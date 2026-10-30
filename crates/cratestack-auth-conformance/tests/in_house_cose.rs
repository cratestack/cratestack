//! cratestack#1243 story 9: `CoseAuthProvider`, CrateStack's own COSE
//! provider, now `first_success(inner).or(CoseSignerProvider)`, passes the
//! conformance kit. The verified signer is the extension the envelope layer
//! records in-process; the kit inserts it as that layer would.

use std::sync::Arc;

use cratestack::{AuthRefusal, CratestackContext, CratestackError, Value, VerifiedSigner};
use cratestack_auth::{AuthError, DeviceKeyResolver};
use cratestack_auth_conformance::{Credentials, Expected, auth_conformance};
use cratestack_cose::auth::CoseAuthProvider;
use cratestack_cose::{CoseVerifierResolver, Ed25519Signer, RegistryVerifierResolver};
use ed25519_dalek::VerifyingKey;
use http::{HeaderMap, HeaderValue};

const ED25519: i64 = -19;

/// No device is enrolled; device authentication stays off anyway.
struct NoDevices;

#[async_trait::async_trait]
impl DeviceKeyResolver for NoDevices {
    async fn lookup_device_verifying_key(
        &self,
        _key_id: &str,
    ) -> Result<Option<VerifyingKey>, AuthError> {
        Ok(None)
    }

    async fn lookup_device_verifying_keys_by_thumbprint(
        &self,
        _kid_prefix: &[u8],
    ) -> Result<Vec<VerifyingKey>, AuthError> {
        Ok(Vec::new())
    }
}

fn header_refusal() -> AuthRefusal {
    AuthRefusal::unauthenticated("token expired")
        .with_member("error", Value::String("invalid_token".into()))
        .with_challenge(r#"Bearer error="invalid_token""#)
}

/// The wrapped header provider: refuses an `Authorization` it is shown,
/// anonymous otherwise.
fn header_provider(headers: &HeaderMap) -> Result<CratestackContext, CratestackError> {
    if headers.contains_key("authorization") {
        Err(header_refusal().into_error())
    } else {
        Ok(CratestackContext::anonymous())
    }
}

#[tokio::test]
async fn cose_auth_provider_conforms() {
    let service = Ed25519Signer::from_seed(&[0x33; 32]).verify_key();
    let registry = Arc::new(RegistryVerifierResolver::new());
    let kid = registry.register(service.clone()).expect("register");
    let provider = CoseAuthProvider::new(header_provider, Arc::new(NoDevices))
        .with_service_keys(registry as Arc<dyn CoseVerifierResolver>);
    let signer = VerifiedSigner::new(kid.to_vec(), service.thumbprint(), ED25519);
    let subject = format!("cose:{}", hex(&service.thumbprint()));

    let signed = move |_: &_| Credentials::default().with_extension(signer.clone());
    let signed_with_a_refused_header = {
        let signed = signed.clone();
        move |canonical: &_| {
            let mut credentials = signed(canonical);
            credentials
                .headers
                .insert("authorization", HeaderValue::from_static("Bearer stale"));
            credentials
        }
    };

    auth_conformance(provider)
        .accepts(subject, signed)
        // A credential the header provider refuses is never rescued by the
        // signer: the chain fails closed.
        .refuses(
            "a refused header beside a valid signer",
            signed_with_a_refused_header,
            Expected::refusal(&header_refusal()),
        )
        .run()
        .await;
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
