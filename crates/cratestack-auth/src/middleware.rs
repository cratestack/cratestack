//! `axum`-gated tower middleware enforcing [`crate::SignedRequestVerifier`]
//! on protected routes. See the crate's `axum` Cargo feature for why this
//! module only compiles when it is enabled.

mod error_mapping;
#[cfg(test)]
mod tests;

use axum::{
    body::{Body, to_bytes},
    extract::State,
    http::{HeaderMap, Request, header::AUTHORIZATION},
    middleware::Next,
    response::Response,
};

use crate::{AuthError, SignedRequestVerifier};
use error_mapping::auth_error_response;

pub async fn require_signed_request(
    State(verifier): State<SignedRequestVerifier>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let headers = request.headers().clone();
    let authorization = match authorization_header(&headers) {
        Ok(header) => header,
        Err(error) => return auth_error_response(error),
    };

    let (parts, body) = request.into_parts();
    let body_bytes = match to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(error) => {
            return auth_error_response(AuthError::RequestBodyRead(error.to_string()));
        }
    };
    let mut request = Request::from_parts(parts, Body::from(body_bytes.clone()));

    match verifier
        .authenticate(request.method(), request.uri(), &body_bytes, authorization)
        .await
    {
        Ok(principal) => {
            // Internal/service-to-service routes are guarded by this middleware
            // and must only accept trusted service callers. A signing key resolved
            // via the cnf-bound id_jwt PoP fallback is an end-user device, not a
            // service — reject it so an enrolled user can't reach `/internal/*`.
            if principal.transport.via_id_token_pop {
                return auth_error_response(AuthError::InternalEndpointForbidden);
            }
            request.extensions_mut().insert(principal);
            next.run(request).await
        }
        Err(error) => auth_error_response(error),
    }
}

fn authorization_header(headers: &HeaderMap) -> Result<&str, AuthError> {
    let value = headers
        .get(AUTHORIZATION)
        .ok_or(AuthError::MissingAuthorizationHeader)?;
    value
        .to_str()
        .map_err(|_| AuthError::InvalidAuthorizationHeaderEncoding)
}
