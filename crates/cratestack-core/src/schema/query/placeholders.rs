//! Positional-placeholder scan over a `query` block's raw SQL body.
//!
//! This is a **text scan, not a SQL parser** — deliberately, and the
//! design (`docs/design/declarative-custom-query.md` §2) prices the
//! alternative: knowing a `$N`'s *type* requires preparing the statement
//! against a live catalogue, which would make `cratestack-macros` need a
//! database connection at macro-expansion time for the first time ever.
//! Knowing a `$N`'s *position* needs none of that, and position is all
//! the two checks in `cratestack-parser`'s `validate/queries.rs` need:
//! no reference beyond the declared arg count, and no declared arg left
//! unreferenced.
//!
//! Known and accepted imprecision, stated here rather than discovered
//! later: a `$1` that appears inside a SQL string literal (`'$1'`) or a
//! dollar-quoted body counts as a reference. The consequences are bounded
//! — it can only make the "declared but never referenced" check *more*
//! permissive, never make a real mismatch pass — and recognising the
//! difference is exactly the SQL parsing this construct exists to avoid.

use std::collections::BTreeSet;

/// Every distinct `N` in a `$N` token appearing in `sql`, ascending.
///
/// `$` followed by a maximal run of ASCII digits. `$$`/`$tag$`
/// dollar-quoting delimiters contain no digits directly after the `$` and
/// so are skipped naturally, without needing a special case.
pub fn scan_sql_placeholders(sql: &str) -> BTreeSet<u32> {
    let bytes = sql.as_bytes();
    let mut found = BTreeSet::new();
    let mut index = 0usize;
    while index < bytes.len() {
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
mod tests {
    use super::scan_sql_placeholders;

    fn scan(sql: &str) -> Vec<u32> {
        scan_sql_placeholders(sql).into_iter().collect()
    }

    #[test]
    fn finds_each_distinct_index_once() {
        assert_eq!(
            scan("SELECT * FROM t WHERE a = $1 AND b = $2 AND c = $1"),
            vec![1, 2]
        );
    }

    #[test]
    fn reads_multi_digit_indices_as_one_number() {
        // `$12` is parameter twelve, not `$1` followed by a stray `2` —
        // the difference decides whether a 12-arg query validates.
        assert_eq!(scan("SELECT $12"), vec![12]);
    }

    #[test]
    fn ignores_dollar_quoting_and_bare_dollars() {
        assert_eq!(scan("SELECT $$body$$, $tag$x$tag$, '$'"), Vec::<u32>::new());
    }

    #[test]
    fn ignores_a_cast_that_merely_follows_a_placeholder() {
        // The motivating query's `::bigint` casts sit right next to
        // placeholders; nothing about them may be swallowed.
        assert_eq!(scan("SELECT ($1)::bigint, ($2)::bigint"), vec![1, 2]);
    }

    #[test]
    fn finds_nothing_in_a_parameterless_body() {
        assert_eq!(
            scan("SELECT COUNT(*) FROM t WHERE created_at >= NOW()"),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn does_not_treat_zero_as_absent() {
        // `$0` is not a legal Postgres parameter, so the validator has to
        // see it to reject it — the scanner must not silently drop it.
        assert_eq!(scan("SELECT $0"), vec![0]);
    }
}
