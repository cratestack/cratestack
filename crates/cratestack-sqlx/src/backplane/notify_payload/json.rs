//! The legacy JSON envelope: what 0.20.5 and earlier wrote, which a rollout
//! still has to read and, until every replica upgrades, write.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

use super::{Body, Notification};

#[derive(Debug, Serialize, Deserialize)]
struct Wire<'a> {
    #[serde(rename = "c", borrow)]
    channel: std::borrow::Cow<'a, str>,
    /// The text payload, inline.
    #[serde(rename = "p", default, skip_serializing_if = "Option::is_none", borrow)]
    payload: Option<std::borrow::Cow<'a, str>>,
    /// Or the outbox row holding it (a large model event).
    #[serde(rename = "r", default, skip_serializing_if = "Option::is_none")]
    event_id: Option<uuid::Uuid>,
    /// A byte payload, base64. New in 0.20.6: an older reader ignores the
    /// field (and so the message, which carries nothing it knows).
    #[serde(rename = "b", default, skip_serializing_if = "Option::is_none", borrow)]
    bytes: Option<std::borrow::Cow<'a, str>>,
    /// Written, never read: whatever it holds, a reader ignores it.
    #[serde(
        rename = "n",
        default,
        skip_deserializing,
        skip_serializing_if = "Option::is_none"
    )]
    nonce: Option<u64>,
}

pub(super) fn write(notification: &Notification<'_>) -> String {
    let mut wire = Wire {
        channel: notification.channel.as_ref().into(),
        payload: None,
        event_id: None,
        bytes: None,
        nonce: notification.nonce,
    };
    match &notification.body {
        Body::Text(text) => wire.payload = Some(text.as_ref().into()),
        Body::Bytes(bytes) => wire.bytes = Some(STANDARD.encode(bytes).into()),
        Body::Outbox(id) => wire.event_id = Some(*id),
    }
    serde_json::to_string(&wire).expect("a map of strings and numbers always serializes")
}

pub(super) fn read(raw: &str) -> Result<Notification<'_>, String> {
    let wire: Wire<'_> = serde_json::from_str(raw).map_err(|error| error.to_string())?;
    // A 0.20.5 writer sends `p` or `r`, never both and never `b`; so more
    // than one is not from any writer, and is refused, not tie-broken.
    let body = match (wire.payload, wire.event_id, wire.bytes) {
        (Some(text), None, None) => Body::Text(text),
        (None, Some(id), None) => Body::Outbox(id),
        (None, None, Some(bytes)) => Body::Bytes(
            STANDARD
                .decode(bytes.as_bytes())
                .map_err(|error| format!("bad base64 in `b`: {error}"))?
                .into(),
        ),
        (None, None, None) => return Err("carries no payload".to_owned()),
        _ => return Err("more than one of `p`, `r` and `b`".to_owned()),
    };
    Ok(Notification {
        channel: wire.channel,
        body,
        nonce: None,
    })
}
