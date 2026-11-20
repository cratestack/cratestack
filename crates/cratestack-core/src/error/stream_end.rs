//! [`StreamEndNotice`], why a live subscription was ended by the server
//! (cratestack#1381): one typed vocabulary for the credential clock, the
//! signed-open lifetime and a revoked, suspended or signed-out principal.
//!
//! On the wire it is an ordinary error frame whose `details` is
//! `{"reason": "<snake_case>"}`; the code and message of the two ends that
//! existed before (`credential_expired`, `stream_lifetime`) are unchanged,
//! so an older client reads them as it always did.

use serde::{Deserialize, Serialize};

use crate::value::Value;

/// Why a stream ended. `#[non_exhaustive]`: match with a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum StreamEndReason {
    /// The `exp` of the credential that opened the stream passed.
    /// `unauthenticated` 401: refresh and resubscribe.
    CredentialExpired,
    /// A signed open lived as long as the server allows
    /// (`max_signed_stream_life`). `unavailable` 503: resubscribe.
    StreamLifetime,
    /// The principal's session ended (logout, console session closed).
    /// `unauthenticated` 401: sign in again.
    SessionEnded,
    /// The principal was suspended. `permission_denied` 403: do not retry.
    PrincipalSuspended,
    /// The device key that signed the open was revoked. `permission_denied`
    /// 403: re-enrol, do not retry.
    DeviceRevoked,
    /// The revocation feed lost messages, so the server cannot tell whether
    /// this stream's principal was revoked and ends it to be safe.
    /// `unavailable` 503: resubscribe.
    RevocationResync,
}

impl StreamEndReason {
    const ALL: [Self; 6] = [
        Self::CredentialExpired,
        Self::StreamLifetime,
        Self::SessionEnded,
        Self::PrincipalSuspended,
        Self::DeviceRevoked,
        Self::RevocationResync,
    ];

    /// The snake_case name carried in `details.reason`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CredentialExpired => "credential_expired",
            Self::StreamLifetime => "stream_lifetime",
            Self::SessionEnded => "session_ended",
            Self::PrincipalSuspended => "principal_suspended",
            Self::DeviceRevoked => "device_revoked",
            Self::RevocationResync => "revocation_resync",
        }
    }

    /// The reason named `name` (the inverse of [`Self::as_str`]).
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|reason| reason.as_str() == name)
    }

    /// The public message a stream ended for this reason carries. The first
    /// two are the strings the stream bounds always used.
    pub const fn default_message(self) -> &'static str {
        match self {
            Self::CredentialExpired => "credential expired",
            Self::StreamLifetime => "stream lifetime",
            Self::SessionEnded => "session ended",
            Self::PrincipalSuspended => "principal suspended",
            Self::DeviceRevoked => "device revoked",
            Self::RevocationResync => "revocation state lost",
        }
    }

    /// Whether the end is the caller being refused for good (403), rather
    /// than a credential to renew (401) or a stream to reopen (503).
    pub const fn is_forbidden(self) -> bool {
        matches!(self, Self::PrincipalSuspended | Self::DeviceRevoked)
    }

    /// Whether the end asks the client to resubscribe as it is (503).
    pub const fn is_unavailable(self) -> bool {
        matches!(self, Self::StreamLifetime | Self::RevocationResync)
    }
}

/// A server-side end of a live stream: the typed reason and the public
/// message. Carried by
/// [`CratestackError::StreamEnded`](super::CratestackError::StreamEnded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamEndNotice {
    pub reason: StreamEndReason,
    pub message: String,
}

impl StreamEndNotice {
    /// An end with the reason's [`default_message`](StreamEndReason::default_message).
    pub fn new(reason: StreamEndReason) -> Self {
        Self {
            reason,
            message: reason.default_message().to_owned(),
        }
    }

    /// The error a stream ends with.
    pub fn into_error(self) -> super::CratestackError {
        super::CratestackError::StreamEnded(self)
    }

    /// `{"reason": "<snake_case>"}`, the wire `details`.
    pub fn details(&self) -> serde_json::Value {
        serde_json::json!({ "reason": self.reason.as_str() })
    }

    pub(super) fn details_value(&self) -> Value {
        Value::Map(
            [(
                "reason".to_owned(),
                Value::String(self.reason.as_str().to_owned()),
            )]
            .into(),
        )
    }
}

impl From<StreamEndReason> for StreamEndNotice {
    fn from(reason: StreamEndReason) -> Self {
        Self::new(reason)
    }
}

#[cfg(test)]
mod tests;
