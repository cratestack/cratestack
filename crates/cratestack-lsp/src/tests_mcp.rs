//! MCP in the editor (cratestack#1036): completion offers the four new
//! words, hover resolves every MCP span the parser records, and the parser's
//! MCP rules arrive as diagnostics pointing at the offending attribute.

use std::str::FromStr;

use tower_lsp_server::ls_types::{CompletionItemKind, SemanticTokenType, Uri};

use crate::analyze::analyze_document;
use crate::completion::completion_items;
use crate::hover::locate_symbol;
use crate::semantic_tokens::{LEGEND, semantic_tokens};
use crate::text::{offset_to_position, range_from_offsets};

const SCHEMA: &str = r#"datasource db {
  provider = "postgresql"
}

/// Agent surface.
mcp {
  name = "blog"
  expose = [tools, resources]
}

type FeedArgs {
  limit Int
}

model Post {
  id Int @id

  @@allow("read", true)
  @@mcp(resource: "posts", max_page_size: 20)
}

procedure getFeed(args: FeedArgs): Post[]
  @allow(auth() != null)
  @mcp(tool, description: "Newest first.")
"#;

fn uri() -> Uri {
    Uri::from_str("file:///schema.cstack").expect("uri should parse")
}

#[test]
fn completion_offers_the_mcp_attributes_and_expose_key_with_detail() {
    let items = completion_items(None);
    for label in [
        "@mcp",
        "@@mcp",
        "expose = [tools, resources]",
        "name = \"...\"",
    ] {
        let item = items
            .iter()
            .find(|item| item.label == label)
            .unwrap_or_else(|| panic!("completion must offer `{label}`"));
        assert_eq!(item.kind, Some(CompletionItemKind::KEYWORD));
        assert!(item.detail.as_deref().is_some_and(|d| d.contains("MCP")));
    }
}

#[test]
fn hover_resolves_each_mcp_declaration() {
    let (schema, diagnostics) = analyze_document(&uri(), SCHEMA);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let schema = schema.expect("valid MCP schema");
    let at = |needle: &str| {
        let offset = SCHEMA.find(needle).expect("needle in schema") + 1;
        locate_symbol(&schema, offset).expect("hover target")
    };

    let tool = at("@mcp(tool");
    assert_eq!((tool.kind, tool.name.as_str()), ("mcp tool", "getFeed"));
    assert!(tool.detail.contains("named after the procedure"));
    assert_eq!(tool.docs, vec!["Newest first.".to_owned()]);

    let resource = at("@@mcp(");
    assert_eq!(
        (resource.kind, resource.name.as_str()),
        ("mcp resource", "posts")
    );
    assert!(resource.detail.contains("at most 20 records per page"));

    let expose = at("tools, resources]");
    assert_eq!(
        (expose.kind, expose.name.as_str()),
        ("mcp exposed kind", "tools")
    );
    assert_eq!(expose.detail, "tools: getFeed");
    assert_eq!(at("resources]").detail, "resources: posts");

    let name = at("name = ");
    assert_eq!((name.kind, name.name.as_str()), ("mcp name", "blog"));
    assert_eq!(name.detail, "MCP resource URIs: cratestack://blog/posts");

    let block = at("mcp {");
    assert_eq!(block.kind, "mcp block");
    assert_eq!(block.docs, vec!["Agent surface.".to_owned()]);
}

#[test]
fn mcp_rules_are_reported_as_diagnostics_at_the_attribute() {
    let text = SCHEMA.replace("  @allow(auth() != null)\n", "");
    let (schema, diagnostics) = analyze_document(&uri(), &text);
    assert!(schema.is_none());
    let diagnostic = diagnostics
        .iter()
        .find(|d| {
            d.message
                .contains("exposes a procedure with no `@allow(...)`")
        })
        .unwrap_or_else(|| panic!("MCP rule missing from {diagnostics:?}"));
    let start = text.find("@mcp(tool").expect("attribute");
    let end = start + "@mcp(tool, description: \"Newest first.\")".len();
    assert_eq!(diagnostic.range, range_from_offsets(&text, start, end));
}

#[test]
fn mcp_syntax_errors_are_reported_as_diagnostics() {
    let text = SCHEMA.replace("[tools, resources]", "[procedures, resources]");
    let (_, diagnostics) = analyze_document(&uri(), &text);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].message.contains("renamed to `tools`"));
    let start = text.find("procedures").expect("element");
    let expected = range_from_offsets(&text, start, start + "procedures".len());
    assert_eq!(diagnostics[0].range, expected);
}

/// `name` is a DNS label (cratestack#1040): the completion popup states the
/// rule, and a name that breaks it is a diagnostic on the entry that names
/// the broken rule.
#[test]
fn the_mcp_name_is_hinted_and_checked_as_a_dns_label() {
    let items = completion_items(None);
    let name = items.iter().find(|item| item.label == "name = \"...\"");
    let detail = name.and_then(|item| item.detail.as_deref()).unwrap_or("");
    assert!(detail.contains("a DNS label"), "{detail}");
    assert!(detail.contains("1-63 characters"), "{detail}");

    for (value, rule) in [
        ("-blog", "must not start or end with `-`"),
        (&"a".repeat(64), "must be 1 to 63 characters"),
    ] {
        let text = SCHEMA.replace("name = \"blog\"", &format!("name = \"{value}\""));
        let (_, diagnostics) = analyze_document(&uri(), &text);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains(rule), "{diagnostics:?}");
        let start = text.find("name = ").expect("entry");
        let end = start + format!("name = \"{value}\"").len();
        assert_eq!(diagnostics[0].range, range_from_offsets(&text, start, end));
    }
}

/// Before cratestack#1036 `@@mcp(...)` was a raw model attribute and got the
/// decorator token every `@@...` gets. Moving it into `Model.mcp` took it out
/// of the list `semantic_tokens` walks; it must still be coloured.
#[test]
fn model_mcp_keeps_its_decorator_token() {
    let (schema, _) = analyze_document(&uri(), SCHEMA);
    let schema = schema.expect("valid MCP schema");
    let at = offset_to_position(SCHEMA, SCHEMA.find("@@mcp(").expect("attribute"));
    let (mut line, mut character) = (0u32, 0u32);
    let decorated = semantic_tokens(SCHEMA, &schema).iter().any(|token| {
        line += token.delta_line;
        character = if token.delta_line == 0 {
            character + token.delta_start
        } else {
            token.delta_start
        };
        (line, character, token.length) == (at.line, at.character, "@@mcp".len() as u32)
            && LEGEND[token.token_type as usize] == SemanticTokenType::DECORATOR
    });
    assert!(decorated, "`@@mcp` must be a decorator token");
}
