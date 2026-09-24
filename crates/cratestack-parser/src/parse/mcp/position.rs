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
    let mut in_string = false;
    for (index, ch) in raw.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            '@' if !in_string => {
                let rest = raw[index..].trim_start_matches('@');
                if rest.strip_prefix("mcp").is_some_and(|after| {
                    !after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
                }) {
                    return true;
                }
            }
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
    use super::{MCP, MODEL_MCP, attribute_has_name, contains_mcp_token};

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
        assert!(!contains_mcp_token("@mcpish"));
    }
}
