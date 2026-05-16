use cratestack_core::{Field, TypeArity, TypeRef};

use crate::views::DataClassKind;

pub(crate) fn dart_field_type(field: &Field, kind: DataClassKind) -> String {
    let is_nullable = matches!(kind, DataClassKind::Patch | DataClassKind::ProjectionModel)
        || field.ty.arity == TypeArity::Optional;
    dart_type(&field.ty, is_nullable)
}

pub(crate) fn dart_type(type_ref: &TypeRef, force_nullable: bool) -> String {
    if type_ref.is_page() {
        let item = type_ref
            .page_item()
            .expect("validated Page<T> should include an item type");
        let base = format!("Page<{}>", dart_type(item, false));
        return if force_nullable {
            format!("{base}?")
        } else {
            base
        };
    }

    let base = match type_ref.name.as_str() {
        "String" | "Cuid" | "Uuid" => "String".to_owned(),
        "Int" => "int".to_owned(),
        "Float" => "double".to_owned(),
        "Boolean" => "bool".to_owned(),
        "DateTime" => "DateTime".to_owned(),
        "Json" => "Object?".to_owned(),
        "Bytes" => "Uint8List".to_owned(),
        other => other.to_owned(),
    };

    match type_ref.arity {
        TypeArity::List => format!("List<{base}>{}", if force_nullable { "?" } else { "" }),
        TypeArity::Required => {
            if force_nullable && base != "Object?" {
                format!("{base}?")
            } else {
                base
            }
        }
        TypeArity::Optional => {
            if base.ends_with('?') {
                base
            } else {
                format!("{base}?")
            }
        }
    }
}
