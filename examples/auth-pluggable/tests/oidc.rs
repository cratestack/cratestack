//! The Authkestra OIDC provider through the shared suite: `authkestra-op`
//! issues each caller a DPoP-bound access token, and every request carries
//! that token plus a fresh DPoP proof for exactly the canonical request.

mod support;

use std::sync::Arc;

use auth_pluggable::providers::oidc::Provider;
use authkestra_engine::store::memory::MemoryStore;
use authkestra_resource::DpopJtiRecord;
use authkestra_resource::jwt::{JwtStrategy, ValidationConfig};
use cratestack::{AuthRefusal, Value};
use cratestack_auth_conformance::{Canonical, Credentials, Expected};
use ed25519_dalek_v2::SigningKey;
use http::{HeaderMap, HeaderValue};
use jsonwebtoken::Algorithm;
use support::op::{CLIENT, DpopKey, ISSUER, Op};
use support::{Fixture, Who, suite};

/// This service's origin: every proof's `htu` is it plus the canonical path.
const ORIGIN: &str = "https://api.example.test";

#[derive(Clone)]
struct Oidc {
    jwks_url: String,
    user: (String, DpopKey),
    admin: (String, DpopKey),
    attacker: DpopKey,
}

impl Oidc {
    fn credentials(token: &str, key: &DpopKey, canonical: &Canonical) -> Credentials {
        let htu = format!("{ORIGIN}{}", canonical.path);
        let proof = key.proof(canonical.method, &htu, Some(token));
        let mut headers = HeaderMap::new();
        let authorization = format!("DPoP {token}");
        headers.insert(
            "authorization",
            HeaderValue::from_str(&authorization).expect("ASCII"),
        );
        headers.insert("dpop", HeaderValue::from_str(&proof).expect("ASCII"));
        headers.into()
    }
}

impl Fixture for Oidc {
    type Provider = Provider;

    fn provider(&self) -> Provider {
        let config = ValidationConfig::builder()
            .jwks_url(self.jwks_url.clone())
            .issuer(ISSUER)
            .audience(CLIENT)
            .algorithms(vec![Algorithm::EdDSA])
            .require_dpop(true)
            .dpop_resource_origin(ORIGIN)
            .build();
        let replay = Arc::new(MemoryStore::<DpopJtiRecord>::new());
        Provider(Arc::new(
            JwtStrategy::new(config).with_dpop_replay_store(replay),
        ))
    }

    fn subject(&self, who: Who) -> String {
        match who {
            Who::User => "usr-ada".to_owned(),
            Who::Admin => "usr-grace".to_owned(),
        }
    }

    fn sign(&self, who: Who, canonical: &Canonical) -> Credentials {
        let (token, key) = match who {
            Who::User => &self.user,
            Who::Admin => &self.admin,
        };
        Self::credentials(token, key, canonical)
    }

    /// The user's token, with a proof from a key it is not bound to.
    fn refused(&self, canonical: &Canonical) -> Credentials {
        Self::credentials(&self.user.0, &self.attacker, canonical)
    }

    fn refusal(&self) -> Expected {
        Expected::refusal(
            &AuthRefusal::unauthenticated("invalid DPoP-bound token")
                .with_member("error", Value::String("invalid_token".to_owned()))
                .with_challenge(r#"DPoP error="invalid_token", algs="EdDSA ES256""#),
        )
    }
}

#[tokio::test]
async fn the_oidc_provider_passes_the_suite() {
    let op = Op::start().await;
    let user = DpopKey(SigningKey::from_bytes(&[0x01; 32]));
    let admin = DpopKey(SigningKey::from_bytes(&[0x02; 32]));
    let customer = [("roles", "customer"), ("amr", "hwk user"), ("acr", "aal2")];
    let operator = [("roles", "customer admin"), ("amr", "pwd")];
    suite(Oidc {
        user: (op.issue("usr-ada", &customer, &user).await, user),
        admin: (op.issue("usr-grace", &operator, &admin).await, admin),
        attacker: DpopKey(SigningKey::from_bytes(&[0x03; 32])),
        jwks_url: op.jwks_url,
    })
    .await;
}

/// The token's authentication facts reach the principal contract.
#[tokio::test]
async fn amr_acr_and_the_key_binding_reach_the_principal() {
    use cratestack::{AuthProvider, RequestContext};

    let op = Op::start().await;
    let key = DpopKey(SigningKey::from_bytes(&[0x04; 32]));
    let token = op
        .issue("usr-ada", &[("amr", "hwk user"), ("acr", "aal2")], &key)
        .await;
    let fixture = Oidc {
        jwks_url: op.jwks_url,
        user: (token.clone(), key.clone()),
        admin: (token, key.clone()),
        attacker: key.clone(),
    };
    let canonical = support::procedure_call("/$procs/whoAmI");
    let credentials = fixture.sign(Who::User, &canonical);
    let request = RequestContext {
        method: canonical.method,
        path: canonical.path,
        query: None,
        headers: &credentials.headers,
        body: &canonical.body,
        extensions: &credentials.extensions,
    };
    let ctx = fixture
        .provider()
        .authenticate(&request)
        .await
        .expect("accepted");
    assert_eq!(ctx.amr(), Some(vec!["hwk", "user"]));
    assert_eq!(ctx.acr(), Some("aal2"));
    assert!(
        ctx.cnf_jkt().is_some_and(|jkt| !jkt.is_empty()),
        "cnf.jkt is the DPoP key's thumbprint"
    );
}
