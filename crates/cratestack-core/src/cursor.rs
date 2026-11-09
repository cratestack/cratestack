//! Opaque keyset-cursor encoding shared by generated `list` routes and
//! hand-written `CursorPageInput`/`CursorPage<T>` procedures
//! (`docs/design/cursor-pagination.md`).
//!
//! A cursor is `base64url(json({"o": <order>, "v": [<value>|null, ..]}))`:
//! the *effective sort order* it was minted for and the last row's value
//! for each ordering column (primary-key tiebreaker last), each rendered
//! as the same wire string a `?field__gt=` filter would carry. It is
//! **not signed**: the server never trusts it for anything a caller could
//! not already express as a filter — the decoded values re-enter the
//! normal typed filter path and are `AND`ed with the read policy.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

use crate::CratestackError;

/// Upper bound on an accepted cursor's length in characters. Cursors are
/// attacker-controlled input; this bounds decode work before any parsing.
pub const MAX_CURSOR_LEN: usize = 2048;

#[derive(Serialize, Deserialize)]
struct CursorWire {
    o: String,
    v: Vec<Option<String>>,
}

/// Mint a cursor for `order` (the canonical effective sort, e.g.
/// `"-createdAt,id"`) from the last row's per-column wire values; `None`
/// is SQL `NULL`.
pub fn encode_cursor(order: &str, values: &[Option<String>]) -> String {
    let wire = CursorWire {
        o: order.to_owned(),
        v: values.to_vec(),
    };
    let json = serde_json::to_vec(&wire).expect("cursor wire shape always serializes");
    URL_SAFE_NO_PAD.encode(json)
}

/// Decode and validate a cursor against the order the request resolved
/// to. Every failure is a [`CratestackError::BadRequest`] (RPC
/// `invalid_argument`): oversized, not base64url/JSON, minted for a
/// different order, or carrying the wrong number of values.
pub fn decode_cursor(
    raw: &str,
    expected_order: &str,
    expected_len: usize,
) -> Result<Vec<Option<String>>, CratestackError> {
    let bad = |detail: &str| CratestackError::BadRequest(format!("invalid cursor: {detail}"));
    if raw.len() > MAX_CURSOR_LEN {
        return Err(bad("too long"));
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| bad("not valid base64url"))?;
    let wire: CursorWire = serde_json::from_slice(&bytes).map_err(|_| bad("malformed payload"))?;
    if wire.o != expected_order {
        return Err(bad("cursor was issued for a different sort order"));
    }
    if wire.v.len() != expected_len {
        return Err(bad("wrong number of key values"));
    }
    Ok(wire.v)
}

/// Render one ordering-column value as its cursor wire string through
/// serde (`DateTime` → RFC 3339, `Uuid` → hyphenated, numbers → decimal).
/// `None` for SQL `NULL`. Arrays/objects are not orderable scalars.
pub fn cursor_scalar<T: Serialize + ?Sized>(value: &T) -> Result<Option<String>, CratestackError> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::Null) => Ok(None),
        Ok(serde_json::Value::String(s)) => Ok(Some(s)),
        Ok(serde_json::Value::Number(n)) => Ok(Some(n.to_string())),
        Ok(serde_json::Value::Bool(b)) => Ok(Some(b.to_string())),
        _ => Err(CratestackError::BadRequest(
            "cursor pagination requires scalar ordering columns".to_owned(),
        )),
    }
}

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod tests;
