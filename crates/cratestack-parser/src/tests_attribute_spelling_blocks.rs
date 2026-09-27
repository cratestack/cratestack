//! Block-level `@@` attributes written in a form no generator reads
//! (`validate::attribute_spelling`): two attributes on one line, and an
//! argument list or stray punctuation on one that takes no arguments.

use super::parse_schema;

#[track_caller]
fn refused(source: &str, needle: &str) {
    let error = parse_schema(source).expect_err("schema should be refused");
    assert!(error.to_string().contains(needle), "error: {error}");
}

fn model_with(line: &str) -> String {
    format!("model Doc {{\n  id Int @id\n  deletedAt DateTime?\n  {line}\n}}\n")
}

fn view_with(line: &str) -> String {
    format!(
        "model A {{\n  id Int @id\n}}\n\
         view V from A {{\n  id Int @id @from(A.id)\n  @@sql(\"SELECT id FROM a\")\n  {line}\n}}\n"
    )
}

#[test]
fn refuses_two_model_attributes_on_one_line() {
    for (line, fix) in [
        ("@@audit@@soft_delete", "`@@audit` / `@@soft_delete`"),
        ("@@audit @@soft_delete", "`@@audit` / `@@soft_delete`"),
        (
            "@@allow(\"read\", true) @@deny(\"read\", false)",
            "`@@allow(\"read\", true)` / `@@deny(\"read\", false)`",
        ),
    ] {
        refused(
            &model_with(line),
            &format!(
                "model `Doc` writes `{line}` as one block attribute: a block-level attribute \
                 is its whole line, so what follows the first attribute is not recognised, and \
                 this is refused. Put each on a line of its own: {fix}"
            ),
        );
    }
}

#[test]
fn refuses_two_view_attributes_on_one_line() {
    refused(
        &view_with("@@no_unique@@allow(\"read\", true)"),
        "view `V` writes `@@no_unique@@allow(\"read\", true)` as one block attribute",
    );
    refused(
        "model A {\n  id Int @id\n}\n\
         view V from A {\n  id Int @id @from(A.id)\n  @@sql(\"SELECT id FROM a\") @@materialized\n}\n",
        "Put each on a line of its own: `@@sql(\"SELECT id FROM a\")` / `@@materialized`",
    );
}

#[test]
fn refuses_an_argument_list_on_a_no_argument_view_attribute() {
    for line in ["@@materialized()", "@@no_unique(true)"] {
        let name = line.split('(').next().unwrap_or_default();
        refused(
            &view_with(line),
            &format!("view `V` writes `{line}`; `{name}` does not take arguments"),
        );
    }
}

#[test]
fn refuses_stray_punctuation_on_a_no_argument_block_attribute() {
    refused(
        &view_with("@@no_unique,"),
        "`@@no_unique` is recognised only when written exactly `@@no_unique`",
    );
    // `@@audit // keep for a year` is no longer here: a trailing comment is
    // stripped before validation (GHSA-69g4-xvcm-vm2j), see
    // `tests_attribute_spelling_comments`.
    for line in ["@@audit,", "@@soft_delete;"] {
        refused(&model_with(line), &format!("model `Doc` writes `{line}`"));
    }
}

// An `@` inside an argument group is not a second attribute: an unquoted
// SQL body keeps the view validator's own, more precise diagnostic.
#[test]
fn an_at_sign_inside_an_argument_group_is_left_to_its_validator() {
    refused(
        "model A {\n  id Int @id\n}\n\
         view V from A {\n  id Int @id @from(A.id)\n  @@sql(SELECT id FROM a WHERE t @> x)\n}\n",
        "has a SQL attribute whose argument is not a quoted string",
    );
}

#[test]
fn block_diagnostics_point_at_the_attribute_line() {
    let source = model_with("@@audit @@soft_delete");
    let error = parse_schema(&source).expect_err("schema should be refused");
    assert_eq!(&source[error.span()], "@@audit @@soft_delete", "{error}");
}

// Positive controls: `@` inside policy and SQL string literals, and the
// no-argument attributes written exactly.
#[test]
fn accepts_at_signs_in_strings_and_one_attribute_per_line() {
    parse_schema(&model_with(
        "@@audit\n  @@soft_delete\n  @@allow(\"read\", auth().email == \"a@b\")\n  \
         @@allow('update', auth().email == 'ops@b.io')\n  @@index([deletedAt])",
    ))
    .expect("model attributes one per line stay accepted");
    parse_schema(
        "model A {\n  id Int @id\n  tags String\n}\n\
         view V from A {\n  id Int @id @from(A.id)\n  @@no_unique\n\
         @@allow(\"read\", auth().email == \"a@b\")\n\
         @@sql(\"\"\"\n    SELECT id FROM a WHERE tags @> '{x}' AND tags <> \"q@r\"\n  \"\"\")\n}\n\
         auth Ctx {\n  id Int\n  email String\n}\n",
    )
    .expect("an `@` inside a SQL body or a policy literal is not an attribute");
}
