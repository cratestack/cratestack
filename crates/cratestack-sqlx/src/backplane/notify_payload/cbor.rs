//! The CBOR envelope: `cbor:` + base64 (standard alphabet, padded) of one
//! CBOR map with text keys:
//!
//! | key | value | |
//! |-----|-------|-|
//! | `v` | unsigned, `1` | version; any other is refused |
//! | `c` | text | the backplane channel |
//! | `t` / `b` / `r` | text / byte string / 16-byte string | the payload, or the outbox `event_id` |
//! | `n` | unsigned | nonce |
//!
//! Unknown keys are skipped, so a later minor version may add some.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use minicbor::{Decoder, Encoder};

use super::{Body, Notification};

const VERSION: u64 = 1;

pub(super) fn write(notification: &Notification<'_>) -> String {
    let mut encoder = Encoder::new(Vec::new());
    let entries = 3 + u64::from(notification.nonce.is_some());
    // Writing into a `Vec` cannot fail.
    let _ = encoder
        .map(entries)
        .and_then(|e| e.str("v")?.u64(VERSION))
        .and_then(|e| e.str("c")?.str(&notification.channel))
        .and_then(|e| match &notification.body {
            Body::Text(text) => e.str("t")?.str(text),
            Body::Bytes(bytes) => e.str("b")?.bytes(bytes),
            Body::Outbox(id) => e.str("r")?.bytes(id.as_bytes()),
        })
        .and_then(|e| match notification.nonce {
            Some(nonce) => e.str("n")?.u64(nonce),
            None => Ok(e),
        });
    let mut text = String::from(super::CBOR_PREFIX);
    STANDARD.encode_string(encoder.into_writer(), &mut text);
    text
}

pub(super) fn read(encoded: &str) -> Result<Notification<'static>, String> {
    let bytes = STANDARD
        .decode(encoded.as_bytes())
        .map_err(|error| format!("bad base64: {error}"))?;
    let bad = |error: minicbor::decode::Error| format!("bad CBOR: {error}");
    let mut decoder = Decoder::new(&bytes);
    let entries = decoder
        .map()
        .map_err(bad)?
        .ok_or("an indefinite-length map")?;
    let (mut version, mut channel, mut body, mut nonce) = (None, None, None, None);
    for _ in 0..entries {
        match decoder.str().map_err(bad)? {
            "v" => version = Some(decoder.u64().map_err(bad)?),
            "c" => channel = Some(decoder.str().map_err(bad)?.to_owned()),
            "t" => body = Some(Body::Text(decoder.str().map_err(bad)?.to_owned().into())),
            "b" => body = Some(Body::Bytes(decoder.bytes().map_err(bad)?.to_vec().into())),
            "r" => {
                let id = uuid::Uuid::from_slice(decoder.bytes().map_err(bad)?)
                    .map_err(|error| format!("bad event id in `r`: {error}"))?;
                body = Some(Body::Outbox(id));
            }
            "n" => nonce = Some(decoder.u64().map_err(bad)?),
            _ => decoder.skip().map_err(bad)?,
        }
    }
    match version {
        Some(VERSION) => {}
        Some(other) => return Err(format!("unsupported envelope version {other}")),
        None => return Err("no envelope version".to_owned()),
    }
    Ok(Notification {
        channel: channel.ok_or("no channel")?.into(),
        body: body.ok_or("carries no payload")?,
        nonce,
    })
}
