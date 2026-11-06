//! An in-process `authkestra-op` (cratestack#1245 story 3): it issues
//! DPoP-bound access tokens through the `authorization_code` grant (PKCE
//! S256), and serves its JWKS on 127.0.0.1, where `JwtStrategy` fetches it.

use std::collections::HashMap;

use authkestra_engine::Identity;
use authkestra_engine::store::KvStore;
use authkestra_engine::store::memory::MemoryStore;
use authkestra_engine::token::TokenManager;
use authkestra_op::client::{ClientRegistration, GrantType};
use authkestra_op::code::{AuthorizationCode, AuthorizationCodeStore};
use authkestra_op::handlers::JwksResponse;
use authkestra_op::handlers::token::{TokenRequest, handle_token_with_client_cert};
use authkestra_op::{DpopJtiRecord, OpConfig};
use cratestack::axum::routing::get;
use cratestack::axum::{Json, Router};
use ed25519_dalek_v2::pkcs8::EncodePrivateKey;
use ed25519_dalek_v2::{Signer, SigningKey};
use serde_json::json;
use sha2::{Digest, Sha256};

use super::jws::{b64, compact, now};

pub const ISSUER: &str = "https://op.example.test";
pub const CLIENT: &str = "auth-pluggable";
const REDIRECT: &str = "https://app.example.test/callback";
const VERIFIER: &str = "a-pkce-verifier-long-enough-for-the-op-to-accept-it";

/// A client's DPoP key (RFC 9449).
#[derive(Clone)]
pub struct DpopKey(pub SigningKey);

impl DpopKey {
    /// A DPoP proof for `htm` / `htu`, bound to `token` (`ath`) when given.
    pub fn proof(&self, htm: &str, htu: &str, token: Option<&str>) -> String {
        static JTI: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let jti = JTI.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let jwk =
            json!({ "kty": "OKP", "crv": "Ed25519", "x": b64(self.0.verifying_key().as_bytes()) });
        let header = json!({ "typ": "dpop+jwt", "alg": "EdDSA", "jwk": jwk });
        let mut claims =
            json!({ "htm": htm, "htu": htu, "iat": now(), "jti": format!("dpop-{jti}") });
        if let Some(token) = token {
            claims["ath"] = json!(b64(&Sha256::digest(token.as_bytes())));
        }
        compact(&header, &claims, |input| {
            b64(&self.0.sign(input).to_bytes())
        })
    }
}

/// The OP: its signing key, its configuration, and its JWKS URL.
pub struct Op {
    tokens: TokenManager,
    config: OpConfig,
    pub jwks_url: String,
}

impl Op {
    /// Starts the OP's JWKS endpoint and returns the OP.
    pub async fn start() -> Self {
        let key = SigningKey::from_bytes(&[0x5a; 32]);
        let pem = key.to_pkcs8_pem(Default::default()).expect("a PKCS#8 PEM");
        let tokens =
            TokenManager::new_ed25519(pem.as_bytes(), Some(ISSUER.into()), Some("op-1".into()))
                .expect("an Ed25519 token manager");
        let jwks = JwksResponse::new(tokens.public_jwk());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port");
        let jwks_url = format!(
            "http://{}/jwks.json",
            listener.local_addr().expect("an address")
        );
        let app = Router::new().route("/jwks.json", get(move || async move { Json(jwks) }));
        tokio::spawn(async move { cratestack::axum::serve(listener, app).await });
        let config = OpConfig {
            issuer: ISSUER.to_owned(),
            scopes_supported: vec!["openid".to_owned()],
            response_types_supported: vec!["code".to_owned()],
            grant_types_supported: vec!["authorization_code".to_owned()],
            id_token_signing_alg: "EdDSA".to_owned(),
            authorization_code_ttl_secs: 60,
            access_token_ttl_secs: 600,
            device_code_ttl_secs: 600,
            token_exchange_enabled: false,
        };
        Self {
            tokens,
            config,
            jwks_url,
        }
    }

    /// Runs the `authorization_code` grant for `user` (the login itself is
    /// the OP's job; the test hands it the authenticated identity) and
    /// returns an access token bound to `dpop`.
    #[allow(deprecated)] // `require_pkce`: PKCE is mandatory anyway.
    pub async fn issue(&self, user: &str, attributes: &[(&str, &str)], dpop: &DpopKey) -> String {
        let clients = MemoryStore::<ClientRegistration>::new();
        let client = ClientRegistration {
            client_id: CLIENT.to_owned(),
            client_secret_hash: None,
            redirect_uris: vec![REDIRECT.to_owned()],
            grant_types: vec![GrantType::AuthorizationCode],
            scopes: vec!["openid".to_owned()],
            require_pkce: true,
            allowed_audiences: vec![],
            token_endpoint_auth_method: None,
            jwks: None,
        };
        clients
            .set(CLIENT, client, std::time::Duration::from_secs(600))
            .await
            .expect("a client");
        let identity = Identity {
            provider_id: "op".to_owned(),
            external_id: user.to_owned(),
            email: None,
            username: Some(user.to_owned()),
            attributes: attributes
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<HashMap<_, _>>(),
        };
        let mut codes = MemoryStore::<AuthorizationCode>::new();
        let mut code = AuthorizationCode::new(
            "code-1".to_owned(),
            CLIENT.to_owned(),
            REDIRECT.to_owned(),
            "openid".to_owned(),
            identity,
            chrono::Utc::now() + chrono::Duration::minutes(1),
            false,
        );
        code.code_challenge = Some(b64(&Sha256::digest(VERIFIER.as_bytes())));
        code.code_challenge_method = Some("S256".to_owned());
        codes.store_code(code).await.expect("a code");
        let mut store = authkestra_op::store::CompositeOpStore::new(
            clients,
            codes,
            MemoryStore::<authkestra_op::refresh::RefreshToken>::new(),
            MemoryStore::<authkestra_op::device::DeviceCodeSession>::new(),
        )
        .with_dpop_replay_store(MemoryStore::<DpopJtiRecord>::new());
        let mut request = TokenRequest::new("authorization_code".to_owned());
        request.code = Some("code-1".to_owned());
        request.redirect_uri = Some(REDIRECT.to_owned());
        request.client_id = Some(CLIENT.to_owned());
        request.code_verifier = Some(VERIFIER.to_owned());
        let proof = dpop.proof("POST", &self.config.token_endpoint(), None);
        let response = handle_token_with_client_cert(
            request,
            None,
            &self.config,
            &mut store,
            &self.tokens,
            None,
            Some(&proof),
        )
        .await
        .unwrap_or_else(|error| panic!("the OP refused the grant: {error:?}"));
        assert_eq!(response.token_type, "DPoP");
        response.access_token
    }
}
