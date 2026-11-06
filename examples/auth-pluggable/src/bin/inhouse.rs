//! The service with CrateStack's own authentication. Service keys come from
//! `CRATESTACK_AUTH_SIGNATURE_TRUSTED_KEYS` (`cratestack-auth`'s format);
//! `AUTH_PLUGGABLE_ADMINS` is a comma-separated list of the key ids that get
//! the `admin` role. COSE signers authenticate when the COSE envelope layer
//! runs in front of the router (cratestack-docs, "Signed transport").

use std::sync::Arc;

use auth_pluggable::no_devices::NoDevices;
use auth_pluggable::providers::inhouse;
use cratestack_auth::SignedRequestVerifier;
use cratestack_cose::RegistryVerifierResolver;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let signed = SignedRequestVerifier::from_env().map_err(std::io::Error::other)?;
    let admins = std::env::var("AUTH_PLUGGABLE_ADMINS").unwrap_or_default();
    let admins = admins.split(',').map(str::to_owned).collect();
    let services = Arc::new(RegistryVerifierResolver::new());
    let provider = inhouse::provider(signed, admins, Arc::new(NoDevices), services);
    auth_pluggable::serve(provider, "127.0.0.1:8080").await
}
