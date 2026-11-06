//! Authkestra device signatures (`authkestra-devsig`): an `X-Signature` per
//! request, signed by the device, and an `X-Attestation` from the issuer
//! binding that device key to an identity.

use std::sync::Arc;

use authkestra_devsig::{IssuerJwks, ReplayStore, SignedRequest, VerifierConfig, VerifyError};
use cratestack::{
    AuthProvider, AuthRefusal, CratestackContext, CratestackError, Credential, RequestContext,
    Value,
};

/// The device-signature provider.
#[derive(Clone)]
pub struct Provider {
    pub config: Arc<VerifierConfig>,
    pub jwks: Arc<IssuerJwks>,
    pub replay: Arc<dyn ReplayStore>,
}

fn header<'r>(request: &RequestContext<'r>, name: &str) -> Option<&'r str> {
    request.headers.get(name)?.to_str().ok()
}

impl AuthProvider for Provider {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        if self.credential(request).is_absent() {
            return Ok(CratestackContext::anonymous());
        }
        let signed = SignedRequest {
            signature: header(request, "x-signature"),
            attestation: header(request, "x-attestation"),
            method: request.method,
            path: request.path,
            query: request.query,
            body: (!request.body.is_empty()).then_some(request.body),
        };
        let device = authkestra_devsig::verify(&signed, &self.config, &self.jwks, &*self.replay)
            .await
            .map_err(refusal)?;
        let roles = device.attributes["roles"].as_array().into_iter().flatten();
        Ok(CratestackContext::builder(device.subject)
            .roles(roles.filter_map(|role| role.as_str()))
            .cnf_jkt(device.key_thumbprint)
            .claim("device", Value::String(device.device))
            .build())
    }

    fn credential<'r>(&self, request: &RequestContext<'r>) -> Credential<'r> {
        match (
            header(request, "x-signature"),
            header(request, "x-attestation"),
        ) {
            (None, None) => Credential::Absent,
            _ => Credential::Present { redact: None },
        }
    }
}

/// `401 {"error": "<code>"}`: the code a device-signature client sorts
/// refusals by (renew on `attestation_expired`, sign out on `key_not_bound`).
pub fn refusal(error: VerifyError) -> CratestackError {
    AuthRefusal::unauthenticated("device signature refused")
        .with_member("error", Value::String(error.code().to_owned()))
        .into_error()
}
