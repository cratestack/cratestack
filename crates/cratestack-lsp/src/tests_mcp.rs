//! MCP in the editor (cratestack#1036): completion offers the four new
//! words, hover resolves every MCP span the parser records, and the parser's
//! MCP rules arrive as diagnostics pointing at the offending attribute.

use std::str::FromStr;

use tower_lsp_server::ls_types::{CompletionItemKind, Uri};

use crate::analyze::analyze_document;
use crate::completion::completion_items;
use crate::hover::locate_symbol;
use crate::text::range_from_offsets;

const SCHEMA: &str = r#"datasource db {
  provider = "postgresql"
}

/// Agent surface.
mcp {
  expose tools
  expose resources
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
fn completion_offers_the_mcp_attributes_and_expose_lines_with_detail() {
    let items = completion_items(None);
    for label in ["@mcp", "@@mcp", "expose tools", "expose resources"] {
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

    let expose = at("expose tools");
    assert_eq!(
        (expose.kind, expose.name.as_str()),
        ("mcp setting", "expose tools")
    );
    assert_eq!(expose.detail, "tools: getFeed");
    assert_eq!(at("expose resources").detail, "resources: posts");

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
    let text = SCHEMA.replace("expose tools", "expose procedures");
    let (_, diagnostics) = analyze_document(&uri(), &text);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].message.contains("renamed to `expose tools`"));
    let start = text.find("expose procedures").expect("line");
    let expected = range_from_offsets(&text, start, start + "expose procedures".len());
    assert_eq!(diagnostics[0].range, expected);
}
