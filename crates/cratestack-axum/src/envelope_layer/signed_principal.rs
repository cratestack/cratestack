//! Who a verified signer is, for the handler behind the layer: the
//! [`PrincipalMapper`](super::PrincipalMapper)'s answer, recorded with the
//! signer in the request's extensions. Shared by a unary signed request and
//! a signed subscription open.

use axum::response::Response;
use cratestack_core::{CratestackError, VerifiedSigner};
use http::{Extensions, StatusCode};

use super::layer::Config;
use super::principal::VerifiedRequest;
use super::refusal;
use super::seal::Sealer;
use crate::ratelimit::VerifiedPrincipal;

/// Map `signer` to its principal and record both in `extensions`; or, as
/// `Some`, the response to answer instead of calling the handler.
pub(super) async fn admit(
    config: &Config,
    sealer: &Sealer,
    signer: VerifiedSigner,
    extensions: &mut Extensions,
) -> Option<Response> {
    let verified = VerifiedRequest::new(&signer, &sealer.inputs.method, &sealer.inputs.route);
    let principal = match config.principal.principal(&verified).await {
        Ok(principal) if !principal.is_empty() => principal,
        Ok(_) => {
            let error = CratestackError::Internal("the principal mapper returned \"\"".to_owned());
            return Some(
                sealer
                    .seal_error(StatusCode::INTERNAL_SERVER_ERROR, error)
                    .await,
            );
        }
        // A signer the mapper refuses is answered like one that did not
        // verify: the same unsigned, coarse 401.
        Err(CratestackError::Unauthorized(_)) => {
            return Some(refusal::unauthenticated(&sealer.headers, &sealer.path));
        }
        Err(error) => {
            let error = CratestackError::Internal(format!("the principal mapper failed: {error}"));
            return Some(
                sealer
                    .seal_error(StatusCode::INTERNAL_SERVER_ERROR, error)
                    .await,
            );
        }
    };
    extensions.insert(signer);
    extensions.insert(VerifiedPrincipal(principal));
    None
}
