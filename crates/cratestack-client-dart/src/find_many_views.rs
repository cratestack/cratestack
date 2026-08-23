//! Per-model `<Model>Where` / `<Model>SortField` / `<Model>OrderByClause`
//! / `<Model>FindMany` view builders — the Dart counterpart to
//! `cratestack-macros`'s `model/find_many_{where,order_by,input}.rs` and
//! `cratestack-client-typescript`'s `find_many_views.rs`. The shared
//! filter classes (`StringFilter`/`NumberFilter`/etc.) these reference
//! are hand-written directly in `models.dart.j2`/
//! `riverpod/shared_types.dart.j2`, mirroring `Page`/`PageInfo`/
//! `PageInput` — only the per-model shapes below vary by schema, so only
//! they need real codegen. Split out per the repo's 200-LoC file
//! convention.

use std::collections::BTreeSet;

use cratestack_core::Field;
use cratestack_core::Model;

use crate::idents::dart_identifier;
use crate::naming::{is_computed_field, scalar_model_fields};
use crate::views::{DataClassView, EnumVariantView, EnumView, FieldView};

/// Same 8 types `cratestack-macros`'s `find_many_where.rs` (and its
/// TypeScript counterpart) filter generated code down to — `Json`/
/// `Bytes`/enum/custom-`type` fields are excluded, matching the untyped
/// REST `?where=` route's own (`query_scalar_parser_tokens`-proven)
/// coverage.
fn is_filterable_scalar(field: &Field) -> bool {
    matches!(
        field.ty.name.as_str(),
        "String" | "Cuid" | "Int" | "Float" | "Boolean" | "Uuid" | "DateTime" | "Decimal"
    )
}

/// The shared filter class (hardcoded once in `models.dart.j2`/
/// `riverpod/shared_types.dart.j2`) this field's operators live on.
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
pub(crate) fn build_where_data_class(
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> Option<DataClassView> {
    let fields = scalar_model_fields(model, model_names)
        .into_iter()
        // `@computed` fields are never filterable — resolved at response
        // time, they never live in a column the server's `?where=` route
        // can query (`docs/design/computed-fields.md`).
        .filter(|field| !is_computed_field(field))
        .filter(|field| is_filterable_scalar(field))
        .collect::<Vec<_>>();
    if fields.is_empty() {
        return None;
    }
    let where_name = format!("{}Where", model.name);
    let fields: Vec<FieldView> = fields
        .iter()
        .map(|field| {
            let identifier = dart_identifier(&field.name);
            let filter_type = filter_type_name(field);
            FieldView::new(
                identifier.clone(),
                field.name.clone(),
                format!("{filter_type}?"),
                false,
                false,
                false,
                format!(
                    "value['{wire}'] == null ? null : {filter_type}.fromWire(cratestackAsValueMap(value['{wire}']))",
                    wire = field.name
                ),
                format!("{identifier}?.toWire()"),
            )
        })
        .collect();
    Some(DataClassView {
        name: where_name,
        has_fields: true,
        // Never `Patch`-kind, no touch flags, no relation-valued list
        // fields — see `DataClassView::builder_args`'s doc.
        builder_args: String::new(),
        emit_builder: true,
        fields,
    })
}

/// A `PostSortField { id, title, ... }` enum — reuses `EnumView`'s
/// existing template rendering rather than a new template block. Every
/// scalar field is sortable (unlike filtering, ordering has no type
/// restriction — see `cratestack-macros`'s `find_many_order_by.rs` doc).
pub(crate) fn build_sort_field_enum(model: &Model, model_names: &BTreeSet<&str>) -> EnumView {
    EnumView {
        name: format!("{}SortField", model.name),
        variants: scalar_model_fields(model, model_names)
            .into_iter()
            // `@computed` fields are never sortable, same reasoning as
            // `build_where_data_class` above.
            .filter(|field| !is_computed_field(field))
            .map(|field| EnumVariantView {
                identifier: dart_identifier(&field.name),
                wire_name: field.name.clone(),
            })
            .collect(),
    }
}

/// `{ field: PostSortField; direction: SortDirection; }` — a `List` of
/// these on the `FindMany` input preserves multi-key sort order (unlike
/// a field-keyed map, whose iteration order Dart doesn't guarantee to
/// match JSON key insertion order either).
pub(crate) fn build_order_by_clause_data_class(model: &Model) -> DataClassView {
    let order_by_name = format!("{}OrderByClause", model.name);
    let sort_field_name = format!("{}SortField", model.name);
    let fields = vec![
        FieldView::new(
            "field".to_owned(),
            "field".to_owned(),
            sort_field_name.clone(),
            true,
            false,
            false,
            format!(
                "{sort_field_name}.fromWire(cratestackRequireWireValue('{order_by_name}', 'field', value['field']))"
            ),
            "field.toWire()".to_owned(),
        ),
        FieldView::new(
            "direction".to_owned(),
            "direction".to_owned(),
            "SortDirection".to_owned(),
            true,
            false,
            false,
            format!(
                "SortDirection.fromWire(cratestackRequireWireValue('{order_by_name}', 'direction', value['direction']))"
            ),
            "direction.toWire()".to_owned(),
        ),
    ];
    DataClassView {
        name: order_by_name.clone(),
        has_fields: true,
        // Never `Patch`-kind, no touch flags, no relation-valued list
        // fields — see `DataClassView::builder_args`'s doc.
        builder_args: String::new(),
        emit_builder: true,
        fields,
    }
}

/// What a `FindMany<Model>` procedure argument resolves to. `where` is
/// omitted from the class entirely (not just left unset) when the model
/// has no filterable field, matching `has_where`'s caller.
pub(crate) fn build_find_many_data_class(model: &Model, has_where: bool) -> DataClassView {
    let find_many_name = format!("{}FindMany", model.name);
    let where_name = format!("{}Where", model.name);
    let order_by_name = format!("{}OrderByClause", model.name);

    let mut fields = Vec::new();
    if has_where {
        fields.push(FieldView::new(
            "where".to_owned(),
            "where".to_owned(),
            format!("{where_name}?"),
            false,
            false,
            false,
            format!(
                "value['where'] == null ? null : {where_name}.fromWire(cratestackAsValueMap(value['where']))"
            ),
            "where?.toWire()".to_owned(),
        ));
    }
    // issue #668 phase 2: `orderBy`'s Dart type is `List<{order_by_name}>?`,
    // same as any genuine schema list field — the old Rust-driven inline
    // builder template used a separate `is_list: false` flag here to
    // exclude this framework-synthesized `FindMany` field from issue
    // #661's default-empty-list/`add{Field}` builder treatment (it has no
    // Rust-side counterpart to keep parity with). `package:
    // cratestack_builder` derives list-ness purely from the emitted Dart
    // source (`DartType.isDartCoreList`) — it cannot see, and was never
    // asked to preserve, that distinction — so `<Model>FindMany.orderBy`
    // now gets an `addOrderBy` append setter and defaults to `[]` rather
    // than `null` when unset, like every other list field on a
    // non-`Patch` class — an accepted consequence of moving builder
    // generation out of this crate (issue #668 phase 2). This file keeps
    // asserting the field's own `dart_type`/wire codec (`tests/
    // generator.rs`), not its builder behavior, which is no longer this
    // crate's concern.
    fields.push(FieldView::new(
        "orderBy".to_owned(),
        "orderBy".to_owned(),
        format!("List<{order_by_name}>?"),
        false,
        false,
        false,
        format!(
            "value['orderBy'] == null ? null : cratestackAsValueList(value['orderBy']).map((item) => {order_by_name}.fromWire(cratestackAsValueMap(item))).toList(growable: false)"
        ),
        "orderBy?.map((item) => item.toWire()).toList(growable: false)".to_owned(),
    ));

    DataClassView {
        name: find_many_name,
        has_fields: true,
        // Never `Patch`-kind, no touch flags, no relation-valued list
        // fields — see `DataClassView::builder_args`'s doc.
        builder_args: String::new(),
        emit_builder: true,
        fields,
    }
}
