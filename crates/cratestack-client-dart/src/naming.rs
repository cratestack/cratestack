use std::collections::BTreeSet;

use cratestack_core::{EnumDecl, Field, Model, Procedure, Schema};

use crate::idents::to_pascal_case;

pub(crate) fn occupied_type_names(schema: &Schema) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for ty in &schema.types {
        names.insert(ty.name.clone());
    }
    for enum_decl in &schema.enums {
        names.insert(enum_decl.name.clone());
    }
    for model in &schema.models {
        names.insert(model.name.clone());
        names.insert(format!("Create{}Input", model.name));
        names.insert(format!("Update{}Input", model.name));
    }
    names
}

pub(crate) fn procedure_wrapper_name(
    procedure: &Procedure,
    occupied_type_names: &BTreeSet<String>,
) -> String {
    let base = format!("{}Args", to_pascal_case(&procedure.name));
    if !occupied_type_names.contains(&base) {
        return base;
    }

    let procedure_name = format!("{}ProcedureArgs", to_pascal_case(&procedure.name));
    if !occupied_type_names.contains(&procedure_name) {
        return procedure_name;
    }

    format!("{}ProcedureRequest", to_pascal_case(&procedure.name))
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

pub(crate) fn is_relation_field(model_names: &BTreeSet<&str>, field: &Field) -> bool {
    model_names.contains(field.ty.name.as_str())
}

/// Field carries `@computed`/`@computed(params: <Type>?)`
/// (`docs/design/computed-fields.md`) — resolved at response time, never
/// stored. Unlike a relation field, a computed field IS part of the
/// default projection (`scalar_model_fields` deliberately does NOT
/// exclude it — a computed field is exactly as "scalar" as any other
/// leaf field from the wire's point of view), so call sites that need to
/// exclude it (create/update inputs, `Where`/`SortField` builders) check
/// this explicitly rather than routing it through `scalar_model_fields`.
pub(crate) fn is_computed_field(field: &Field) -> bool {
    cratestack_core::is_computed_field(field)
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
