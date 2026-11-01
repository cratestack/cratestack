//! `topic` IR node (`docs/design/subscriptions-websocket.md` §3.4, slice
//! S4): a typed, keyed, cross-replica publish/subscribe channel, declared
//! once as `topic RoomMessage(roomId: String): ChatMessage`.
//!
//! A topic is server-internal: it is not an op, has no route and no policy
//! of its own. The `subscription procedure` that returns its stream is the
//! policy boundary, so nothing about a topic reaches a client contract.

use serde::{Deserialize, Serialize};

use super::SourceSpan;
use super::model::TypeRef;
use super::procedure::ProcedureArg;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Topic {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    /// The one key a message is published and subscribed under
    /// (`roomId: String`).
    pub key: ProcedureArg,
    /// The message type.
    pub payload: TypeRef,
    pub span: SourceSpan,
}
