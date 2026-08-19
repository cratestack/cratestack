use std::collections::BTreeSet;

use cratestack_core::{EnumDecl, Field, TypeArity};

use crate::dart_types::dart_field_type;
use crate::idents::{dart_identifier, to_camel_case};
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

pub(crate) fn build_data_class(
    name: &str,
    fields: &[&Field],
    kind: DataClassKind,
    enum_names: &BTreeSet<&str>,
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
