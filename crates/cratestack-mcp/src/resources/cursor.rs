//! The opaque collection cursor.
//!
//! **What it carries:** a position in the caller's own visible sequence —
//! the number of visible records already returned — bound to the resource
//! it came from. Hex of `version (1) ‖ offset (8, big-endian) ‖ tag (8)`,
//! where `tag` is the first 8 bytes of SHA-256 over a domain label, the
//! segment and the offset.
//!
//! **Why an offset over the *visible* rows leaks nothing.** The page query
//! applies the row policy in its `WHERE`, before `ORDER BY ... LIMIT ...
//! OFFSET` (cratestack-sqlx's `push_scoped_conditions`, then
//! `push_order_and_paging`), so offset `n` means "the n-th row this caller
//! may read". Where a hidden row sits changes no page boundary the caller
//! can observe.
//!
//! **Why the tag is unkeyed.** It exists so a cursor that was edited,
//! truncated or taken from another resource is refused with `-32602`
//! instead of silently serving some other page — not to stop a forger. A
//! forged cursor with a correct tag names an offset the caller could reach
//! by paging anyway, over rows it may read anyway, so a secret key would
//! protect nothing, and it would tie every cursor to the process that
//! minted it: under Streamable HTTP (phase 4) a second replica, or a
//! restart, would reject every cursor in flight. Whether to add an
//! application-supplied key anyway is left to the maintainer.
//!
//! **Opaque** is a promise about the format, not a secret: clients must not
//! build or parse cursors, so the layout can change behind the version byte.

use sha2::{Digest, Sha256};

const VERSION: u8 = 1;
const TAG_LEN: usize = 8;
const ENCODED_LEN: usize = 2 * (1 + 8 + TAG_LEN);
const DOMAIN: &[u8] = b"cratestack-mcp/resource-cursor/v1";

/// The cursor for the page that starts `offset` visible records in.
pub(crate) fn encode(segment: &str, offset: u64) -> String {
    let mut bytes = Vec::with_capacity(1 + 8 + TAG_LEN);
    bytes.push(VERSION);
    bytes.extend_from_slice(&offset.to_be_bytes());
    bytes.extend_from_slice(&tag(segment, offset));
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The offset a cursor names, or `None` for anything this server did not
/// mint for `segment`. One answer for every failure: telling a caller
/// *which* check failed would only document the format.
pub(crate) fn decode(segment: &str, cursor: &str) -> Option<u64> {
    // Lowercase hex only, exactly as `encode` writes it: one cursor per
    // position, so an edited-but-equivalent spelling is refused too.
    let lower_hex = |byte: u8| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte);
    if cursor.len() != ENCODED_LEN || !cursor.bytes().all(lower_hex) {
        return None;
    }
    let bytes = (0..ENCODED_LEN)
        .step_by(2)
        .map(|at| u8::from_str_radix(&cursor[at..at + 2], 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    let (&version, rest) = bytes.split_first()?;
    let (offset, received) = rest.split_at(8);
    let offset = u64::from_be_bytes(offset.try_into().ok()?);
    // `i64::MAX` is the largest `OFFSET` the ORM can bind.
    let in_range = i64::try_from(offset).is_ok();
    (version == VERSION && in_range && received == tag(segment, offset)).then_some(offset)
}

fn tag(segment: &str, offset: u64) -> [u8; TAG_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    hasher.update([0]);
    hasher.update(segment.as_bytes());
    hasher.update([0]);
    hasher.update(offset.to_be_bytes());
    let digest = hasher.finalize();
    let mut tag = [0; TAG_LEN];
    tag.copy_from_slice(&digest[..TAG_LEN]);
    tag
}
