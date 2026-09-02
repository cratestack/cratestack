//! Coverage for [`super::scan_sql_placeholders`].
//!
//! The cases that matter most are the *false positives*: a `$N`-looking
//! token inside text does not merely weaken a check, it makes the
//! validator refuse to compile valid SQL. Each of those is written
//! against a body a real author could plausibly write.

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
    // `$12` is parameter twelve, not `$1` followed by a stray `2` — the
    // difference decides whether a 12-argument query validates.
    assert_eq!(scan("SELECT $12"), vec![12]);
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
    // `$0` is not a legal Postgres parameter, so the validator has to see
    // it to reject it — the scanner must not silently drop it.
    assert_eq!(scan("SELECT $0"), vec![0]);
}

#[test]
fn skips_a_dollar_quoted_body_with_an_empty_tag() {
    // cratestack#867 review: this returned `[1]` before, so
    // `WHERE note = $$see $1$$` with zero declared parameters was
    // rejected as an out-of-range reference.
    assert_eq!(scan("SELECT $$1$$"), Vec::<u32>::new());
}

#[test]
fn skips_a_dollar_quoted_body_with_a_named_tag() {
    // Also from the review: returned `[99]`.
    assert_eq!(scan("SELECT $q$99 bottles$q$"), Vec::<u32>::new());
}

#[test]
fn still_finds_parameters_around_a_dollar_quoted_body() {
    // The skip must be a span, not a bail-out.
    assert_eq!(scan("SELECT $1, $tag$ inner $9 $tag$, $2"), vec![1, 2]);
}

#[test]
fn a_dollar_quote_tag_may_contain_digits_after_its_first_character() {
    assert_eq!(scan("SELECT $q1$ $7 $q1$"), Vec::<u32>::new());
}

#[test]
fn an_unterminated_dollar_quote_swallows_the_rest() {
    // Fail closed: the body is Postgres's error to report, and scanning
    // its tail could only invent an error of our own.
    assert_eq!(scan("SELECT $$ oops $3"), Vec::<u32>::new());
}

#[test]
fn a_lone_dollar_is_not_a_delimiter_and_does_not_hide_what_follows() {
    assert_eq!(scan("SELECT '$' , $1"), vec![1]);
}

#[test]
fn skips_string_literals_including_doubled_quote_escapes() {
    // `'it''s $9'` is ONE literal. Terminating at the doubled quote would
    // leave ` $9'` scanned as live SQL.
    assert_eq!(scan("SELECT 'it''s $9 here', $1"), vec![1]);
}

#[test]
fn skips_quoted_identifiers() {
    // A `query` result column is aliased with a quoted identifier, so a
    // column legitimately named `cost $2` must not read as a parameter.
    assert_eq!(scan(r#"SELECT x AS "cost $2" WHERE a = $1"#), vec![1]);
}

#[test]
fn skips_line_comments() {
    assert_eq!(scan("SELECT $1 -- was $5 before\n, $2"), vec![1, 2]);
}

#[test]
fn skips_block_comments_and_honours_postgres_nesting() {
    // Postgres block comments nest, unlike C's. Stopping at the first
    // `*/` would leave `WHERE x = $9 */` scanned as live SQL.
    assert_eq!(
        scan("SELECT $1 /* outer /* inner */ WHERE x = $9 */ , $2"),
        vec![1, 2]
    );
}

#[test]
fn the_motivating_query_still_scans_to_one_and_two() {
    // End-to-end guard: the lexer additions must not change the answer
    // for the body this whole feature exists to run.
    let sql = r#"
        SELECT
          COALESCE(SUM(discount), 0)::bigint AS "total",
          COALESCE(SUM(discount) FILTER (WHERE created_at >= $2), 0)::bigint AS "thisMonth"
        FROM loyalty_fee_events
        WHERE user_id = $1
    "#;
    assert_eq!(scan(sql), vec![1, 2]);
}

#[test]
fn skips_an_escape_string_whose_quote_is_backslash_escaped() {
    // cratestack#870 review round 2, measured end-to-end: this was
    // ACCEPTED with one declared parameter, because `\'` read as "literal
    // closed" and the following `'` reopened one, swallowing `$5`. A false
    // ACCEPT — the out-of-range reference reaches Postgres instead of
    // being refused at build time.
    assert_eq!(scan(r"SELECT 1 WHERE note = E'\'' AND x = $5"), vec![5]);
}

#[test]
fn an_escape_string_ending_in_an_escaped_backslash_closes_where_it_should() {
    // `E'a\\'` is a complete literal containing one backslash. Treating
    // the second `\` as escaping the closing quote would swallow `$1`.
    assert_eq!(scan(r"SELECT E'a\\', $1"), vec![1]);
}

#[test]
fn a_plain_string_literal_does_not_honour_backslash_escapes() {
    // `standard_conforming_strings` has been on by default since Postgres
    // 9.1, so `'a\'` is a complete literal and the `$1` after it is live
    // SQL. Unchanged by the E-string handling — pinned so it stays that
    // way.
    assert_eq!(scan(r"SELECT 'a\', $1"), vec![1]);
}

#[test]
fn doubled_quotes_still_escape_inside_an_escape_string() {
    // `E'…'` honours BOTH `\'` and `''`; handling only the new rule would
    // regress the old one for E-strings specifically.
    assert_eq!(scan(r"SELECT E'it''s $9', $1"), vec![1]);
}

#[test]
fn an_identifier_ending_in_e_does_not_open_an_escape_string() {
    // The lookbehind that stops `some_e'…'` being read as an E-string —
    // if it were, the literal's closing rules would change under it.
    assert_eq!(scan(r"SELECT some_e'\', $1"), vec![1]);
}
