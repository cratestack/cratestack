//! Field-attribute predicates: `@id`, `@readonly`, `@server_only`,
//! `@pii`, `@sensitive`, `@version`, `@default(...)`, plus the
//! comparison-support check used by query-filter generation.

use cratestack_core::{Field, Model, TypeArity};

pub(crate) fn supports_comparison(field: &Field) -> bool {
    field.ty.arity == TypeArity::Required
        && matches!(
            field.ty.name.as_str(),
            "String" | "Cuid" | "Int" | "Float" | "DateTime" | "Decimal" | "Uuid"
        )
}

pub(crate) fn is_custom_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@custom")
}

pub(crate) fn is_primary_key(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@id"))
}

pub(crate) fn is_paged_model(model: &Model) -> bool {
    model
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@@paged")
}

/// Field carries `@readonly`.
pub(crate) fn is_readonly_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@readonly")
}

/// Field carries `@server_only`.
pub(crate) fn is_server_only_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@server_only")
}

/// Field carries `@pii` — redacted in audit log.
pub(crate) fn is_pii_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@pii")
}

/// Field carries `@sensitive` — redacted in audit log.
pub(crate) fn is_sensitive_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@sensitive")
}

fn has_default(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@default"))
}

pub(crate) fn auth_default_field(field: &Field) -> Option<&str> {
    field.attributes.iter().find_map(|attribute| {
        let inner = attribute
            .raw
            .trim()
            .strip_prefix("@default(")?
            .strip_suffix(')')?
            .trim();
        inner.strip_prefix("auth().").map(str::trim)
    })
}

pub(crate) fn is_generated_on_create(field: &Field) -> bool {
    has_default(field)
}

/// Field carries `@version` — the optimistic-lock column.
pub(crate) fn is_version_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@version")
}
