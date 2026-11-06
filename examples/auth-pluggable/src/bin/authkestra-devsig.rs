//! The service with Authkestra device signatures. `AUTH_PLUGGABLE_ISSUER` is
//! the attestation issuer, `AUTH_PLUGGABLE_ISSUER_JWKS` a file holding its
//! public JWKS, and `AUTH_PLUGGABLE_AUDIENCE` the `aud` each signature names.

use std::sync::Arc;
use std::time::Duration;

use auth_pluggable::providers::devsig::Provider;
use authkestra_devsig::{InMemoryReplayStore, IssuerJwks, VerifierConfig};
use jsonwebtoken::Algorithm;
use jsonwebtoken::jwk::JwkSet;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let issuer = env("AUTH_PLUGGABLE_ISSUER");
    let set: JwkSet =
        serde_json::from_str(&std::fs::read_to_string(env("AUTH_PLUGGABLE_ISSUER_JWKS"))?)?;
    let jwks = IssuerJwks::new();
    for key in set.keys {
        let kid = key.common.key_id.clone().unwrap_or_default();
        jwks.insert(issuer.clone(), kid, key).await;
    }
    let algs = [Algorithm::ES256, Algorithm::RS256, Algorithm::EdDSA];
    let config = VerifierConfig::new(
        [issuer],
        algs,
        Duration::from_secs(30),
        Duration::from_secs(300),
        env("AUTH_PLUGGABLE_AUDIENCE"),
    );
    let provider = Provider {
        config: Arc::new(config),
        jwks: Arc::new(jwks),
        replay: Arc::new(InMemoryReplayStore::new()),
    };
    auth_pluggable::serve(provider, "127.0.0.1:8080").await
}
