//! Freshness and replay for `nonce` mode (ADR 0006 §5, the P0 mode).
//!
//! A request carries `iat` (seconds) and a 16-byte random `cti`. The
//! opener accepts it only while `|now - iat| <= skew`, and records
//! `(kid, cti)` in core's [`NonceStore`](cratestack_core::NonceStore) so
//! that a second delivery inside that window is refused. P2's `window`
//! mode (device counters, no time limit) replaces this for device keys.

use chrono::{DateTime, Utc};
use cratestack_core::CratestackError;

/// The default clock skew, the same 300 s `HmacEnvelope` and the
/// signed-request verifier use (`ENVELOPE_DEFAULT_CLOCK_SKEW_SECS`).
pub const DEFAULT_SKEW_SECS: u64 = 300;

/// The length of the random `cti` the default source produces (§5).
pub const RANDOM_CTI_LEN: usize = 16;

/// Whether `iat` is within `skew` seconds of `now`, in either direction.
/// A future `iat` is as suspect as an old one: accepting it would stretch
/// the window a captured request stays replayable in.
pub(crate) fn is_fresh(now: i64, iat: u64, skew: u64) -> bool {
    (i128::from(now) - i128::from(iat)).unsigned_abs() <= u128::from(skew)
}

/// When the nonce store may forget `(kid, cti)`: `iat + 2·skew + 1`.
///
/// The request stops being fresh at `iat + skew` **on the verifier's
/// clock**, but the store expires entries on **another** clock: the
/// in-memory store compares against `Utc::now()`, and `cratestack-auth`'s
/// Redis store turns the expiry into a TTL against the recording replica's
/// clock. If the clocks disagree, an entry can vanish while some replica
/// still accepts the request, which is a replay window. The second `skew` is the margin
/// for that disagreement: the deployment already assumes clocks agree
/// within `skew` (every sender's does), so its own replicas and store are
/// held to the same bound. The final second covers the boundary, where
/// `InMemoryNonceStore` drops an entry whose expiry is not strictly in the
/// future. The cost is a working set twice the skew window.
pub(crate) fn nonce_expiry(iat: u64, skew: u64) -> Option<DateTime<Utc>> {
    let iat = i64::try_from(iat).ok()?;
    let skew = i64::try_from(skew).ok()?;
    let last = iat.checked_add(skew.checked_mul(2)?)?.checked_add(1)?;
    DateTime::from_timestamp(last, 0)
}

/// The key a `(kid, cti)` pair is recorded under. `NonceStore` takes a
/// string, and the store may be shared with `HmacEnvelope`, so the key is
/// namespaced and hex-encoded; the fixed `kid` length keeps the split
/// between `kid` and `cti` unambiguous.
pub(crate) fn nonce_key(kid: &[u8], cti: &[u8]) -> String {
    let mut key = String::with_capacity(6 + 2 * (kid.len() + cti.len()));
    key.push_str("cose:");
    push_hex(&mut key, kid);
    key.push(':');
    push_hex(&mut key, cti);
    key
}

fn push_hex(out: &mut String, bytes: &[u8]) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
}

/// The default clock: Unix seconds from `chrono`, which also works on
/// `wasm32-unknown-unknown`.
pub(crate) fn system_clock() -> i64 {
    Utc::now().timestamp()
}

/// The default `cti` source: 16 bytes from the operating system's CSPRNG.
pub(crate) fn random_cti() -> Result<Vec<u8>, CratestackError> {
    let mut cti = vec![0; RANDOM_CTI_LEN];
    getrandom::fill(&mut cti)
        .map_err(|error| CratestackError::Internal(format!("cose cti source failed: {error}")))?;
    Ok(cti)
}
