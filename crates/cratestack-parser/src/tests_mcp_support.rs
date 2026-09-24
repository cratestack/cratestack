//! Shared fixture and helpers for the MCP suites (`tests_mcp_parse`,
//! `tests_mcp_rules`, `tests_mcp_placement`; cratestack#1036).
//!
//! Every rejection test derives its schema from [`VALID`] by one edit, so
//! the schema is valid except for the one thing under test. That is what
//! makes the per-rule mutation evidence meaningful: with the rule's check
//! deleted, the edited schema parses cleanly and the test fails, rather than
//! passing on some unrelated error.

use crate::parse_schema_diagnostics;

pub(crate) const VALID: &str = r#"datasource db {
  provider = "postgresql"
}

/// Agent-facing surface.
mcp {
  expose tools
  expose resources
}

type FeedArgs {
  limit Int
}

model Post {
  id Int @id
  title String

  @@allow("read", true)
  @@mcp(resource: "posts", max_page_size: 20)
}

procedure getFeed(args: FeedArgs): Post[]
  @allow(auth() != null)
  @mcp(tool)

mutation procedure publishPost(args: FeedArgs): Post
  @allow(auth() != null)
  @mcp(tool: "publish_post", description: "Publish a draft, then notify: now.")
"#;

/// [`VALID`] with `from` replaced by `to`; panics if `from` is absent, so a
/// fixture edit can never silently become a no-op.
pub(crate) fn edit(from: &str, to: &str) -> String {
    assert!(VALID.contains(from), "fixture does not contain {from:?}");
    VALID.replacen(from, to, 1)
}

/// Asserts `source` is rejected with a message containing `needle`, and
/// returns the source text that diagnostic's span covers — so each test can
/// also pin *where* the error points, not only that it happened.
pub(crate) fn rejected<'a>(source: &'a str, needle: &str) -> &'a str {
    let (schema, errors) = parse_schema_diagnostics("mcp.cstack", source);
    assert!(schema.is_none(), "expected a rejection for {needle:?}");
    let error = errors
        .iter()
        .find(|error| error.message().contains(needle))
        .unwrap_or_else(|| {
            let messages = errors.iter().map(|e| e.message()).collect::<Vec<_>>();
            panic!("no diagnostic contains {needle:?}; got {messages:#?}")
        });
    &source[error.span()]
}

/// The one-and-only syntax error for `source` (parse errors are not
/// collected — parsing has no recovery).
pub(crate) fn syntax_error(source: &str) -> String {
    let (schema, errors) = parse_schema_diagnostics("mcp.cstack", source);
    assert!(schema.is_none(), "expected a syntax error");
    assert_eq!(errors.len(), 1, "a parse error is reported alone");
    errors[0].message().to_owned()
}
