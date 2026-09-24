//! Recognising an `@mcp`/`@@mcp` attribute by name, wherever it appears.
//!
//! Shared by the parser (which extracts the attribute where it belongs) and
//! by `validate::mcp::placement` (which rejects it everywhere else), so "is
//! this an MCP attribute?" has one answer in both places.

use cratestack_core::Attribute;

use crate::diagnostics::{SchemaError, span_error};

pub(crate) const MCP: &str = "@mcp";
pub(crate) const MODEL_MCP: &str = "@@mcp";

/// Whether `raw` is the attribute `name` — bare, with arguments, or in the
/// dotted form D1 rejected (`@mcp.tool`, kept recognisable so it gets a
/// pointed error instead of passing as an unrelated attribute). `@mcpx` is
/// not a match.
pub(crate) fn attribute_has_name(raw: &str, name: &str) -> bool {
    raw.strip_prefix(name).is_some_and(|rest| {
        rest.is_empty() || rest.starts_with('(') || rest.starts_with('.') || rest.starts_with(' ')
    })
}

/// Whether `@mcp` or `@@mcp` occurs anywhere in `raw` outside a string
/// literal — used to catch one sharing a line with another attribute.
pub(crate) fn contains_mcp_token(raw: &str) -> bool {
    any_attribute_head(raw, |head| head.strip_prefix("mcp").is_some_and(ends_name))
}

/// Looser than [`contains_mcp_token`]: any case (`@MCP`, `@@Mcp`) and
/// whitespace after the `@`s (`@ mcp`). The parser reads neither spelling,
/// so raw text this matches names MCP and is read by nothing —
/// `validate::mcp::inert` rejects it wherever it survives parsing.
pub(crate) fn mentions_mcp(raw: &str) -> bool {
    any_attribute_head(raw, |head| {
        let head = head.trim_start();
        head.get(..3)
            .is_some_and(|word| word.eq_ignore_ascii_case("mcp"))
            && ends_name(&head[3..])
    })
}

/// `@mcpx` or `@mcp_tool` is another name, not MCP.
fn ends_name(after: &str) -> bool {
    !after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether `matches` holds for the text after some run of `@`s outside a
/// string literal. Policy literals may be single- or double-quoted
/// (`@@allow('read', email == 'ops@mcp.io')`), so both open a string, and
/// only the same quote closes it.
fn any_attribute_head(raw: &str, matches: impl Fn(&str) -> bool) -> bool {
    let mut quote: Option<char> = None;
    for (index, ch) in raw.char_indices() {
        match (quote, ch) {
            (None, '"' | '\'') => quote = Some(ch),
            (Some(open), _) if ch == open => quote = None,
            (None, '@') if matches(raw[index..].trim_start_matches('@')) => return true,
            _ => {}
        }
    }
    false
}

/// An `@mcp`/`@@mcp` that shares a line with another attribute would be
/// stored inside that attribute's raw text and never seen — silently inert,
/// which is the one outcome MCP must not have.
pub(super) fn reject_embedded_mcp(attribute: &Attribute, owner: &str) -> Result<(), SchemaError> {
    if contains_mcp_token(&attribute.raw) {
        return Err(span_error(
            format!(
                "{owner}: an `@mcp`/`@@mcp` attribute must be on its own line; `{}` puts it \
                 after another attribute, where it would be read as part of that one",
                attribute.raw
            ),
            attribute.span,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MCP, MODEL_MCP, attribute_has_name, contains_mcp_token, mentions_mcp};

    #[test]
    fn names_match_exactly_not_by_prefix() {
        assert!(attribute_has_name("@mcp(tool)", MCP));
        assert!(attribute_has_name("@mcp", MCP));
        assert!(attribute_has_name("@mcp.tool", MCP));
        assert!(!attribute_has_name("@mcpx(tool)", MCP));
        assert!(!attribute_has_name("@@mcp(resource: \"x\")", MCP));
        assert!(attribute_has_name("@@mcp(resource: \"x\")", MODEL_MCP));
    }

    #[test]
    fn finds_an_mcp_token_after_another_attribute_but_not_inside_a_string() {
        assert!(contains_mcp_token("@allow(true) @mcp(tool)"));
        assert!(contains_mcp_token(
            "@@allow(\"read\", true) @@mcp(resource: \"x\")"
        ));
        assert!(!contains_mcp_token("@allow(auth().note == \"@mcp\")"));
        assert!(!contains_mcp_token(
            "@@allow('read', email == 'ops@mcp.io')"
        ));
        assert!(!contains_mcp_token("@allow(auth().note == \"it's @mcp\")"));
        assert!(contains_mcp_token(
            "@@allow('read', true) @@mcp(resource: \"x\")"
        ));
        assert!(!contains_mcp_token("@mcpish"));
        assert!(!contains_mcp_token("@MCP(tool)"));
    }

    #[test]
    fn mentions_mcp_in_any_case_or_spacing_but_not_inside_a_string() {
        for raw in [
            "@MCP(tool)",
            "@@Mcp",
            "@ mcp(tool)",
            "@allow(true) @mCp(tool)",
        ] {
            assert!(mentions_mcp(raw), "{raw}");
        }
        for raw in [
            "@mcpx(tool)",
            "@mcp_tool",
            "@default(\"@MCP\")",
            "@allow(true)",
        ] {
            assert!(!mentions_mcp(raw), "{raw}");
        }
    }
}
