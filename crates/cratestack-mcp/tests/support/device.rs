//! A provider that does not use `Authorization` at all, standing in for a
//! device-signature or COSE scheme (cratestack#1243): it reads `x-device`,
//! and refuses according to `x-refusal`, so one server covers every case.
//! It counts its calls, so a test can tell "refused before the provider
//! ran" from "refused by the provider".

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cratestack_core::{
    AuthProvider, AuthRefusal, CratestackContext, CratestackError, Credential, RequestContext,
    Value,
};

#[derive(Clone, Default)]
pub struct DeviceProvider {
    calls: Arc<AtomicUsize>,
}

impl DeviceProvider {
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

fn header<'a>(request: &'a RequestContext<'_>, name: &str) -> Option<&'a str> {
    request.headers.get(name).and_then(|v| v.to_str().ok())
}

impl AuthProvider for DeviceProvider {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match header(request, "x-refusal") {
            Some("renew") => {
                return Err(AuthRefusal::unauthenticated("attestation expired")
                    .with_member("error", Value::String("attestation_expired".into()))
                    .with_challenge(r#"Signature error="attestation_expired""#)
                    .into_error());
            }
            Some("revoked") => {
                return Err(AuthRefusal::forbidden("device revoked")
                    .with_member("error", Value::String("DEVICE_REVOKED".into()))
                    .with_details(Value::Map(
                        [("device".to_owned(), Value::String("d-1".into()))].into(),
                    ))
                    .with_challenge(r#"Signature error="device_revoked""#)
                    .into_error());
            }
            Some("plain") => {
                return Err(CratestackError::Unauthorized(
                    "bad signature from secret-device-key".to_owned(),
                ));
            }
            _ => {}
        }
        Ok(match header(request, "x-device") {
            Some(device) => CratestackContext::authenticated([(
                "id".to_owned(),
                Value::String(device.to_owned()),
            )]),
            None => CratestackContext::anonymous(),
        })
    }

    /// Present with `x-device`, malformed when it is `malformed`, absent
    /// without it: what `CredentialPresence::ask_provider()` reads.
    fn credential<'r>(&self, request: &RequestContext<'r>) -> Credential<'r> {
        match request.headers.get("x-device").map(|v| v.to_str()) {
            None => Credential::Absent,
            Some(Ok("malformed")) | Some(Err(_)) => Credential::Malformed("an unreadable x-device"),
            Some(Ok(device)) => Credential::Present {
                redact: Some(device),
            },
        }
    }
}
