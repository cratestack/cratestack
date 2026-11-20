//! The text a `NOTIFY` on [`NOTIFY_CHANNEL`] carries: the backplane channel
//! and the payload itself (text or bytes) or, for a model event too large for
//! Postgres' 8000-byte limit, the outbox row's `event_id` to read it from.
//!
//! Two envelopes exist (`cbor`, `json`):
//!
//! - **CBOR** (the default): `cbor:` + base64 of a CBOR map
//!   `{"v": 1, "c": channel, "t": text | "b": bytes | "r": event_id, "n": nonce}`.
//! - **JSON** (what 0.20.5 and earlier wrote and read):
//!   `{"c": channel, "p": text, "r": event_id, "n": nonce}`, plus `"b"`
//!   (base64) for a byte payload, which those readers ignore.
//!
//! The reader tells them apart by the first character, `{` or not, and
//! reads both, so a replica on either version hears the other.

mod cbor;
mod json;

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The one Postgres channel every replica `LISTEN`s on; backplane channels
/// are multiplexed inside the payload.
pub(crate) const NOTIFY_CHANNEL: &str = "cratestack_events";

/// Postgres refuses a notification payload of 8000 bytes or more. Applies to
/// the final notification text, whatever the envelope.
pub(crate) const NOTIFY_LIMIT: usize = 7900;

/// What starts a CBOR notification. The JSON envelope starts with `{`, so
/// the two can never be confused.
pub(crate) const CBOR_PREFIX: &str = "cbor:";

/// Source of [`Notification::nonce`]. Postgres folds identical notifications
/// of one transaction into one; only one process's transactions need telling
/// apart, so a counter is enough.
static NONCE: AtomicU64 = AtomicU64::new(0);

/// Set by `PgNotifyBackplane::with_json_envelope`: what the writers that hold
/// no backplane (a model event's write path, `publish_in_tx`) use.
static JSON_BY_DEFAULT: AtomicBool = AtomicBool::new(false);

/// Which envelope a notification is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Envelope {
    Cbor,
    Json,
}

impl Envelope {
    /// The process default: CBOR unless a backplane asked for JSON.
    pub(crate) fn current() -> Self {
        if JSON_BY_DEFAULT.load(Ordering::Relaxed) {
            Self::Json
        } else {
            Self::Cbor
        }
    }

    pub(crate) fn make_default_json() {
        JSON_BY_DEFAULT.store(true, Ordering::Relaxed);
    }
}

/// What a notification delivers.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Body<'a> {
    Text(Cow<'a, str>),
    Bytes(Cow<'a, [u8]>),
    /// The outbox row holding a large model event.
    Outbox(uuid::Uuid),
}

#[derive(Debug)]
pub(crate) struct Notification<'a> {
    /// The backplane channel.
    pub(crate) channel: Cow<'a, str>,
    pub(crate) body: Body<'a>,
    /// Makes every notification distinct, so Postgres does not deliver two
    /// identical ones from one transaction once. Readers ignore it.
    pub(crate) nonce: Option<u64>,
}

impl<'a> Notification<'a> {
    fn new(channel: &'a str, body: Body<'a>) -> Self {
        Self {
            channel: channel.into(),
            body,
            nonce: Some(NONCE.fetch_add(1, Ordering::Relaxed)),
        }
    }

    fn text(&self, envelope: Envelope) -> String {
        match envelope {
            Envelope::Cbor => cbor::write(self),
            Envelope::Json => json::write(self),
        }
    }
}

/// The notification text for the text `payload` on `channel`, falling back to
/// a reference to `event_id` when inline would exceed the limit. `None` when
/// it is too large and there is no row to refer to. Written in the process
/// default envelope.
pub(crate) fn encode(channel: &str, payload: &str, event_id: Option<uuid::Uuid>) -> Option<String> {
    encode_text(Envelope::current(), channel, payload, event_id)
}

pub(crate) fn encode_text(
    envelope: Envelope,
    channel: &str,
    payload: &str,
    event_id: Option<uuid::Uuid>,
) -> Option<String> {
    let inline = Notification::new(channel, Body::Text(payload.into())).text(envelope);
    if inline.len() <= NOTIFY_LIMIT {
        return Some(inline);
    }
    let by_reference = Notification::new(channel, Body::Outbox(event_id?)).text(envelope);
    (by_reference.len() <= NOTIFY_LIMIT).then_some(by_reference)
}

/// The notification text for the byte `payload`; `Err` is the encoded size
/// when it exceeds the limit (there is no outbox row to refer to).
pub(crate) fn encode_bytes(
    envelope: Envelope,
    channel: &str,
    payload: &[u8],
) -> Result<String, usize> {
    let text = Notification::new(channel, Body::Bytes(payload.into())).text(envelope);
    if text.len() <= NOTIFY_LIMIT {
        Ok(text)
    } else {
        Err(text.len())
    }
}

/// Reads a notification in either envelope. `Err` says why it is unreadable.
pub(crate) fn decode(raw: &str) -> Result<Notification<'_>, String> {
    // 0.20.5 parsed JSON that began with whitespace: so does this.
    let raw = raw.trim_start();
    match raw.strip_prefix(CBOR_PREFIX) {
        Some(encoded) => cbor::read(encoded),
        None if raw.starts_with('{') => json::read(raw),
        None => Err("neither the CBOR nor the JSON envelope".to_owned()),
    }
}
