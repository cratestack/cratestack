//! `query` attributes the generator did not read (GHSA-69g4-xvcm-vm2j).
//! The query validator used to name an attribute by everything before its
//! first `(`, trimmed, so `@deny (…)`, `@deny\t(…)` and `@deny(…) banned`
//! counted as `@deny` while `cratestack-macros` dropped them: a caller the
//! written `@deny` refuses got rows back on a live database.

use crate::parse_schema;

const SQL: &str = "@@sql(\"SELECT $1::bigint AS \\\"total\\\"\")";

/// `totals` with a SQL body, `@allow(auth() != null)` and then `line`.
fn totals(line: &str) -> String {
    format!(
        "auth SessionUser {{\n  id Int\n  role String\n}}\n\n\
         type Summary {{\n  total Int\n}}\n\n\
         query totals(userId: Int): Summary\n  {SQL}\n  @allow(auth() != null)\n  {line}\n"
    )
}

const REFUSED: &[(&str, &str)] = &[
    (
        "@deny (hasRole(\"banned\"))",
        "`@deny` must be followed directly by its `(`",
    ),
    (
        "@deny\t(hasRole(\"banned\"))",
        "`@deny` must be followed directly by its `(`",
    ),
    (
        "@deny(hasRole(\"banned\")) banned",
        "`banned` after the closing `)` of `@deny`",
    ),
    (
        "@deny(hasRole(\"banned\"));",
        "`;` after the closing `)` of `@deny`",
    ),
    (
        "@Deny(hasRole(\"banned\"))",
        "unsupported attribute `@Deny` on a query (did you mean `@deny`?)",
    ),
    ("@deny", "`@deny` takes an argument list"),
    (
        "@allow (auth() != null)",
        "`@allow` must be followed directly by its `(`",
    ),
    (
        "@ allow(auth() != null)",
        "no space is allowed between `@` and the attribute name",
    ),
    (
        "@@sql (\"SELECT 1\")",
        "`@@sql` must be followed directly by its `(`",
    ),
    (
        "@authorize(Account, read, userId)",
        "unsupported attribute `@authorize` on a query",
    ),
];

#[test]
fn every_spelling_the_generator_skipped_is_refused() {
    for (line, needle) in REFUSED {
        let message = parse_schema(&totals(line))
            .err()
            .unwrap_or_else(|| panic!("`{line}` must be refused, but the schema parsed"))
            .to_string();
        assert!(message.contains(needle), "`{line}`: {message}");
    }
}

#[test]
fn a_policy_sharing_a_line_or_followed_by_a_comment_is_read() {
    for (line, expected) in [
        (
            "@deny(hasRole(\"banned\")) // banned users see nothing",
            &["@deny(hasRole(\"banned\"))"][..],
        ),
        (
            "@deny(hasRole(\"banned\")) @deny(hasRole(\"frozen\"))",
            &["@deny(hasRole(\"banned\"))", "@deny(hasRole(\"frozen\"))"],
        ),
    ] {
        let schema = parse_schema(&totals(line))
            .unwrap_or_else(|error| panic!("`{line}` should parse: {error}"));
        let raws = schema.queries[0].attributes[2..]
            .iter()
            .map(|a| a.raw.as_str())
            .collect::<Vec<_>>();
        assert_eq!(raws, expected, "{line}");
    }
}

// The SQL line itself: a trailing comment and a policy after the body.
#[test]
fn a_sql_body_line_keeps_its_body_and_reads_what_follows_it() {
    let source = "type Summary {\n  total Int\n}\n\n\
         query totals(userId: Int): Summary\n  \
         @@sql(\"SELECT $1::bigint AS \\\"total\\\" -- http://x\") @deny(auth() == null) // c\n  \
         @allow(true)\n\n\
         query multi(userId: Int): Summary\n  @@sql(\"\"\"\n    SELECT $1::bigint AS \"total\" -- a // b @x\n  \
         \"\"\") @deny(auth() == null) // after\n  @allow(true)\n";
    let schema = parse_schema(source).unwrap_or_else(|error| panic!("should parse: {error}"));
    let [one, multi] = &schema.queries[..] else {
        panic!("two queries")
    };
    assert_eq!(
        one.sql().as_deref(),
        Some("SELECT $1::bigint AS \"total\" -- http://x")
    );
    assert_eq!(
        multi.sql().as_deref().map(str::trim),
        Some("SELECT $1::bigint AS \"total\" -- a // b @x")
    );
    for query in [one, multi] {
        let raws = query.attributes[1..]
            .iter()
            .map(|a| a.raw.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            raws,
            ["@deny(auth() == null)", "@allow(true)"],
            "{}",
            query.name
        );
    }
}
