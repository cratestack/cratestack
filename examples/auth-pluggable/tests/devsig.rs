//! The Authkestra device-signature provider through the shared suite. The
//! test issuer and devices mint exactly what `authkestra-devsig` verifies:
//! an issuer-signed `X-Attestation` binding the device key's thumbprint,
//! and a device-signed `X-Signature` over `mth`, `pth`, `aud` and the
//! `qsh` / `bdh` hashes of the canonical request.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use auth_pluggable::providers::devsig::Provider;
use authkestra_devsig::{InMemoryReplayStore, IssuerJwks, VerifierConfig};
use cratestack::{AuthRefusal, Value as CsValue};
use cratestack_auth_conformance::{Canonical, Credentials, Expected};
use http::{HeaderMap, HeaderValue};
use jsonwebtoken::jwk::{Jwk, ThumbprintHash};
use jsonwebtoken::{Algorithm, EncodingKey, crypto};
use serde_json::{Value, json};
use support::jws::{compact, hex_sha256, now};
use support::{Fixture, Who, suite};

const ISSUER: &str = "https://id.example.test";
const AUD: &str = "api.example.test";

/// An ES256 key: the issuer's, or a device's.
#[derive(Clone)]
struct Key {
    encoding: EncodingKey,
    jwk: Jwk,
}

impl Key {
    fn generate(kid: Option<&str>) -> Self {
        use p256::pkcs8::EncodePrivateKey;
        let secret = p256::SecretKey::random(&mut rand::rngs::OsRng);
        let pem = secret
            .to_pkcs8_pem(p256::pkcs8::LineEnding::LF)
            .expect("PEM");
        let encoding = EncodingKey::from_ec_pem(pem.as_bytes()).expect("an EC key");
        let mut jwk = Jwk::from_encoding_key(&encoding, Algorithm::ES256).expect("a JWK");
        jwk.common.key_id = kid.map(str::to_owned);
        Self { encoding, jwk }
    }

    fn thumbprint(&self) -> String {
        self.jwk
            .thumbprint(ThumbprintHash::SHA256)
            .expect("a thumbprint")
    }

    fn sign(&self, header: Value, claims: Value) -> String {
        compact(&header, &claims, |input| {
            crypto::sign(input, &self.encoding, Algorithm::ES256).expect("signed")
        })
    }
}

#[derive(Clone)]
struct Devsig {
    issuer: Key,
    jwks: Arc<IssuerJwks>,
    user: Key,
    admin: Key,
    attacker: Key,
}

impl Devsig {
    fn attestation(&self, sub: &str, device: &Key, roles: &[&str]) -> String {
        let header = json!({ "alg": "ES256", "kid": "issuer-1", "typ": "webank-attest+jws" });
        let claims = json!({
            "iss": ISSUER, "sub": sub, "cnf": { "jkt": device.thumbprint() }, "did": format!("dev-{sub}"),
            "iat": now(), "exp": now() + 3600, "att": { "status": "active", "roles": roles },
        });
        self.issuer.sign(header, claims)
    }

    fn signature(&self, device: &Key, canonical: &Canonical) -> String {
        static JTI: AtomicU64 = AtomicU64::new(0);
        let header = json!({ "alg": "ES256", "typ": "webank-devsig+jws", "jwk": device.jwk });
        let mut claims = json!({
            "iat": now(), "exp": now() + 60, "jti": format!("jti-{}", JTI.fetch_add(1, Ordering::Relaxed)),
            "mth": canonical.method, "pth": canonical.path, "aud": AUD,
        });
        if let Some(query) = canonical.query {
            claims["qsh"] = json!(hex_sha256(query.as_bytes()));
        }
        if !canonical.body.is_empty() {
            claims["bdh"] = json!(hex_sha256(&canonical.body));
        }
        device.sign(header, claims)
    }

    fn headers(attestation: String, signature: String) -> Credentials {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-attestation",
            HeaderValue::from_str(&attestation).expect("ASCII"),
        );
        headers.insert(
            "x-signature",
            HeaderValue::from_str(&signature).expect("ASCII"),
        );
        headers.into()
    }
}

impl Fixture for Devsig {
    type Provider = Provider;

    fn provider(&self) -> Provider {
        let algs = [Algorithm::ES256];
        let config = VerifierConfig::new(
            [ISSUER],
            algs,
            Duration::from_secs(30),
            Duration::from_secs(120),
            AUD,
        );
        Provider {
            config: Arc::new(config),
            jwks: self.jwks.clone(),
            replay: Arc::new(InMemoryReplayStore::new()),
        }
    }

    fn subject(&self, who: Who) -> String {
        match who {
            Who::User => "usr-ada".to_owned(),
            Who::Admin => "usr-grace".to_owned(),
        }
    }

    fn sign(&self, who: Who, canonical: &Canonical) -> Credentials {
        let (device, roles): (&Key, &[&str]) = match who {
            Who::User => (&self.user, &["customer"]),
            Who::Admin => (&self.admin, &["customer", "admin"]),
        };
        let attestation = self.attestation(&self.subject(who), device, roles);
        Self::headers(attestation, self.signature(device, canonical))
    }

    /// The user's attestation, signed by an attacker's key it does not bind.
    fn refused(&self, canonical: &Canonical) -> Credentials {
        let attestation = self.attestation(&self.subject(Who::User), &self.user, &["customer"]);
        Self::headers(attestation, self.signature(&self.attacker, canonical))
    }

    fn refusal(&self) -> Expected {
        Expected::refusal(
            &AuthRefusal::unauthenticated("device signature refused")
                .with_member("error", CsValue::String("key_not_bound".to_owned())),
        )
    }
}

#[tokio::test]
async fn the_device_signature_provider_passes_the_suite() {
    let issuer = Key::generate(Some("issuer-1"));
    let jwks = IssuerJwks::new();
    jwks.insert(ISSUER, "issuer-1", issuer.jwk.clone()).await;
    suite(Devsig {
        issuer,
        jwks: Arc::new(jwks),
        user: Key::generate(None),
        admin: Key::generate(None),
        attacker: Key::generate(None),
    })
    .await;
}
