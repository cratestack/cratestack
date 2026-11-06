//! The service with Authkestra OIDC and DPoP. `AUTH_PLUGGABLE_ISSUER` is the
//! OP, `AUTH_PLUGGABLE_JWKS_URL` its JWKS, `AUTH_PLUGGABLE_AUDIENCE` the
//! token audience, and `AUTH_PLUGGABLE_ORIGIN` this service's origin, which
//! every DPoP proof's `htu` starts with.

use std::sync::Arc;

use auth_pluggable::providers::oidc::Provider;
use authkestra_engine::store::memory::MemoryStore;
use authkestra_resource::DpopJtiRecord;
use authkestra_resource::jwt::{JwtStrategy, ValidationConfig};
use jsonwebtoken::Algorithm;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = ValidationConfig::builder()
        .jwks_url(env("AUTH_PLUGGABLE_JWKS_URL"))
        .issuer(env("AUTH_PLUGGABLE_ISSUER"))
        .audience(env("AUTH_PLUGGABLE_AUDIENCE"))
        .algorithms(vec![Algorithm::EdDSA, Algorithm::ES256, Algorithm::RS256])
        .require_dpop(true)
        .dpop_resource_origin(env("AUTH_PLUGGABLE_ORIGIN"))
        .build();
    let replay = Arc::new(MemoryStore::<DpopJtiRecord>::new());
    let strategy = JwtStrategy::new(config).with_dpop_replay_store(replay);
    auth_pluggable::serve(Provider(Arc::new(strategy)), "127.0.0.1:8080").await
}
