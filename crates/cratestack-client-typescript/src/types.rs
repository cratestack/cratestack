use std::collections::BTreeSet;

use cratestack_core::{EnumDecl, Field, Model, TypeArity, TypeRef};

pub(crate) fn ts_type(type_ref: &TypeRef, enum_names: &BTreeSet<&str>) -> String {
    if type_ref.is_page() {
        let item = type_ref
            .page_item()
            .expect("validated Page<T> should include an item type");
        return format!("Page<{}>", ts_type(item, enum_names));
    }

    let base = match type_ref.name.as_str() {
        "String" | "Cuid" | "Uuid" | "DateTime" => "string".to_owned(),
        "Int" | "Float" => "number".to_owned(),
        "Boolean" => "boolean".to_owned(),
        "Json" => "JsonValue".to_owned(),
        "Bytes" => "number[]".to_owned(),
        other if enum_names.contains(other) => other.to_owned(),
        other => other.to_owned(),
    };

    match type_ref.arity {
        TypeArity::Required => base,
        TypeArity::Optional => format!("{base} | null"),
        TypeArity::List => format!("{base}[]"),
    }
}

pub(crate) fn model_name_set(models: &[Model]) -> BTreeSet<&str> {
    models.iter().map(|model| model.name.as_str()).collect()
}

pub(crate) fn enum_name_set(enums: &[EnumDecl]) -> BTreeSet<&str> {
    enums
        .iter()
        .map(|enum_decl| enum_decl.name.as_str())
        .collect()
}

pub(crate) fn scalar_model_fields<'a>(
    model: &'a Model,
    model_names: &BTreeSet<&str>,
) -> Vec<&'a Field> {
    model
        .fields
        .iter()
        .filter(|field| !is_relation_field(model_names, field))
        .collect()
}

fn is_relation_field(model_names: &BTreeSet<&str>, field: &Field) -> bool {
    model_names.contains(field.ty.name.as_str())
}

pub(crate) fn primary_key_field(model: &Model) -> Option<&Field> {
    model.fields.iter().find(|field| is_primary_key(field))
}

pub(crate) fn is_primary_key(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@id"))
}

fn has_default(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@default"))
}

pub(crate) fn is_generated_on_create(field: &Field) -> bool {
    has_default(field)
}

pub(crate) fn is_paged_model(model: &Model) -> bool {
    model
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@@paged")
}
