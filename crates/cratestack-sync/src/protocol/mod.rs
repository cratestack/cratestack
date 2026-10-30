//! Wire protocol shared by the server and client halves. JSON for the
//! prototype; nothing here is JSON-specific, so CBOR (the framework
//! default) is a codec swap, not a protocol change.
//!
//! Endpoints, all relative to wherever the router is mounted:
//!
//! | Method | Path | Body | Response |
//! |---|---|---|---|
//! | `POST` | `/sync/register` | [`RegisterRequest`] | [`RegisterResponse`] |
//! | `POST` | `/sync/push` | [`PushRequest`] | [`PushResponse`] |
//! | `POST` | `/sync/pull` | [`PullRequest`] | [`PullResponse`] |
//! | `GET` | `/sync/poke?clientId=…` | — | `text/event-stream` of payload-free `poke` events |
//!
//! Failures are a [`SyncErrorBody`] with the status from
//! [`SyncErrorCode::status`].

mod cursor;
mod error;
mod messages;

pub use cursor::{Cursor, CursorParseError};
pub use error::{RESERVED_SYNC_ERROR_MEMBERS, SyncErrorBody, SyncErrorCode};
pub use messages::{
    ACK_MODEL, Change, ChangeOp, Mutation, MutationOutcome, MutationResult, PullRequest,
    PullResponse, PushRequest, PushResponse, RegisterRequest, RegisterResponse, RejectCode,
    client_scope,
};
