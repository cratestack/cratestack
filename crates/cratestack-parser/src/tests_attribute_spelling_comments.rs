//! A trailing `//` comment on an attribute line (GHSA-69g4-xvcm-vm2j,
//! maintainer decision 1): outside a string it starts a comment, on every
//! attribute line — field, `@@`, procedure, query — and the parser drops
//! it before building [`Attribute::raw`](cratestack_core::Attribute). An
//! `@` inside the comment is therefore never another attribute, and an
//! attribute followed by a note is still read.

use super::parse_schema;

fn view_with_body(line: &str) -> String {
    format!(
        "model A {{\n  id Int @id\n}}\n\
         view V from A {{\n  id Int @id @from(A.id)\n  {line}\n}}\n"
    )
}

#[test]
fn a_comment_after_a_sql_body_is_not_another_attribute() {
    for (line, reader) in [
        ("@@sql(\"SELECT id FROM a\") // owned by @ops", "server"),
        ("@@sql(\"SELECT id FROM a\") // it's @ops's", "server"),
        (
            "@@server_sql(\"SELECT id FROM a\") // see @ops (#1)",
            "server",
        ),
        (
            "@@embedded_sql(\"SELECT id FROM a\") // see @ops",
            "embedded",
        ),
        (
            "@@sql(\"\"\"\n    SELECT id FROM a\n  \"\"\") // owned by @ops",
            "server",
        ),
    ] {
        let schema = parse_schema(&view_with_body(line))
            .unwrap_or_else(|error| panic!("`{line}` should parse: {error}"));
        let view = &schema.views[0];
        let body = match reader {
            "server" => view.server_sql(),
            _ => view.embedded_sql(),
        };
        assert_eq!(
            body.as_deref().map(str::trim),
            Some("SELECT id FROM a"),
            "{line}"
        );
        let raw = &view.attributes[0].raw;
        assert!(!raw.contains("//"), "the comment is not part of `{raw}`");
    }
}

#[test]
fn a_double_slash_inside_a_sql_string_is_sql() {
    for (line, sql) in [
        (
            "@@sql(\"SELECT id FROM a WHERE u = 'http://x'\") // c",
            "SELECT id FROM a WHERE u = 'http://x'",
        ),
        (
            "@@sql(\"\"\"\n  SELECT id FROM a -- see http://x // y\n  \"\"\")",
            "SELECT id FROM a -- see http://x // y",
        ),
        (
            "@@sql(\"\"\"\n  SELECT id, '\"' AS q FROM a -- // \"\n\"\"\") // after",
            "SELECT id, '\"' AS q FROM a -- // \"",
        ),
    ] {
        let schema = parse_schema(&view_with_body(line))
            .unwrap_or_else(|error| panic!("`{line}` should parse: {error}"));
        assert_eq!(
            schema.views[0].server_sql().as_deref().map(str::trim),
            Some(sql)
        );
    }
}

// Before, the `@` in the comment was refused as a second attribute
// (and a comment without one was kept in the raw text, where the policy
// generator's "ends at `)`" check silently skipped the rule).
#[test]
fn a_comment_after_a_model_attribute_is_dropped() {
    for (line, raw) in [
        (
            "@@allow(\"read\", true) // ask @ops",
            "@@allow(\"read\", true)",
        ),
        (
            "@@allow(\"read\", true) // it's @ops's",
            "@@allow(\"read\", true)",
        ),
        (
            "@@deny(\"read\", auth() == null)// mail ops@x.io",
            "@@deny(\"read\", auth() == null)",
        ),
        ("@@audit // keep for a year", "@@audit"),
    ] {
        let source = format!("model Doc {{\n  id Int @id\n  {line}\n}}\n");
        let schema =
            parse_schema(&source).unwrap_or_else(|error| panic!("`{line}` should parse: {error}"));
        let attribute = &schema.models[0].attributes[0];
        assert_eq!(attribute.raw, raw);
        assert_eq!(&source[attribute.span.start..attribute.span.end], raw);
    }
}

// The investigation's field case: the `@readonly` in the comment used to
// become a live attribute of the field.
#[test]
fn an_attribute_named_in_a_field_comment_is_not_applied() {
    let source = "model Doc {\n  id Int @id\n  name String @unique // not @readonly\n  \
                  site String @default(\"http://x\") // @server_only later\n}\n";
    let schema = parse_schema(source).expect("comments after fields parse");
    let raws = |index: usize| {
        schema.models[0].fields[index]
            .attributes
            .iter()
            .map(|attribute| attribute.raw.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(raws(1), ["@unique"]);
    assert_eq!(raws(2), ["@default(\"http://x\")"]);
}

#[test]
fn a_comment_after_a_procedure_or_query_attribute_is_dropped() {
    let source = "type R {\n  n Int\n}\n\n\
                  procedure p(n: Int): R\n  @allow(auth() != null) // any user\n  \
                  @deny(auth().url == \"http://x\")//x\n\n\
                  query q(n: Int): R\n  @@sql(\"SELECT $1::int AS n\") // body\n  \
                  @allow(true) // anyone\n";
    let schema = parse_schema(source).expect("comments after attributes parse");
    let raws = |attributes: &[cratestack_core::Attribute]| {
        attributes.iter().map(|a| a.raw.clone()).collect::<Vec<_>>()
    };
    assert_eq!(
        raws(&schema.procedures[0].attributes),
        [
            "@allow(auth() != null)",
            "@deny(auth().url == \"http://x\")"
        ]
    );
    assert_eq!(
        raws(&schema.queries[0].attributes),
        ["@@sql(\"SELECT $1::int AS n\")", "@allow(true)"]
    );
}
