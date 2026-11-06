//! CrateStack's own authentication: a signed request (`cratestack-auth`),
//! then a COSE-verified signer (`cratestack-cose`), as one first-success
//! chain. `CoseAuthProvider` is that chain (ADR 0021 §5).

use std::collections::HashSet;
use std::sync::Arc;

use cratestack::CratestackContext;
use cratestack_auth::{
    DeviceKeyResolver, PrincipalMapper, SignedRequestAuthProvider, SignedRequestVerifier,
    TransportCallerMode, principal_to_cratestack_context,
};
use cratestack_cose::CoseVerifierResolver;
use cratestack_cose::auth::CoseAuthProvider;

/// The in-house provider.
pub type Provider = CoseAuthProvider<SignedRequestAuthProvider>;

/// A service key in `admins` gets the `admin` role, any other the `caller`
/// role; a request carrying a user's id token keeps the product mapping.
pub fn provider(
    signed: SignedRequestVerifier,
    admins: HashSet<String>,
    devices: Arc<dyn DeviceKeyResolver>,
    services: Arc<dyn CoseVerifierResolver>,
) -> Provider {
    let mapper = PrincipalMapper::custom(move |principal, role, allow_services| {
        if principal.user.is_some() || !allow_services {
            return principal_to_cratestack_context(principal, role, allow_services);
        }
        let key = &principal.transport.key_id;
        let role = if admins.contains(key) {
            "admin"
        } else {
            "caller"
        };
        Ok(CratestackContext::builder(format!("svc:{key}"))
            .roles([role])
            .build())
    });
    let signed = SignedRequestAuthProvider::new(signed)
        .allow_transport_callers(TransportCallerMode::AllMethods)
        .principal_mapper(mapper);
    CoseAuthProvider::new(signed, devices).with_service_keys(services)
}
