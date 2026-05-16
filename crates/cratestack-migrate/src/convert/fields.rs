//! Per-field projection: scalar/enum/user-defined type detection,
//! arity mapping, default-value parsing.

use std::collections::HashSet;

use cratestack_core::{Field, TypeArity};

use crate::ir::{Column, ColumnArity, ColumnDefault, ColumnType};
use crate::naming::column_name;

pub(super) fn field_to_column(
    field: &Field,
    known_enums: &HashSet<&str>,
    known_types: &HashSet<&str>,
) -> Column {
    let primary_key = field_has_id(field);
    let arity = match field.ty.arity {
        TypeArity::Required => ColumnArity::Required,
        TypeArity::Optional => ColumnArity::Optional,
        TypeArity::List => ColumnArity::List,
    };

    let ty_name = field.ty.name.as_str();
    let ty = if known_enums.contains(ty_name) {
        ColumnType::Enum(ty_name.to_owned())
    } else if known_types.contains(ty_name) {
        ColumnType::UserDefined(ty_name.to_owned())
    } else {
        ColumnType::Scalar(ty_name.to_owned())
    };

    Column {
        name: column_name(&field.name),
        ty,
        arity,
        default: field_default(field),
        primary_key,
    }
}

fn field_has_id(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@id" || attribute.raw.starts_with("@id("))
}

pub(super) fn field_has_unique(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@unique" || attribute.raw.starts_with("@unique("))
}

pub(super) fn is_relation_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@relation("))
}

fn field_default(field: &Field) -> Option<ColumnDefault> {
    let raw = field
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@default("))?
        .raw
        .as_str();
    // `@default(<inner>)` — strip prefix/suffix, trim. We classify
    // function calls (suffix `()`) vs literals; everything else is
    // passed to the emitter as a literal and quoted per dialect.
    let inner = raw
        .strip_prefix("@default(")?
        .strip_suffix(')')?
        .trim()
        .to_owned();
    if inner.is_empty() {
        return None;
    }
    if inner.ends_with(')') && !inner.starts_with('\'') {
        Some(ColumnDefault::Function(inner))
    } else {
        Some(ColumnDefault::Literal(inner))
    }
}
