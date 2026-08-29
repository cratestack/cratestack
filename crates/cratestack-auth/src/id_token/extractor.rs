//! `FromRequestParts` extractor impls for [`CurrentPrincipal`]/[`AuthenticatedPrincipal`].
//!
//! This whole module is only registered (see `id_token.rs`) behind the crate's
//! default-on `axum` feature, so a signing-only consumer that opts out of `axum`
//! never compiles it — hence no per-item `#[cfg(feature = "axum")]` here.

use axum::{extract::FromRequestParts, http::request::Parts, response::Response};
use http::StatusCode;

use super::principal::{AuthenticatedPrincipal, CurrentPrincipal, RequestPrincipal};

impl<S> FromRequestParts<S> for CurrentPrincipal
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<RequestPrincipal>()
            .cloned()
            .map(Self)
            .ok_or_else(|| {
                principal_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "principal_unavailable",
                    "Request principal was not installed by authentication middleware",
                )
            })
    }
}

impl<S> FromRequestParts<S> for AuthenticatedPrincipal
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let principal = CurrentPrincipal::from_request_parts(parts, state).await?;
        if principal.0.user.is_none() {
            return Err(principal_error_response(
                StatusCode::UNAUTHORIZED,
                "authenticated_principal_required",
                "Protected endpoint requires a validated id_jwt bound to the request signature key",
            ));
        }

        Ok(Self(principal.0))
    }
}

fn principal_error_response(status: StatusCode, code: &str, message: &str) -> Response {
    crate::response::error_response(status, code, message)
}
