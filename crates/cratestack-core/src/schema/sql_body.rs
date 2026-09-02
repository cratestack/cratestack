//! Shared extractor for the raw SQL text inside an `@@…_sql(…)` block
//! attribute.
//!
//! Lifted out of [`super::view`] when the `query` block (cratestack#867,
//! design `docs/design/declarative-custom-query.md`) needed the identical
//! parse: both constructs capture an author-written SQL string verbatim
//! from an attribute, and both accept the multi-line `"""…"""` form the
//! parser stitches together. Keeping one copy is what stops the two
//! constructs quietly disagreeing about, say, whether a `\"` survives —
//! they must not, because the same `collect_attribute_text` in
//! `cratestack-parser` feeds both.

/// Extract the SQL body from an attribute like `@@server_sql("…")`.
///
/// Accepts both `"single-line"` and `"""multi-line"""` strings. The outer
/// quotes are stripped. Returns `None` when the attribute is present but
/// its argument is not a quoted string at all (`@@sql(SELECT 1)`, a bare
/// `@@sql`, or a line carrying a second attribute after the closing
/// paren) — callers must treat that as an **error** rather than as "no
/// SQL", or a malformed body silently becomes an empty one.
///
/// **Escaping differs between the two forms, deliberately.**
///
/// - `"""…"""` is verbatim: nothing is unescaped, because nothing needs
///   to be. A `"` inside it is just a `"`. This is the form to use for
///   any SQL containing quoting, and the one every multi-line body uses.
/// - `"…"` unescapes `\"` to `"` and `\\` to a single `\`, because inside
///   a double-quoted string those are the only two spellings that cannot
///   be written literally. Before cratestack#867's review this form
///   passed `\"` through to Postgres as a literal backslash-quote, which
///   is not valid SQL — a schema writing `AS \"total\"` (the spelling a
///   `query` result column needs) produced a syntax error at first
///   execution with nothing pointing at the cause.
///
/// Any other `\x` sequence passes through unchanged, so a regex like
/// `\d` keeps working without doubling. That asymmetry is exactly why
/// `"""…"""` is the form to prefer: it has no rules to remember.
pub(crate) fn extract_sql_body(raw: &str, prefix: &str) -> Option<String> {
    let after_prefix = raw.strip_prefix(prefix)?.trim_start();
    let inside_parens = after_prefix
        .strip_prefix('(')?
        .rsplit_once(')')
        .map(|(body, _tail)| body)?
        .trim();
    if let Some(rest) = inside_parens.strip_prefix("\"\"\"") {
        rest.strip_suffix("\"\"\"").map(str::to_owned)
    } else if let Some(rest) = inside_parens.strip_prefix('"') {
        rest.strip_suffix('"').map(unescape_double_quoted)
    } else {
        None
    }
}

/// `\"` -> `"`, `\\` -> `\`, everything else verbatim. See
/// [`extract_sql_body`]'s doc for why the last clause is not an oversight.
fn unescape_double_quoted(body: &str) -> String {
    if !body.contains('\\') {
        return body.to_owned();
    }
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            // Unknown escape: keep both characters, so `\d` stays `\d`.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            // Trailing lone backslash.
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::extract_sql_body;

    #[test]
    fn strips_the_outer_quotes_of_a_single_line_body() {
        assert_eq!(
            extract_sql_body(r#"@@sql("SELECT 1")"#, "@@sql").as_deref(),
            Some("SELECT 1"),
        );
    }

    #[test]
    fn unescapes_quotes_in_a_single_line_body() {
        // The case cratestack#867's review found: a `query` result column
        // must be aliased `AS "total"`, and on one line that has to be
        // written `\"total\"`. Passing the backslashes through produced a
        // Postgres syntax error at first execution.
        assert_eq!(
            extract_sql_body(r#"@@sql("SELECT 1 AS \"total\"")"#, "@@sql").as_deref(),
            Some(r#"SELECT 1 AS "total""#),
        );
    }

    #[test]
    fn unescapes_a_doubled_backslash_but_leaves_other_escapes_alone() {
        assert_eq!(
            extract_sql_body(r#"@@sql("a \\ b \d c")"#, "@@sql").as_deref(),
            Some(r"a \ b \d c"),
        );
    }

    #[test]
    fn leaves_a_triple_quoted_body_verbatim() {
        // No unescaping here: `"""…"""` needs none, and applying it would
        // corrupt a regex or a Postgres escape-string constant.
        assert_eq!(
            extract_sql_body("@@sql(\"\"\"SELECT 'a\\d' AS \"x\"\"\"\")", "@@sql").as_deref(),
            Some("SELECT 'a\\d' AS \"x\""),
        );
    }

    #[test]
    fn rejects_an_unquoted_argument() {
        // Must be `None`, not `Some("")`: the caller turns this into a
        // schema error, and returning an empty body would instead compile
        // a query whose SQL is the empty string.
        assert_eq!(extract_sql_body("@@sql(SELECT 1)", "@@sql"), None);
    }

    #[test]
    fn rejects_a_bare_attribute_with_no_parentheses() {
        assert_eq!(extract_sql_body("@@sql", "@@sql"), None);
    }

    #[test]
    fn rejects_a_line_that_carries_a_second_attribute_after_the_body() {
        // `rsplit_once(')')` takes the LAST paren, so this yields
        // `"SELECT 1") @allow(auth() != null` and fails the closing-quote
        // check. Correct, but only by accident, so it is pinned.
        assert_eq!(
            extract_sql_body(r#"@@sql("SELECT 1") @allow(auth() != null)"#, "@@sql"),
            None,
        );
    }
}
