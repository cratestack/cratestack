//! `name = "<name>"` inside `mcp { }`: the `<name>` of every resource URI,
//! `cratestack://<name>/<segment>/{id}` (maintainer decision on
//! cratestack#1040, replacing the `.cstack` file stem phase 5 first used).
//!
//! The entry's own shape is checked here, where the entry is read, as
//! `expose`'s is: a quoted string holding a lowercase DNS label,
//! `[a-z0-9-]`, 1 to 63 characters, no `-` at either end, plus two rules
//! of our own (`broken_label_rule`): not IDNA's reserved `??--` form, and
//! at least one letter. Being set twice is `block.rs`'s rule, beside
//! `expose`'s. Whether the block needs a `name` at all depends on `expose`,
//! so those two rules are cross-entry and live with the other MCP scope
//! rules in `validate::mcp::name`.

use cratestack_core::McpName;

use super::block::entry_error;
use crate::diagnostics::SchemaError;
use crate::line_helpers::{Line, trimmed_span};

/// `value` is the text after `=`.
pub(super) fn parse_name(line: &Line<'_>, value: &str) -> Result<McpName, SchemaError> {
    let trimmed = value.trim();
    let Some(inner) = trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return Err(entry_error(
            line,
            &format!(
                "`name = {trimmed}` must be a quoted string, like `name = \"blog\"`: it is the \
                 `<name>` in every resource URI, `cratestack://<name>/<segment>/{{id}}`"
            ),
        ));
    };
    if let Some(rule) = broken_label_rule(inner) {
        return Err(entry_error(
            line,
            &format!(
                "`name = {trimmed}` {rule}: it is the host of every resource URI, \
                 `cratestack://<name>/<segment>/{{id}}`, which is matched exactly"
            ),
        ));
    }
    Ok(McpName {
        value: inner.to_owned(),
        span: trimmed_span(line),
    })
}

/// The DNS-label rule `name` breaks first, if any (maintainer decision on
/// cratestack#1040: the name is a URI's host, so it is held to what a host
/// label may be, RFC 1035 § 2.3.4 and RFC 1123 § 2.1). Lowercase only, not
/// the case-insensitive label DNS allows, because the host is compared
/// exactly and one spelling must be the only spelling. One message per rule,
/// so each rule is its own test.
///
/// Two rules go beyond a DNS label's (maintainer decisions on
/// cratestack#1040, 2026-09-25). `--` as the 3rd and 4th characters is the
/// form IDNA reserves (RFC 5891 § 4.2.3.1): `xn--` marks an encoded Unicode
/// label, which an IDNA-aware client may show as a different string, and
/// every other `??--` is held for a prefix like it. And a name needs a
/// letter, so an all-digit one such as `127` is never taken for a number or
/// an address.
///
/// The character set is checked before the length because `len()` counts
/// bytes: only once every byte is ASCII is it also the character count, so
/// a 32-character non-ASCII name is not told it is too long, and byte
/// offsets are character positions for the reserved-form check. After
/// those, the narrowest rule first: two exact positions, then either end,
/// then the whole name. So `xn--` hears about IDNA, not about its trailing
/// `-` (`xn--a`, which fixes only the end, is still the reserved form),
/// and `-127` about its `-`, not about the letter it also lacks.
fn broken_label_rule(name: &str) -> Option<&'static str> {
    let charset = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-';
    if !name.bytes().all(charset) {
        return Some("must be lowercase ASCII letters, digits and `-` only");
    }
    if name.is_empty() || name.len() > 63 {
        return Some("must be 1 to 63 characters, the length of a DNS label");
    }
    if name.get(2..4) == Some("--") {
        return Some(
            "must not have `--` as its 3rd and 4th characters, the form IDNA reserves for \
             encoded labels like `xn--` (RFC 5891 § 4.2.3.1)",
        );
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Some("must not start or end with `-`, as a DNS label may not");
    }
    if !name.bytes().any(|byte| byte.is_ascii_lowercase()) {
        return Some("must contain at least one letter, so it never reads as a number");
    }
    None
}
