//! Positional-placeholder scan over a `query` block's raw SQL body.
//!
//! This is a **lexical scan, not a SQL parser** — deliberately, and the
//! design (`docs/design/declarative-custom-query.md` §2) prices the
//! alternative: knowing a `$N`'s *type* requires preparing the statement
//! against a live catalogue, which would make `cratestack-macros` need a
//! database connection at macro-expansion time for the first time ever.
//! Knowing a `$N`'s *position* needs none of that, and position is all
//! the two checks in `cratestack-parser`'s `validate/queries.rs` need:
//! no reference beyond the declared parameter count, and no declared
//! parameter left unreferenced.
//!
//! It does step over the spans Postgres itself reads as text — string
//! literals, dollar-quoted bodies, quoted identifiers and comments (see
//! [`skip`]) — because not doing so does not merely miss checks, it
//! **falsely rejects valid SQL**. That regression was real and measured
//! in cratestack#867's review; [`skip`]'s module doc has the numbers and
//! the reasoning for why a lexer is on the right side of the line design
//! §3 draws.

mod skip;

use std::collections::BTreeSet;

/// Every distinct `N` in a `$N` token appearing in `sql` as an actual
/// parameter reference, ascending.
///
/// `$` followed by a maximal run of ASCII digits, outside any string
/// literal, dollar-quoted body, quoted identifier or comment.
pub fn scan_sql_placeholders(sql: &str) -> BTreeSet<u32> {
    let bytes = sql.as_bytes();
    let mut found = BTreeSet::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if let Some(end) = skip::text_span_end(bytes, index) {
            // `text_span_end` also answers "is this `$` a dollar-quote
            // opener?", so a `$` that reaches the next branch is known
            // not to be one.
            index = end.max(index + 1);
            continue;
        }
        if bytes[index] != b'$' {
            index += 1;
            continue;
        }
        let digits_start = index + 1;
        let mut cursor = digits_start;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor == digits_start {
            index += 1;
            continue;
        }
        // A run long enough to overflow `u32` is not a plausible
        // parameter index; treating it as "not a placeholder" is right,
        // and is also what keeps this function total.
        if let Ok(value) = sql[digits_start..cursor].parse::<u32>() {
            found.insert(value);
        }
        index = cursor;
    }
    found
}

#[cfg(test)]
mod tests;
