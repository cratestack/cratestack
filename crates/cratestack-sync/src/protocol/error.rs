//! Request-level failures. Per-mutation failures are not errors — they are
//! [`super::MutationOutcome::Rejected`] inside a successful push.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncErrorCode {
    Unauthenticated,
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
            Self::ClientMismatch => 403,
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
}

impl SyncErrorBody {
    pub fn new(code: SyncErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            last_mutation_seq: None,
            retry_after_secs: None,
        }
    }
}
