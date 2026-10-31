//! What a remote refusal says beyond its code (cratestack#1243): the
//! `WWW-Authenticate` challenges an `AuthRefusal` sends, and one place to
//! read the decoded error envelope from either error type.
//!
//! The envelope's extra members (cratestack#1208) are already decoded:
//! `CratestackErrorResponse::extra` on REST, `RpcErrorBody::extra` on RPC.

use cratestack_core::CratestackErrorResponse;

use crate::error::ClientError;
use crate::rpc::error::{RpcClientError, RpcRemoteError};
use crate::runtime::wire::RuntimeHeader;

mod challenges;

pub use challenges::Challenges;

/// Every `WWW-Authenticate` value in `headers`, raw and in the order they
/// arrived.
pub(crate) fn www_authenticate(headers: &[RuntimeHeader]) -> Challenges {
    headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("www-authenticate"))
        .map(|header| header.value.clone())
        .collect::<Vec<_>>()
        .into()
}

impl ClientError {
    /// The decoded error envelope of a [`ClientError::Remote`]: `code`,
    /// `message`, `details` and the extra members in `extra`. `None` for
    /// any other variant, or when the body did not decode.
    pub fn remote_error(&self) -> Option<&CratestackErrorResponse> {
        match self {
            ClientError::Remote { error, .. } => error.as_deref(),
            _ => None,
        }
    }

    /// The `WWW-Authenticate` values of a [`ClientError::Remote`], raw and
    /// in order; empty for any other variant.
    pub fn challenges(&self) -> &[String] {
        match self {
            ClientError::Remote { challenges, .. } => challenges.as_slice(),
            _ => &[],
        }
    }
}

impl RpcClientError {
    /// The server's answer, for an [`RpcClientError::Remote`].
    pub fn remote(&self) -> Option<&RpcRemoteError> {
        match self {
            RpcClientError::Remote(remote) => Some(remote),
            _ => None,
        }
    }

    /// The `WWW-Authenticate` values of an [`RpcClientError::Remote`], raw
    /// and in order; empty for any other variant.
    pub fn challenges(&self) -> &[String] {
        self.remote()
            .map_or(&[], |remote| remote.challenges.as_slice())
    }
}

#[cfg(test)]
mod tests;
