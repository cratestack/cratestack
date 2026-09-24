//! `name = "<name>"` inside `mcp { }`: the `<name>` of every resource URI,
//! `cratestack://<name>/<segment>/{id}` (maintainer decision on
//! cratestack#1040, replacing the `.cstack` file stem phase 5 first used).
//!
//! The entry's own shape is checked here, where the entry is read, as
//! `expose`'s is: a quoted string of `[a-z0-9-]+`. Being set twice is
//! `block.rs`'s rule, beside `expose`'s. Whether the block needs a `name`
//! at all depends on `expose`, so those two rules are cross-entry and live
//! with the other MCP scope rules in `validate::mcp::name`.

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
    let well_formed = !inner.is_empty()
        && inner
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if !well_formed {
        return Err(entry_error(
            line,
            &format!(
                "`name = {trimmed}` must be lowercase ASCII letters, digits and `-` only, and \
                 not empty: it is the host of every resource URI, \
                 `cratestack://<name>/<segment>/{{id}}`, which is matched exactly"
            ),
        ));
    }
    Ok(McpName {
        value: inner.to_owned(),
        span: trimmed_span(line),
    })
}
