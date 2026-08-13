//! Per-model `<Model>Where` / `<Model>SortField` / `<Model>OrderByClause`
//! / `<Model>FindMany` view builders — the TypeScript counterpart to
//! `cratestack-macros`'s `model/find_many_{where,order_by,input}.rs`.
//! Split out from `views.rs` per the repo's 200-LoC file convention.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model};

use crate::naming::{escape_ts_string, ts_identifier};
use crate::types::scalar_model_fields;
use crate::views::{EnumView, FieldView, InterfaceView};

/// Same 8 types `cratestack-macros`'s `find_many_where.rs` filters
/// generated code down to — `Json`/`Bytes`/enum/custom-`type` fields are
/// excluded, matching the untyped REST `?where=` route's own
/// (`query_scalar_parser_tokens`-proven) coverage.
fn is_filterable_scalar(field: &Field) -> bool {
    matches!(
        field.ty.name.as_str(),
        "String" | "Cuid" | "Int" | "Float" | "Boolean" | "Uuid" | "DateTime" | "Decimal"
    )
}

/// The shared filter interface (hardcoded once in `models.ts.j2`/
/// `swr/models-shared.ts.j2`, mirroring `Page`/`PageInfo`/`PageInput`)
/// this field's operators live on.
fn filter_type_name(field: &Field) -> &'static str {
    match field.ty.name.as_str() {
        "String" | "Cuid" => "StringFilter",
        "Int" | "Float" => "NumberFilter",
        "Boolean" => "BooleanFilter",
        "Uuid" => "UuidFilter",
        "DateTime" => "DateTimeFilter",
        "Decimal" => "DecimalFilter",
        other => unreachable!("{other} is not a filterable scalar — call site must gate first"),
    }
}

/// `None` when the model has no filterable field at all — same
/// omit-rather-than-emit-empty convention `Create<Model>Input` follows
/// when a model disallows create.
pub(crate) fn build_where_interface(
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> Option<InterfaceView> {
    let fields = scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| is_filterable_scalar(field))
        .collect::<Vec<_>>();
    if fields.is_empty() {
        return None;
    }
    Some(InterfaceView {
        name: format!("{}Where", model.name),
        has_fields: true,
        fields: fields
            .iter()
            .map(|field| FieldView {
                property: ts_identifier(&field.name),
                wire_name: field.name.clone(),
                type_name: filter_type_name(field).to_owned(),
                optional: true,
            })
            .collect(),
    })
}

/// A `type PostSortField = 'id' | 'title' | ...;` union — reuses
/// `EnumView`'s existing template rendering (`export type X = union;
/// export const XValues = [...] as const satisfies readonly X[];`)
/// rather than a new template block. Every scalar field is sortable
/// (unlike filtering, ordering has no type restriction — see
/// `find_many_order_by.rs`'s own doc for why).
pub(crate) fn build_sort_field_view(model: &Model, model_names: &BTreeSet<&str>) -> EnumView {
    let values = scalar_model_fields(model, model_names)
        .into_iter()
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    let union = values
        .iter()
        .map(|value| format!("'{}'", escape_ts_string(value)))
        .collect::<Vec<_>>()
        .join(" | ");
    EnumView {
        name: format!("{}SortField", model.name),
        union,
        values,
    }
}

/// `{ field: PostSortField; direction: SortDirection; }` — a `Vec`/array
/// of these on the `FindMany` input preserves multi-key sort order
/// (unlike a field-keyed object, which — depending on the runtime —
/// isn't guaranteed to round-trip key insertion order through JSON).
pub(crate) fn build_order_by_clause_interface(model: &Model) -> InterfaceView {
    InterfaceView {
        name: format!("{}OrderByClause", model.name),
        has_fields: true,
        fields: vec![
            FieldView {
                property: "field".to_owned(),
                wire_name: "field".to_owned(),
                type_name: format!("{}SortField", model.name),
                optional: false,
            },
            FieldView {
                property: "direction".to_owned(),
                wire_name: "direction".to_owned(),
                type_name: "SortDirection".to_owned(),
                optional: false,
            },
        ],
    }
}

/// What a `FindMany<Model>` procedure argument resolves to. `where` is
/// omitted from the interface entirely (not just left unset) when the
/// model has no filterable field, matching `has_where`'s caller.
pub(crate) fn build_find_many_interface(model: &Model, has_where: bool) -> InterfaceView {
    let mut fields = Vec::new();
    if has_where {
        fields.push(FieldView {
            property: "where".to_owned(),
            wire_name: "where".to_owned(),
            type_name: format!("{}Where", model.name),
            optional: true,
        });
    }
    fields.push(FieldView {
        property: "orderBy".to_owned(),
        wire_name: "orderBy".to_owned(),
        type_name: format!("{}OrderByClause[]", model.name),
        optional: true,
    });
    InterfaceView {
        name: format!("{}FindMany", model.name),
        has_fields: true,
        fields,
    }
}
