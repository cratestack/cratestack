//! The kit on a provider that signs the canonical request: it passes, and a
//! provider that verifies against anything else fails.

use cratestack::{
    AuthProvider, AuthRefusal, CratestackContext, CratestackError, Credential, RequestContext,
    Value,
};
use cratestack_auth_conformance::{Canonical, Expected, auth_conformance};
use http::{HeaderMap, HeaderValue};

/// A stand-in device-signature provider: `x-signature` must be
/// `<method> <path> <body length>` of the request it is given, and
/// `x-refusal` picks a refusal.
#[derive(Clone)]
struct Signing {
    /// What a broken provider wrongly prefixes the path with.
    prefix: &'static str,
}

fn signature(method: &str, path: &str, body: &[u8]) -> String {
    format!("{method} {path} {}", body.len())
}

fn expired() -> AuthRefusal {
    AuthRefusal::unauthenticated("attestation expired")
        .with_member("error", Value::String("attestation_expired".into()))
        .with_challenge(r#"Signature error="attestation_expired""#)
}

fn revoked() -> AuthRefusal {
    AuthRefusal::forbidden("device revoked")
        .with_member("error", Value::String("DEVICE_REVOKED".into()))
        .with_details(Value::Map(
            [("device".to_owned(), Value::String("d-1".into()))].into(),
        ))
}

impl AuthProvider for Signing {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        let header = |name| request.headers.get(name).and_then(|v| v.to_str().ok());
        match header("x-refusal") {
            Some("expired") => return Err(expired().into_error()),
            Some("revoked") => return Err(revoked().into_error()),
            Some(_) => return Err(CratestackError::Unauthorized("bad signature".into())),
            None => {}
        }
        let path = format!("{}{}", self.prefix, request.path);
        if header("x-signature") != Some(&signature(request.method, &path, request.body)) {
            return Err(CratestackError::Unauthorized("bad signature".into()));
        }
        Ok(CratestackContext::authenticated([(
            "sub".to_owned(),
            Value::String("device-1".into()),
        )]))
    }

    fn credential<'r>(&self, request: &RequestContext<'r>) -> Credential<'r> {
        if request.headers.contains_key("x-signature") || request.headers.contains_key("x-refusal")
        {
            Credential::Present { redact: None }
        } else {
            Credential::Absent
        }
    }
}

fn signed(canonical: &Canonical) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let value = signature(canonical.method, canonical.path, &canonical.body);
    headers.insert("x-signature", HeaderValue::from_str(&value).unwrap());
    headers
}

fn refusal(case: &'static str) -> impl Fn(&Canonical) -> HeaderMap + Send + Sync {
    move |_| {
        let mut headers = HeaderMap::new();
        headers.insert("x-refusal", HeaderValue::from_static(case));
        headers
    }
}

#[tokio::test]
async fn a_provider_that_signs_the_canonical_request_conforms() {
    auth_conformance(Signing { prefix: "" })
        .accepts("device-1", signed)
        .refuses("expired", refusal("expired"), Expected::refusal(&expired()))
        .refuses("revoked", refusal("revoked"), Expected::refusal(&revoked()))
        .refuses(
            "plain",
            refusal("plain"),
            Expected::unauthorized(Some("bad signature")),
        )
        .refuses(
            "unsigned",
            |_| HeaderMap::new(),
            Expected::unauthorized(None),
        )
        .run()
        .await;
}

#[tokio::test]
#[should_panic(expected = "does not conform")]
async fn a_provider_that_verifies_another_path_does_not_conform() {
    auth_conformance(Signing { prefix: "/kit" })
        .accepts("device-1", signed)
        .run()
        .await;
}
