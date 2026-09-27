//! The closed `@@` list of models and views (GHSA-69g4-xvcm-vm2j,
//! maintainer decision 2; `validate::block_attributes`), and a view's
//! single-quoted `@@allow` (decision 4).

use crate::parse_schema;

const HEAD: &str =
    "datasource db {\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}\n\n";

fn model_with(line: &str) -> String {
    format!("{HEAD}model Doc {{\n  id Int @id\n  a Int\n  b Int\n  {line}\n}}\n")
}

fn view_with(line: &str) -> String {
    format!(
        "{HEAD}model Doc {{\n  id Int @id\n}}\n\nview V from Doc {{\n  id Int @id\n  \
         @@server_sql(\"SELECT id FROM docs\")\n  {line}\n}}\n"
    )
}

#[track_caller]
fn refused(source: &str, needles: &[&str]) {
    let message = parse_schema(source)
        .err()
        .unwrap_or_else(|| panic!("must be refused, but parsed:\n{source}"))
        .to_string();
    for needle in needles {
        assert!(message.contains(needle), "missing {needle:?}: {message}");
    }
}

#[test]
fn every_listed_model_attribute_is_accepted() {
    let source = format!(
        "{HEAD}transport rpc\n\nmodel Doc {{\n  id Int @id\n  a Int\n  b Int\n  \
         @@allow(\"read\", true)\n  @@deny('update', false)\n  @@emit(created)\n  @@paged\n  \
         @@audit\n  @@soft_delete\n  @@retain(days: 30)\n  @@subscribe\n  @@unique([a, b])\n  \
         @@index([a])\n  @@internal(\"delete\")\n  @@rename(from = \"old_docs\")\n}}\n\n\
         model Pair {{\n  a Int\n  b Int\n  @@id([a, b])\n}}\n"
    );
    let schema = parse_schema(&source).unwrap_or_else(|error| panic!("should parse: {error}"));
    assert_eq!(schema.models[0].attributes.len(), 12);
}

#[test]
fn every_listed_view_attribute_is_accepted() {
    for lines in [
        "@@materialized\n  @@allow(\"read\", true)\n  @@deny(\"read\", false)",
        "@@no_unique\n  @@embedded_sql(\"SELECT id FROM docs\")",
    ] {
        let source = view_with(lines);
        parse_schema(&source).unwrap_or_else(|error| panic!("{lines}: {error}"));
    }
    let sql = format!(
        "{HEAD}model Doc {{\n  id Int @id\n}}\n\nview V from Doc {{\n  id Int @id\n  \
         @@sql(\"\"\"\n    SELECT id FROM docs\n  \"\"\")\n}}\n"
    );
    parse_schema(&sql).unwrap_or_else(|error| panic!("{error}"));
}

/// Before, each of these checked as `schema OK` and did nothing.
#[test]
fn a_model_attribute_nothing_reads_is_refused() {
    for (line, name) in [
        ("@@map(\"docs\")", "@@map"),
        ("@@check(\"a > 0\")", "@@check"),
        ("@@unique_per_tenant(a)", "@@unique_per_tenant"),
        ("@@index_hint(a)", "@@index_hint"),
        ("@@materialized", "@@materialized"),
        ("@@server_sql(\"SELECT 1\")", "@@server_sql"),
        ("@@auditt", "@@auditt"),
    ] {
        refused(
            &model_with(line),
            &[
                &format!("unsupported attribute `{name}` on a model"),
                "@@rename",
            ],
        );
    }
    refused(
        &model_with("@@softdelete"),
        &["(did you mean `@@soft_delete`?)"],
    );
    refused(
        &model_with("@@emit (created)"),
        &["`@@emit` must be followed directly by its `(`"],
    );
    refused(
        &model_with("@@rename(from = \"old\") forever"),
        &["`forever` after the closing `)` of `@@rename`"],
    );
    refused(
        &model_with("@@rename()"),
        &["`@@rename()` has an empty argument list"],
    );
}

/// `cratestack migrate` reads only `from = "<old>"`; before, every other
/// form checked OK and the migration dropped the table instead.
#[test]
fn a_rename_in_any_other_form_is_refused() {
    for arguments in [
        "from: \"old_docs\"",
        "\"old_docs\"",
        "old_docs",
        "from = old_docs",
        "from = 'old_docs'",
        "from = \"\"",
        "from = \"old\" \"docs\"",
        "from = \"old_docs\", to = \"docs\"",
        "to = \"old_docs\"",
    ] {
        refused(
            &model_with(&format!("@@rename({arguments})")),
            &[
                "`@@rename` takes exactly one argument, `@@rename(from = \"<old_name>\")`",
                "drop the old table",
            ],
        );
    }
    for accepted in [
        "from = \"old_docs\"",
        "from=\"old_docs\"",
        " from =  \"old_docs\" ",
    ] {
        parse_schema(&model_with(&format!("@@rename({accepted})")))
            .unwrap_or_else(|error| panic!("{accepted}: {error}"));
    }
}

#[test]
fn a_view_attribute_nothing_reads_is_refused() {
    for (line, name) in [
        ("@@paged", "@@paged"),
        ("@@audit", "@@audit"),
        ("@@emit(created)", "@@emit"),
        ("@@unique([id])", "@@unique"),
        ("@@map(\"v\")", "@@map"),
    ] {
        refused(
            &view_with(line),
            &[
                &format!("unsupported attribute `{name}` on a view"),
                "@@no_unique",
            ],
        );
    }
    refused(
        &view_with("@@materialised"),
        &["(did you mean `@@materialized`?)"],
    );
}

/// Maintainer decision 4: the view generator reads either quote
/// (`parse_rule_action`), and so does every other policy rule; the view
/// rule that limits `@@allow` to `read` refused the single-quoted form.
#[test]
fn a_view_allow_may_be_single_quoted() {
    let schema = parse_schema(&view_with("@@allow('read', auth() != null)"))
        .unwrap_or_else(|error| panic!("should parse: {error}"));
    assert_eq!(
        schema.views[0].attributes[1].raw,
        "@@allow('read', auth() != null)"
    );
    for action in ["'list'", "\"list\"", "'all'"] {
        refused(
            &view_with(&format!("@@allow({action}, true)")),
            &["`@@allow` only supports the `read` action"],
        );
    }
}
