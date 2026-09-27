//! Parse `@rename(from = "...")` / `@@rename(from = "...")` markers.
//!
//! Both are read with [`rename_marker_from`], the reader
//! `cratestack-parser` validates them against (GHSA-69g4-xvcm-vm2j), so a
//! marker that parsed is one this reads, and a schema carries at most one
//! of each per model or field — the parser refuses a second, since only
//! the first is read here.

use cratestack_core::schema::rename_marker_from;
use cratestack_core::{Field, Model};

pub(super) fn model_rename_from(model: &Model) -> Option<String> {
    let raw = model
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@@rename("))?
        .raw
        .as_str();
    rename_marker_from(raw, "@@rename").map(str::to_owned)
}

/// `None` for malformed input — the diff engine treats a malformed rename
/// as if the attribute were absent, falling back to drop+add; the parser
/// refuses that input first.
pub(super) fn field_rename_from(field: &Field) -> Option<String> {
    let raw = field
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@rename("))?
        .raw
        .as_str();
    rename_marker_from(raw, "@rename").map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use cratestack_parser::parse_schema;

    use super::{field_rename_from, model_rename_from};

    /// The form the parser accepts is the form this reads.
    #[test]
    fn an_accepted_model_marker_is_read() {
        let schema =
            parse_schema("model Doc {\n  id Int @id\n  @@rename(from = \"old_docs\")\n}\n")
                .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            model_rename_from(&schema.models[0]).as_deref(),
            Some("old_docs")
        );
    }

    #[test]
    fn an_accepted_field_marker_is_read() {
        let schema = parse_schema(
            "model Doc {\n  id Int @id\n  title String @rename(from = \"name\") @unique\n}\n",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            field_rename_from(&schema.models[0].fields[1]).as_deref(),
            Some("name")
        );
    }
}
