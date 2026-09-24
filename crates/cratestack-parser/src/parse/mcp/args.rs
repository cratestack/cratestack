//! Argument-list lexing for `@mcp(...)` / `@@mcp(...)`.
//!
//! Written rather than reused: the existing splitters
//! (`cratestack-core`'s `attribute_syntax::split_top_level_commas`, this
//! crate's `parse::arg_split`) only track bracket depth, not quotes, so a
//! comma inside `description: "Publish, then notify."` would split the
//! description in two (cratestack#1036, "Assumptions"). The syntax was not
//! bent to fit a helper; this helper was written to fit the syntax.
//!
//! Same string-literal model as the rest of the parser: a `"` always toggles
//! the in-string state and there are no escape sequences (see
//! `parse::attribute_spacing`'s module doc). A description therefore cannot
//! contain a `"`, which is stated in the error rather than silently
//! mis-split.

/// One `key` or `key: value` argument. `value` is the raw, trimmed text
/// after the colon; the caller decides whether it must be a string literal or
/// an integer, since that depends on the key.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct McpArg<'a> {
    pub(super) key: &'a str,
    pub(super) value: Option<&'a str>,
}

/// Splits the text between an attribute's parentheses into arguments.
///
/// Errors (as a message; the caller attaches the span) on an unterminated
/// string, an empty argument (`tool,` or `, tool`), and a key that is not a
/// bare identifier — each of which would otherwise be dropped or mis-read.
pub(super) fn parse_mcp_args(inner: &str) -> Result<Vec<McpArg<'_>>, String> {
    let mut args = Vec::new();
    if inner.trim().is_empty() {
        // `@mcp()` has no arguments rather than one empty one, so the caller
        // reports the missing required key, which is the real mistake.
        return Ok(args);
    }
    for part in split_outside_quotes(inner, ',')? {
        let part = part.trim();
        if part.is_empty() {
            return Err("has an empty argument (a stray `,`)".to_owned());
        }
        let (key, value) = match split_outside_quotes(part, ':')?.as_slice() {
            [key] => (key.trim(), None),
            [key, value] => (key.trim(), Some(value.trim())),
            _ => return Err(format!("has a malformed argument `{part}`")),
        };
        if !is_identifier(key) {
            return Err(format!(
                "has a malformed argument `{part}` (expected `key` or `key: value`)"
            ));
        }
        args.push(McpArg { key, value });
    }
    Ok(args)
}

/// The contents of a `"..."` literal, or `None` if `value` is anything else.
fn string_literal(value: &str) -> Option<&str> {
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.contains('"')).then_some(inner)
}

fn split_outside_quotes(input: &str, separator: char) -> Result<Vec<&str>, String> {
    let mut parts = Vec::new();
    let mut in_string = false;
    let mut start = 0usize;
    for (index, ch) in input.char_indices() {
        if ch == '"' {
            in_string = !in_string;
        } else if ch == separator && !in_string {
            parts.push(&input[start..index]);
            start = index + ch.len_utf8();
        }
    }
    if in_string {
        return Err("has an unterminated string literal".to_owned());
    }
    parts.push(&input[start..]);
    Ok(parts)
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

pub(super) fn quoted(value: &str, key: &str) -> Result<String, String> {
    string_literal(value).map(str::to_owned).ok_or_else(|| {
        format!("has `{key}: {value}`; `{key}:` must be a string literal like `\"...\"`")
    })
}

pub(super) fn page_size(value: Option<&str>) -> Result<u32, String> {
    let value = value.unwrap_or_default();
    value.parse::<u32>().map_err(|_| {
        format!(
            "has `max_page_size: {value}`; `max_page_size:` must be an integer from 1 to {} \
             (ADR 0002 Q3)",
            cratestack_core::MCP_MAX_PAGE_SIZE
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{McpArg, parse_mcp_args, string_literal};

    #[test]
    fn a_comma_inside_a_string_does_not_split_the_argument() {
        let args = parse_mcp_args(r#"tool: "x", description: "a, b: c""#).expect("parses");
        assert_eq!(
            args,
            vec![
                McpArg {
                    key: "tool",
                    value: Some(r#""x""#)
                },
                McpArg {
                    key: "description",
                    value: Some(r#""a, b: c""#)
                },
            ]
        );
    }

    #[test]
    fn a_bare_key_has_no_value() {
        let args = parse_mcp_args("tool").expect("parses");
        assert_eq!(
            args,
            vec![McpArg {
                key: "tool",
                value: None
            }]
        );
    }

    #[test]
    fn stray_commas_unterminated_strings_and_odd_keys_are_errors() {
        assert!(parse_mcp_args("tool,").is_err());
        assert!(parse_mcp_args(", tool").is_err());
        assert!(parse_mcp_args(r#"tool: "x"#).is_err());
        assert!(parse_mcp_args(r#""tool": "x""#).is_err());
        assert!(parse_mcp_args("tool: a: b").is_err());
    }

    #[test]
    fn string_literal_requires_matching_quotes() {
        assert_eq!(string_literal(r#""posts""#), Some("posts"));
        assert_eq!(string_literal("posts"), None);
        assert_eq!(string_literal(r#""po"sts""#), None);
    }
}
