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
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!("DPoP {token}")).expect("ASCII"),
        );
        headers.insert("dpop", HeaderValue::from_str(&proof).expect("ASCII"));
        headers.into()
    }
    fn provider_with(&self, require_dpop: bool) -> Provider {
        let config = ValidationConfig::builder()
            .jwks_url(self.jwks_url.clone())
            .issuer(ISSUER)
            .audience(CLIENT)
            .algorithms(vec![Algorithm::EdDSA])
            .require_dpop(require_dpop)
            .dpop_resource_origin(ORIGIN)
            .build();
        let replay = Arc::new(MemoryStore::<DpopJtiRecord>::new());
        Provider(Arc::new(
            JwtStrategy::new(config).with_dpop_replay_store(replay),
        ))
    }
}

impl Fixture for Oidc {
    type Provider = Provider;

    fn provider(&self) -> Provider {
        self.provider_with(true)
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

/// The token's authentication facts reach the principal contract: top-level
/// claims override raw `identity.attributes`, and attributes act as fallback.
#[tokio::test]
async fn amr_acr_and_the_key_binding_reach_the_principal() {
    use authkestra_engine::Identity;
    use cratestack::{AuthProvider, CratestackContext, RequestContext};
    use serde_json::json;
    use std::collections::HashMap;

    async fn eval(op: &Op, key: &DpopKey, token: String, dpop: bool) -> CratestackContext {
        let fixture = Oidc {
            jwks_url: op.jwks_url.clone(),
            user: (token.clone(), key.clone()),
            admin: (token, key.clone()),
            attacker: key.clone(),
        };
        let canonical = support::procedure_call("/$procs/whoAmI");
        let cred = fixture.sign(Who::User, &canonical);
        let req = RequestContext {
            method: canonical.method,
            path: canonical.path,
            query: None,
            headers: &cred.headers,
            body: &canonical.body,
            extensions: &cred.extensions,
        };
        let provider = fixture.provider_with(dpop);
        provider.authenticate(&req).await.unwrap()
    }

    let op = Op::start().await;
    let key = DpopKey(SigningKey::from_bytes(&[0x04; 32]));
    let attrs = [
        ("amr", "password"),
        ("acr", "aal2"),
        ("auth_time", "1700000123"),
    ];
    let ctx = eval(&op, &key, op.issue("usr-ada", &attrs, &key).await, true).await;
    assert_eq!(ctx.amr(), Some(vec!["pwd"]));
    assert_eq!(ctx.acr(), Some("urn:authkestra:acr:single-factor"));
    assert_eq!(ctx.auth_time(), Some(1_700_000_123));
    let jkt = ctx.cnf_jkt().expect("cnf.jkt thumbprint").to_owned();
    assert!(!jkt.is_empty());

    let identity = Identity {
        provider_id: "op".into(),
        external_id: "usr-ada".into(),
        email: None,
        username: Some("usr-ada".into()),
        attributes: HashMap::from([
            ("amr".into(), "hwk user".into()),
            ("acr".into(), "aal2".into()),
            ("auth_time".into(), "1111111111".into()),
        ]),
    };
    let mint = |entries: &[(&str, serde_json::Value)]| {
        let mut extra = HashMap::from([("cnf".into(), json!({ "jkt": jkt }))]);
        extra.extend(entries.iter().map(|(k, v)| ((*k).into(), v.clone())));
        op.tokens
            .issue_user_token_with_extra(
                identity.clone(),
                600,
                Some("openid".into()),
                Some(CLIENT.into()),
                extra,
            )
            .expect("minted token")
    };
    let tok = mint(&[("auth_time", json!(2_222_222_222_i64))]);
    let override_ctx = eval(&op, &key, tok, true).await;
    assert_eq!(override_ctx.amr(), Some(vec!["hwk", "user"]));
    assert_eq!(override_ctx.acr(), Some("aal2"));
    assert_eq!(override_ctx.auth_time(), Some(2_222_222_222));

    let fallback_ctx = eval(&op, &key, mint(&[]), true).await;
    assert_eq!(fallback_ctx.auth_time(), Some(1_111_111_111));

    // Malformed or null top-level claims stay `None` (`require_dpop(false)` skips `CnfClaim`).
    for (dpop, amr, acr, auth_time) in [
        (true, json!("pwd"), json!(null), json!(null)),
        (false, json!("pwd"), json!(42), json!("not-an-int")),
        (false, json!(["pwd", 1]), json!(false), json!(1.5)),
    ] {
        let tok = mint(&[("amr", amr), ("acr", acr), ("auth_time", auth_time)]);
        let bad = eval(&op, &key, tok, dpop).await;
        assert_eq!((bad.amr(), bad.acr(), bad.auth_time()), (None, None, None));
    }
}
