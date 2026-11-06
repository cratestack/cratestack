//! The columns under a `@unique` or `@@unique` constraint, for
//! `ModelDescriptor::with_unique_columns`.

use cratestack_core::Model;

use crate::shared::to_snake_case;

/// SQL names of the columns under `@unique` or any `@@unique([...])`.
pub(super) fn unique_column_names(model: &Model) -> Vec<String> {
    let field_level = model.fields.iter().filter(|field| {
        field
            .attributes
            .iter()
            .any(|attribute| attribute.raw == "@unique" || attribute.raw.starts_with("@unique("))
    });
    let composite = model.attributes.iter().filter_map(|attribute| {
        cratestack_core::parse_composite_unique_attribute(&attribute.raw).ok()
    });
    let mut names: Vec<String> = field_level
        .map(|field| to_snake_case(&field.name))
        .chain(
            composite
                .flat_map(|parsed| parsed.fields)
                .map(|field| to_snake_case(&field)),
        )
        .collect();
    names.sort();
    names.dedup();
    names
}
