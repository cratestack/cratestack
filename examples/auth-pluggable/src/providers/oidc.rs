//! An Authkestra OIDC resource server (`authkestra-resource`'s
//! `JwtStrategy`): an access token from `authkestra-op`, bound to the
//! client's key with DPoP (RFC 9449), and mapped onto the principal
//! contract (`sub`, `roles`, `amr`, `acr`, `auth_time`, `cnf.jkt`).

use std::sync::Arc;

use authkestra_engine::strategy::AuthenticationStrategy;
use authkestra_engine::token::Claims;
use authkestra_resource::jwt::JwtStrategy;
use cratestack::{
    AuthProvider, AuthRefusal, CratestackContext, CratestackError, Credential, RequestContext,
    Value,
};

/// The OIDC provider.
#[derive(Clone)]
pub struct Provider(pub Arc<JwtStrategy<Claims>>);

impl AuthProvider for Provider {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        if self.credential(request).is_absent() {
            return Ok(CratestackContext::anonymous());
        }
        // `JwtStrategy` reads `http::request::Parts`: the canonical method
        // and path (`htm` / `htu`) and the headers.
        let uri = match request.query {
            Some(query) => format!("{}?{query}", request.path),
            None => request.path.to_owned(),
        };
        let (mut parts, ()) = http::Request::builder()
            .method(request.method)
            .uri(uri)
            .body(())
            .map_err(|error| CratestackError::BadRequest(error.to_string()))?
            .into_parts();
        parts.headers = request.headers.clone();
        match self.0.authenticate(&parts).await {
            Ok(Some(claims)) => Ok(context(claims)),
            // `JwtStrategy` says only "no": the reason is in its logs.
            Ok(None) => Err(AuthRefusal::unauthenticated("invalid DPoP-bound token")
                .with_member("error", Value::String("invalid_token".to_owned()))
                .with_challenge(r#"DPoP error="invalid_token", algs="EdDSA ES256""#)
                .into_error()),
            Err(error) => Err(CratestackError::Internal(error.to_string())),
        }
    }

    fn credential<'r>(&self, request: &RequestContext<'r>) -> Credential<'r> {
        let authorization = request.headers.get("authorization");
        match authorization.and_then(|value| value.to_str().ok()) {
            Some(value) if value.starts_with("DPoP ") => Credential::Present { redact: None },
            Some(_) => Credential::Malformed("expected a DPoP-bound access token"),
            None => Credential::Absent,
        }
    }
}

fn context(claims: Claims) -> CratestackContext {
    let attributes = claims.identity.map(|identity| identity.attributes);
    let attribute = |name: &str| attributes.as_ref().and_then(|map| map.get(name)).cloned();
    let words = |name: &str| attribute(name).unwrap_or_default();
    let mut builder = CratestackContext::builder(claims.sub)
        .roles(words("roles").split_whitespace())
        .amr(words("amr").split_whitespace());
    if let Some(acr) = attribute("acr") {
        builder = builder.acr(acr);
    }
    if let Some(auth_time) = attribute("auth_time").and_then(|value| value.parse().ok()) {
        builder = builder.auth_time(auth_time);
    }
    if let Some(jkt) = claims.extra.get("cnf").and_then(|cnf| cnf["jkt"].as_str()) {
        builder = builder.cnf_jkt(jkt);
    }
    builder.build()
}
