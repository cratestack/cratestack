//! Where a field's `@rename` has an effect (GHSA-69g4-xvcm-vm2j;
//! `validate::rename_attributes`): `cratestack migrate` reads it only on a
//! stored column of a model, so on a field of a `view`, a `type` or the
//! `auth` block, or on a relation field, it renames nothing and is refused
//! in any form.

use crate::parse_schema;

const HEAD: &str = "datasource db {\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}\n\n\
                    model Doc {\n  id Int @id\n}\n\n";

const MARKERS: [&str; 2] = ["@rename(from = \"name\")", "@rename(from: \"name\")"];

/// The refusal, with the span checked to be `marker` itself.
#[track_caller]
fn refused_as_inert(source: &str, marker: &str, needles: &[&str]) {
    let error = parse_schema(source)
        .err()
        .unwrap_or_else(|| panic!("must be refused, but parsed:\n{source}"));
    let message = error.to_string();
    for needle in [
        "which has no effect here",
        "reads `@rename` only on a stored column of a model",
    ]
    .iter()
    .chain(needles)
    {
        assert!(message.contains(needle), "missing {needle:?}: {message}");
    }
    assert_eq!(&source[error.span()], marker, "{message}");
}

#[test]
fn a_rename_on_a_view_field_is_refused() {
    for marker in MARKERS {
        refused_as_inert(
            &format!(
                "{HEAD}view V from Doc {{\n  id Int @id\n  title String {marker}\n  \
                 @@server_sql(\"SELECT id, title FROM docs\")\n}}\n"
            ),
            marker,
            &[
                "field `title` on view `V`",
                "a view's columns come from its SQL",
            ],
        );
    }
}

#[test]
fn a_rename_on_a_type_or_auth_field_is_refused() {
    for marker in MARKERS {
        refused_as_inert(
            &format!("{HEAD}type Summary {{\n  title String {marker}\n}}\n"),
            marker,
            &[
                "field `title` on type `Summary`",
                "the block is not a table",
            ],
        );
        refused_as_inert(
            &format!("{HEAD}auth SessionUser {{\n  id Int\n  role String {marker}\n}}\n"),
            marker,
            &[
                "field `role` on auth block `SessionUser`",
                "the block is not a table",
            ],
        );
    }
}

#[test]
fn a_rename_on_a_relation_field_is_refused_in_a_model_and_a_mixin() {
    const RELATION: &str = "@relation(fields: [docId], references: [id])";
    for marker in MARKERS {
        refused_as_inert(
            &format!(
                "{HEAD}model Note {{\n  id Int @id\n  docId Int\n  doc Doc {RELATION} {marker}\n}}\n"
            ),
            marker,
            &[
                "field `doc` on model `Note`",
                "a relation field is not a column",
            ],
        );
        refused_as_inert(
            &format!(
                "{HEAD}mixin OfDoc {{\n  docId Int\n  doc Doc {RELATION} {marker}\n}}\n\n\
                 model Note {{\n  id Int @id\n  @use(OfDoc)\n}}\n"
            ),
            marker,
            &["field `doc` on ", "a relation field is not a column"],
        );
    }
}

/// A mixin's stored field becomes a model column once `@use`d, so its
/// marker is read and stays accepted, as on a model's own column.
#[test]
fn a_rename_on_a_stored_model_or_mixin_field_is_accepted() {
    for source in [
        format!(
            "{HEAD}model Note {{\n  id Int @id\n  title String {}\n}}\n",
            MARKERS[0]
        ),
        format!(
            "{HEAD}mixin Named {{\n  title String {}\n}}\n\n\
             model Note {{\n  id Int @id\n  @use(Named)\n}}\n",
            MARKERS[0]
        ),
    ] {
        parse_schema(&source).unwrap_or_else(|error| panic!("{source}: {error}"));
    }
}
