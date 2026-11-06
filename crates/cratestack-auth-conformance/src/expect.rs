//! [`Expected`]: how a refusal must read on every transport.

use std::collections::BTreeMap;

use cratestack::{AuthRefusal, AuthRefusalKind, CratestackError};
use http::StatusCode;
use serde_json::Value;

/// A refusal as the client must receive it, whatever the transport: the
/// status (401 or 403), the message, the extra envelope members, `details`
/// and the `WWW-Authenticate` challenges. The wire code follows from the
/// status: `UNAUTHORIZED` / `FORBIDDEN` on REST and MCP, `unauthenticated` /
/// `permission_denied` on RPC.
#[derive(Debug, Clone, PartialEq)]
pub struct Expected {
    pub(crate) status: StatusCode,
    pub(crate) message: Option<String>,
    pub(crate) members: BTreeMap<String, Value>,
    pub(crate) details: Option<Value>,
    pub(crate) challenges: Vec<String>,
    /// A plain error, not an `AuthRefusal`: MCP answers it with its own
    /// fixed message (ADR 0002), so only its status and code are compared
    /// there.
    pub(crate) plain: bool,
}

impl Expected {
    /// The rendering of `refusal`: its status, message, members, `details`
    /// and challenges, exactly.
    pub fn refusal(refusal: &AuthRefusal) -> Self {
        let status = match refusal.kind() {
            AuthRefusalKind::Forbidden => StatusCode::FORBIDDEN,
            _ => StatusCode::UNAUTHORIZED,
        };
        Self {
            status,
            message: Some(refusal.message().to_owned()),
            members: refusal
                .members()
                .iter()
                .map(|(name, value)| (name.clone(), json(value)))
                .collect(),
            details: refusal.details().map(json),
            challenges: refusal.challenges().to_vec(),
            plain: false,
        }
    }

    /// A plain `CratestackError::Unauthorized`: a 401 with no members,
    /// `details` or challenges. `message` is checked when given, except on
    /// MCP, which answers a plain error with its own fixed message; use an
    /// `AuthRefusal` for a message every client reads.
    pub fn unauthorized(message: Option<&str>) -> Self {
        Self::plain(StatusCode::UNAUTHORIZED, message)
    }

    /// A plain `CratestackError::Forbidden`: a 403 with no members, `details`
    /// or challenges. `message` is checked when given.
    pub fn forbidden(message: Option<&str>) -> Self {
        Self::plain(StatusCode::FORBIDDEN, message)
    }

    fn plain(status: StatusCode, message: Option<&str>) -> Self {
        Self {
            status,
            message: message.map(str::to_owned),
            members: BTreeMap::new(),
            details: None,
            challenges: Vec::new(),
            plain: true,
        }
    }

    pub(crate) fn rest_code(&self) -> &'static str {
        if self.status == StatusCode::FORBIDDEN {
            "FORBIDDEN"
        } else {
            "UNAUTHORIZED"
        }
    }

    pub(crate) fn rpc_code(&self) -> &'static str {
        if self.status == StatusCode::FORBIDDEN {
            "permission_denied"
        } else {
            "unauthenticated"
        }
    }
}

fn json(value: &cratestack::Value) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// An error a provider returned, as an [`Expected`] (for a provider whose
/// refusal the caller builds rather than describes).
impl From<&CratestackError> for Expected {
    fn from(error: &CratestackError) -> Self {
        match error {
            CratestackError::AuthRefused(refusal) => Self::refusal(refusal),
            CratestackError::Forbidden(message) => Self::forbidden(Some(message)),
            other => Self::unauthorized(Some(&other.public_message())),
        }
    }
}
