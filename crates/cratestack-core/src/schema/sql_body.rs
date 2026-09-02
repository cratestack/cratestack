//! Shared extractor for the raw SQL text inside an `@@…_sql(…)` block
//! attribute.
//!
//! Lifted out of [`super::view`] when the `query` block (cratestack#867,
//! design `docs/design/declarative-custom-query.md`) needed the identical
//! parse: both constructs capture an author-written SQL string verbatim
//! from an attribute, and both accept the multi-line `"""…"""` form the
//! parser stitches together. Keeping one copy is what stops the two
//! constructs quietly disagreeing about, say, whether trailing whitespace
//! inside the quotes survives — they must not, because the same
//! `collect_attribute_text` in `cratestack-parser` feeds both.

/// Extract the SQL body from an attribute like `@@server_sql("…")`.
/// Accepts both `"single-line"` and `"""multi-line"""` strings. The
/// outer quotes are stripped; embedded newlines and quotes are
/// preserved verbatim.
pub(crate) fn extract_sql_body<'a>(raw: &'a str, prefix: &str) -> Option<&'a str> {
    let after_prefix = raw.strip_prefix(prefix)?.trim_start();
    let inside_parens = after_prefix
        .strip_prefix('(')?
        .rsplit_once(')')
        .map(|(body, _tail)| body)?
        .trim();
    if let Some(rest) = inside_parens.strip_prefix("\"\"\"") {
        rest.strip_suffix("\"\"\"")
    } else if let Some(rest) = inside_parens.strip_prefix('"') {
        rest.strip_suffix('"')
    } else {
        None
    }
}
