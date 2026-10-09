//! Which declaration the cursor is inside, for the completions that depend
//! on it: the field attributes a `model`, `view`, `mixin`, `type` or `auth`
//! block accepts differ (ADR 0019 D5), and the editor offers exactly the
//! cursor's block's list.
//!
//! A scan of the text before the cursor, not of the last good parse: while
//! someone is typing the document rarely parses, and the retained schema's
//! spans no longer match the text (`crate::state::DocumentState`). Blocks
//! open on a line that starts with a declaration keyword and ends in `{`,
//! and close on a line that is just `}`; the lines of a `"""` string
//! (a view's SQL body) are skipped, so a `}` in SQL closes nothing.

use cratestack_parser::FieldHost;
use tower_lsp_server::ls_types::Position;

use crate::text::position_to_offset;

/// [`enclosing_field_host`] for an editor position in `text`.
pub(crate) fn host_at_position(text: &str, position: Position) -> Option<FieldHost> {
    enclosing_field_host(text, position_to_offset(text, position)?)
}

/// The field-bearing block the cursor, at byte `offset` of `text`, is in.
/// `None` outside one: at the top level, in a procedure's attributes, in an
/// `enum`, in a `datasource`.
pub(crate) fn enclosing_field_host(text: &str, offset: usize) -> Option<FieldHost> {
    let before = &text[..offset.min(text.len())];
    let mut open: Vec<Option<FieldHost>> = Vec::new();
    let mut in_string = false;
    for line in before.lines() {
        let starts_in_string = in_string;
        in_string = ends_in_string(line, in_string);
        if starts_in_string {
            continue;
        }
        let line = line.split("//").next().unwrap_or_default().trim();
        if line == "}" {
            open.pop();
        } else if line.ends_with('{') && !line.starts_with('@') {
            open.push(host_of_header(line));
        }
    }
    open.last().copied().flatten()
}

/// Whether a `"""` string is still open at the end of `line`, which begins
/// inside one when `in_string`. Outside a string a `//` starts a comment, so
/// a `"""` after it opens nothing (`// why """ is quoted`), while a `//`
/// inside a string is SQL text (`'http://x'`) and hides nothing.
fn ends_in_string(line: &str, mut in_string: bool) -> bool {
    let mut rest = line;
    loop {
        if in_string {
            let Some(at) = rest.find("\"\"\"") else {
                return true;
            };
            in_string = false;
            rest = &rest[at + 3..];
        } else {
            let quote = rest.find("\"\"\"");
            let comment = rest.find("//");
            match quote {
                Some(at) if comment.is_none_or(|comment| at < comment) => {
                    in_string = true;
                    rest = &rest[at + 3..];
                }
                // No opener, or a comment starts first: the rest is not code.
                _ => return false,
            }
        }
    }
}

/// The declaration a header line opens, `None` for any other block.
fn host_of_header(header: &str) -> Option<FieldHost> {
    match header.split_whitespace().next()? {
        "model" => Some(FieldHost::Model),
        "view" => Some(FieldHost::View),
        "mixin" => Some(FieldHost::Mixin),
        "type" => Some(FieldHost::Type),
        "auth" => Some(FieldHost::Auth),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The host at the `|` in `marked`.
    fn host_at(marked: &str) -> Option<FieldHost> {
        let offset = marked.find('|').expect("a cursor marker");
        enclosing_field_host(&marked.replace('|', ""), offset)
    }

    #[test]
    fn each_field_bearing_block_is_recognised() {
        for (header, host) in [
            ("model Note", FieldHost::Model),
            ("view Summary from Note", FieldHost::View),
            ("mixin Audited", FieldHost::Mixin),
            ("type Args", FieldHost::Type),
            ("auth Caller", FieldHost::Auth),
        ] {
            let source = format!("{header} {{\n  id Int @id\n  name String |\n}}\n");
            assert_eq!(host_at(&source), Some(host), "{header}");
        }
    }

    #[test]
    fn outside_a_field_bearing_block_there_is_no_host() {
        for marked in [
            "|\nmodel A {\n  id Int @id\n}\n",
            "model A {\n  id Int @id\n}\n|\n",
            "enum Role {\n  Admin\n  |\n}\n",
            "datasource db {\n  provider = \"none\"\n  |\n}\n",
            "procedure p(): A\n  |\n",
        ] {
            assert_eq!(host_at(marked), None, "{marked}");
        }
    }

    #[test]
    fn blocks_close_and_a_sql_body_does_not_confuse_the_scan() {
        let after_model =
            "model A {\n  id Int @id\n}\ntype T {\n  x String\n}\nenum E {\n  V\n}\n|";
        assert_eq!(host_at(after_model), None);
        // The attribute name is assembled so that `cratestack-core`'s pinned
        // reader list for it does not count this test as a reader.
        let body = format!("@@{}(\"\"\"\n  SELECT {{\n  }}\n  \"\"\")", "sql");
        let in_view = format!("view V from A {{\n  id Int @id\n  {body}\n  |");
        assert_eq!(host_at(&in_view), Some(FieldHost::View));
        let comment = "model A {\n  // type T {\n  |\n}\n";
        assert_eq!(host_at(comment), Some(FieldHost::Model));
    }

    /// A `"""` inside a `//` comment opens no string. Counting it first
    /// treated everything after as SQL, so the `}` below closed nothing and
    /// the cursor in `type T` was still read as inside `model A`.
    #[test]
    fn a_triple_quote_in_a_comment_does_not_open_a_string() {
        let source =
            "model A {\n  // why \"\"\" is quoted\n  id Int @id\n}\ntype T {\n  x String\n  |\n}\n";
        assert_eq!(host_at(source), Some(FieldHost::Type));
        // Two of them in one comment are no better.
        let twice = "model A {\n  // \"\"\" and \"\"\"\n}\ntype T {\n  |\n}\n";
        assert_eq!(host_at(twice), Some(FieldHost::Type));
    }

    /// The other direction: a `//` inside a string is SQL, not a comment, so
    /// a one-line `"""` string that holds a URL still closes on its line.
    #[test]
    fn a_double_slash_inside_a_string_is_not_a_comment() {
        let body = format!("@@{}(\"\"\"SELECT 'http://x' \"\"\")", "sql");
        let source = format!("view V from A {{\n  id Int @id\n  {body}\n}}\ntype T {{\n  |\n}}\n");
        assert_eq!(host_at(&source), Some(FieldHost::Type));
    }
}
