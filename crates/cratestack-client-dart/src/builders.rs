use std::collections::BTreeSet;

use cratestack_core::{EnumDecl, Field, TypeArity};

use crate::dart_types::dart_field_type;
use crate::idents::{dart_identifier, to_camel_case};
use crate::naming::is_relation_field;
use crate::views::{DataClassKind, DataClassView, EnumVariantView, EnumView, FieldView};
use crate::wire_decode::decode_value_expr;
use crate::wire_encode::encode_value_expr;

pub(crate) fn build_enum_view(enum_decl: &EnumDecl) -> EnumView {
    EnumView {
        name: enum_decl.name.clone(),
        variants: enum_decl
            .variants
            .iter()
            .map(|variant| EnumVariantView {
                identifier: dart_identifier(&to_camel_case(&variant.name)),
                wire_name: variant.name.clone(),
            })
            .collect(),
    }
}

/// Whether this field gets an `add{Field}` append setter (issue #661).
///
/// List arity, minus one exclusion: a *relation*-valued list on the model
/// class. Rust builds that class from `scalar_model_fields`, which drops
/// relation fields entirely, so a Dart `addPosts` there would have no Rust
/// counterpart — reintroducing exactly the cross-language divergence #661
/// exists to remove.
///
/// Deliberately scoped to `ProjectionModel` rather than applied wherever a
/// field happens to name a model: a `type` block's fields go through Rust's
/// `scoped_builder_fields`, which does *not* filter relations, so a
/// model-typed list inside a `type` does get `add_` on both sides and must
/// keep it here.
fn emits_append_setter(field: &Field, kind: DataClassKind, model_names: &BTreeSet<&str>) -> bool {
    if field.ty.arity != TypeArity::List {
        return false;
    }
    !(matches!(kind, DataClassKind::ProjectionModel) && is_relation_field(model_names, field))
}

pub(crate) fn build_data_class(
    name: &str,
    fields: &[&Field],
    kind: DataClassKind,
    enum_names: &BTreeSet<&str>,
    model_names: &BTreeSet<&str>,
) -> DataClassView {
    let fields: Vec<FieldView> = fields
        .iter()
        .map(|field| {
            FieldView::new(
                dart_identifier(&field.name),
                field.name.clone(),
                dart_field_type(field, kind),
                matches!(kind, DataClassKind::Plain)
                    && matches!(field.ty.arity, TypeArity::Required | TypeArity::List),
                emits_append_setter(field, kind, model_names),
                matches!(kind, DataClassKind::Patch),
                decode_value_expr(
                    &format!("value['{}']", field.name),
                    &field.ty,
                    enum_names,
                    matches!(kind, DataClassKind::Patch | DataClassKind::ProjectionModel),
                    name,
                    &field.name,
                ),
                encode_value_expr(
                    &dart_identifier(&field.name),
                    &field.ty,
                    enum_names,
                    matches!(kind, DataClassKind::Patch | DataClassKind::ProjectionModel),
                ),
            )
        })
        .collect();
    DataClassView {
        name: name.to_owned(),
        has_fields: !fields.is_empty(),
        fields,
    }
}
