//! Rename markers (GHSA-69g4-xvcm-vm2j; `validate::rename_attributes`): a
//! field's `@rename` in exactly the form `cratestack migrate` reads, and
//! at most one marker per field or model.

use crate::parse_schema;

const HEAD: &str =
    "datasource db {\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}\n\n";

fn field_with(attributes: &str) -> String {
    format!("{HEAD}model Doc {{\n  id Int @id\n  title String {attributes}\n}}\n")
}

/// The refusal, with the span checked to be `marker` itself.
#[track_caller]
fn refused_at(source: &str, marker: &str, needles: &[&str]) {
    let error = parse_schema(source)
        .err()
        .unwrap_or_else(|| panic!("must be refused, but parsed:\n{source}"));
    let message = error.to_string();
    for needle in needles {
        assert!(message.contains(needle), "missing {needle:?}: {message}");
    }
    assert_eq!(&source[error.span()], marker, "{message}");
}

#[test]
fn a_field_rename_in_the_form_the_migrator_reads_is_accepted() {
    for marker in [
        "@rename(from = \"name\")",
        "@rename(from=\"name\")",
        "@rename(  from  =  \"name\"  )",
        "@rename(from = \"name\") @unique",
    ] {
        parse_schema(&field_with(marker)).unwrap_or_else(|error| panic!("{marker}: {error}"));
    }
}

/// `title String @rename(from: "name")` checked OK, and the next migration
/// dropped `name` and added `title`, the column's data gone.
#[test]
fn a_field_rename_in_any_other_form_is_refused() {
    for marker in [
        "@rename(from: \"name\")",
        "@rename(\"name\")",
        "@rename(name)",
        "@rename(from = name)",
        "@rename(from = 'name')",
        "@rename(from = \"\")",
        "@rename(from = \"na\\\"me\")",
        "@rename(to = \"name\")",
        "@rename(from = \"name\", to = \"title\")",
        "@rename()",
        "@rename",
        "@rename(from = \"name\"),",
        "@rename:name",
    ] {
        refused_at(
            &field_with(marker),
            marker,
            &[
                "field `title` on model `Doc` writes",
                "`@rename` takes exactly one argument, `@rename(from = \"<old_name>\")`",
                "drop the old column",
            ],
        );
    }
}

/// A misspelled name is a near-miss of `@rename`; one written apart from
/// its `(` is refused by the attribute splitter.
#[test]
fn a_misspelled_or_spaced_field_rename_is_refused() {
    for marker in [
        "@renam(from = \"name\")",
        "@Rename(from = \"name\")",
        "@rnaeme(from = \"name\")",
    ] {
        let message = parse_schema(&field_with(marker))
            .expect_err(marker)
            .to_string();
        assert!(message.contains("did you mean `@rename`?"), "{message}");
    }
    let message = parse_schema(&field_with("@rename (from = \"name\")"))
        .expect_err("spaced")
        .to_string();
    assert!(message.contains("must not be separated"), "{message}");
}

/// The form is checked where the marker is written, a mixin's included.
/// On a view, a type, the auth block or a relation field it is refused in
/// any form (`tests_rename_placement`).
#[test]
fn a_malformed_field_rename_is_refused_in_a_mixin() {
    let mixin = format!(
        "{HEAD}mixin Named {{\n  title String @rename(from: \"name\")\n}}\n\n\
         model Doc {{\n  id Int @id\n  @use(Named)\n}}\n"
    );
    refused_at(
        &mixin,
        "@rename(from: \"name\")",
        &["`@rename` takes exactly one argument"],
    );
}

/// The migrator reads only the first marker, so a second was silently
/// ignored — the same name twice included.
#[test]
fn a_second_field_rename_is_refused_at_the_second() {
    for second in ["@rename(from = \"label\")", "@rename(from = \"name\")"] {
        refused_at(
            &field_with(&format!("@rename(from = \"name\") {second}")),
            second,
            &["declares a second `@rename`", "reads only the first"],
        );
    }
}

#[test]
fn a_second_model_rename_is_refused_at_the_second() {
    for second in [
        "@@rename(from = \"papers\")",
        "@@rename(from = \"documents\")",
    ] {
        let source = format!(
            "{HEAD}model Doc {{\n  id Int @id\n  @@rename(from = \"documents\")\n  {second}\n}}\n"
        );
        refused_at(
            &source,
            second,
            &[
                "model `Doc` declares a second `@@rename`",
                "reads only the first",
            ],
        );
    }
    parse_schema(&format!(
        "{HEAD}model Doc {{\n  id Int @id\n  @@rename(from = \"documents\")\n}}\n"
    ))
    .unwrap_or_else(|error| panic!("{error}"));
}
