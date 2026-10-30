//! Request-level failures. Per-mutation failures are not errors — they are
//! [`super::MutationOutcome::Rejected`] inside a successful push.

use std::collections::BTreeMap;

use cratestack_core::Value;
use serde::{Deserialize, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncErrorCode {
    Unauthenticated,
    /// The application's `AuthProvider` knows the caller and refused it
    /// (an `AuthRefusal::forbidden`, cratestack#1243).
    Forbidden,
    /// The client id is registered to another principal.
    ClientMismatch,
    /// The client id is unknown or was evicted by the client TTL. Recovery:
    /// re-register, renumber pending mutations from 1, full resync.
    ClientExpired,
    /// The cursor predates the tombstone purge horizon. Recovery: full resync.
    CursorExpired,
    /// The pushed seqs do not continue from the server's last seen seq.
    SequenceGap,
    TooManyMutations,
    MutationTooLarge,
    TooManyClients,
    TooManyStreams,
    RateLimited,
    BadRequest,
    Internal,
}

impl SyncErrorCode {
    pub fn status(self) -> u16 {
        match self {
            Self::Unauthenticated => 401,
            Self::ClientMismatch | Self::Forbidden => 403,
            Self::SequenceGap => 409,
            Self::ClientExpired | Self::CursorExpired => 410,
            Self::TooManyMutations | Self::MutationTooLarge => 413,
            Self::TooManyClients | Self::TooManyStreams | Self::RateLimited => 429,
            Self::BadRequest => 400,
            Self::Internal => 500,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{code:?}: {message}")]
pub struct SyncErrorBody {
    pub code: SyncErrorCode,
    pub message: String,
    /// Set on `sequence_gap` so the client can realign.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_mutation_seq: Option<u64>,
    /// Set on `rate_limited`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u32>,
    /// An `AuthRefusal`'s `details` (cratestack#1243). Boxed so the body,
    /// the error of every sync handler, stays small.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Box<Value>>,
    /// An `AuthRefusal`'s extra members, top-level next to `code` and
    /// `message` as on REST (`{"code": "unauthenticated", ..., "error":
    /// "attestation_expired"}`). A name this body already uses
    /// ([`RESERVED_SYNC_ERROR_MEMBERS`]) is never written; read from the
    /// wire, every member the body does not know lands here.
    #[serde(flatten, serialize_with = "visible_members")]
    pub extra: BTreeMap<String, Value>,
    /// An `AuthRefusal`'s `WWW-Authenticate` challenges. Sent as headers,
    /// never in the body, so a decoded body has none.
    #[serde(skip)]
    pub challenges: Vec<String>,
}

/// The members [`SyncErrorBody::extra`] can never set.
pub const RESERVED_SYNC_ERROR_MEMBERS: [&str; 5] = [
    "code",
    "message",
    "details",
    "lastMutationSeq",
    "retryAfterSecs",
];

fn visible_members<S: Serializer>(
    extra: &BTreeMap<String, Value>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_map(
        extra
            .iter()
            .filter(|(name, _)| !RESERVED_SYNC_ERROR_MEMBERS.contains(&name.as_str())),
    )
}

impl SyncErrorBody {
    pub fn new(code: SyncErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            last_mutation_seq: None,
            retry_after_secs: None,
            details: None,
            extra: BTreeMap::new(),
            challenges: Vec::new(),
        }
    }
}
