//! Request-level entry points that turn a [`cratestack_core::RequestContext`]
//! into a verified [`cratestack_core::CratestackContext`], plus the small
//! `http` helpers they need.

use crate::error::auth_error_to_cratestack_error;
use crate::id_token::RequestPrincipal;
use crate::signed_request::SignedRequestVerifier;

pub fn authorization_header(headers: &http::HeaderMap) -> Option<String> {
    headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

pub fn request_uri(path: &str, query: Option<&str>) -> Result<http::Uri, http::uri::InvalidUri> {
    let value = match query {
        Some(query) => format!("{path}?{query}"),
        None => path.to_owned(),
    };
    value.parse()
}

pub async fn authenticate_cratestack_request<F>(
    verifier: SignedRequestVerifier,
    request: &cratestack_core::RequestContext<'_>,
    map_context: F,
) -> Result<cratestack_core::CratestackContext, cratestack_core::CratestackError>
where
    F: FnOnce(
            &RequestPrincipal,
        )
            -> Result<cratestack_core::CratestackContext, cratestack_core::CratestackError>
        + Send,
{
    authenticate_cratestack_request_with(verifier, request, |principal| {
        core::future::ready(map_context(&principal))
    })
    .await
}

/// Like [`authenticate_cratestack_request`] but the context mapper is **async** and
/// receives the principal by value. This lets the caller consult live state
/// (e.g. reload a user's role from the database) and adjust the verified
/// principal before building the `CratestackContext` — e.g. re-deriving an admin
/// role on every request so revoking it takes effect immediately instead of
/// waiting for the frozen `role` claim to expire.
pub async fn authenticate_cratestack_request_with<F, Fut>(
    verifier: SignedRequestVerifier,
    request: &cratestack_core::RequestContext<'_>,
    map_context: F,
) -> Result<cratestack_core::CratestackContext, cratestack_core::CratestackError>
where
    F: FnOnce(RequestPrincipal) -> Fut + Send,
    Fut: core::future::Future<
            Output = Result<cratestack_core::CratestackContext, cratestack_core::CratestackError>,
        > + Send,
{
    let authorization = authorization_header(request.headers);
    let method = request.method.to_owned();
    let path = request.path.to_owned();
    let query = request.query.map(str::to_owned);
    let body = request.body.to_vec();

    let Some(authorization) = authorization else {
        return Ok(cratestack_core::CratestackContext::anonymous());
    };

    let uri = request_uri(&path, query.as_deref())
        .map_err(|error| cratestack_core::CratestackError::BadRequest(error.to_string()))?;
    let method = http::Method::from_bytes(method.as_bytes())
        .map_err(|error| cratestack_core::CratestackError::BadRequest(error.to_string()))?;
    let principal = verifier
        .authenticate(&method, &uri, &body, &authorization)
        .await
        .map_err(auth_error_to_cratestack_error)?;

    map_context(principal).await
}
