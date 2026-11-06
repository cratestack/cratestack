//! The in-house provider through the shared suite: signed requests with two
//! service keys, one of them an admin, then the COSE half of the chain
//! through the conformance kit.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use auth_pluggable::no_devices::NoDevices;
use auth_pluggable::providers::inhouse;
use chrono::{SecondsFormat, Utc};
use cratestack::VerifiedSigner;
use cratestack_auth::{
    SignRequestParams, SignedRequestVerifier, content_sha256_base64url, sign_request,
};
use cratestack_auth_conformance::{Canonical, Credentials, Expected, auth_conformance};
use cratestack_cose::{CoseVerifierResolver, Ed25519Signer, RegistryVerifierResolver};
use ed25519_dalek::SigningKey;
use http::{HeaderMap, HeaderValue, Method};
use support::{Fixture, Who, suite};

const USER: &str = "app-user";
const ADMIN: &str = "app-admin";
const ED25519: i64 = -19;

#[derive(Clone)]
struct InHouse {
    user: SigningKey,
    admin: SigningKey,
    services: Arc<RegistryVerifierResolver>,
}

impl InHouse {
    fn new() -> Self {
        Self {
            user: SigningKey::from_bytes(&[0x11; 32]),
            admin: SigningKey::from_bytes(&[0x22; 32]),
            services: Arc::new(RegistryVerifierResolver::new()),
        }
    }

    fn signed(
        &self,
        key_id: &str,
        key: &SigningKey,
        canonical: &Canonical,
        path: &str,
    ) -> Credentials {
        static NONCE: AtomicU64 = AtomicU64::new(0);
        let nonce = format!("n-{}", NONCE.fetch_add(1, Ordering::Relaxed));
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let method = Method::from_bytes(canonical.method.as_bytes()).expect("a method");
        let signature = sign_request(SignRequestParams {
            signing_key: key,
            method: &method,
            path,
            query: canonical.query,
            body: &canonical.body,
            timestamp: &timestamp,
            nonce: &nonce,
            key_id,
        });
        let value = format!(
            "Signature keyId=\"{key_id}\", timestamp=\"{timestamp}\", nonce=\"{nonce}\", \
             signature=\"{signature}\", alg=\"Ed25519\", content_sha256=\"{}\"",
            content_sha256_base64url(&canonical.body),
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_str(&value).expect("ASCII"),
        );
        headers.into()
    }
}

impl Fixture for InHouse {
    type Provider = inhouse::Provider;

    fn provider(&self) -> Self::Provider {
        let verifier = SignedRequestVerifier::new([
            (USER.to_owned(), self.user.verifying_key()),
            (ADMIN.to_owned(), self.admin.verifying_key()),
        ]);
        let services = self.services.clone() as Arc<dyn CoseVerifierResolver>;
        inhouse::provider(
            verifier,
            [ADMIN.to_owned()].into(),
            Arc::new(NoDevices),
            services,
        )
    }

    fn subject(&self, who: Who) -> String {
        match who {
            Who::User => format!("svc:{USER}"),
            Who::Admin => format!("svc:{ADMIN}"),
        }
    }

    fn sign(&self, who: Who, canonical: &Canonical) -> Credentials {
        match who {
            Who::User => self.signed(USER, &self.user, canonical, canonical.path),
            Who::Admin => self.signed(ADMIN, &self.admin, canonical, canonical.path),
        }
    }

    /// Signed over the URL with an extra prefix: not the canonical request.
    fn refused(&self, canonical: &Canonical) -> Credentials {
        let path = format!("/prefixed{}", canonical.path);
        self.signed(USER, &self.user, canonical, &path)
    }

    fn refusal(&self) -> Expected {
        Expected::unauthorized(Some("signature verification failed"))
    }
}

#[tokio::test]
async fn the_in_house_provider_passes_the_suite() {
    suite(InHouse::new()).await;
}

/// The chain's second member: a COSE service signer, recorded in the
/// request's extensions as the envelope layer would.
#[tokio::test]
async fn its_cose_half_passes_the_kit() {
    let fixture = InHouse::new();
    let service = Ed25519Signer::from_seed(&[0x33; 32]).verify_key();
    let kid = fixture
        .services
        .register(service.clone())
        .expect("register");
    let signer = VerifiedSigner::new(kid.to_vec(), service.thumbprint(), ED25519);
    let subject = format!("cose:{}", hex(&service.thumbprint()));
    auth_conformance(fixture.provider())
        .accepts(subject, move |_: &_| {
            Credentials::default().with_extension(signer.clone())
        })
        .run()
        .await;
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
