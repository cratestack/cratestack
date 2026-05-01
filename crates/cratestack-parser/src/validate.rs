use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{Field, Schema, SourceSpan, TypeRef, parse_emit_attribute};

use crate::diagnostics::{SchemaError, span_error};
use crate::relation_helpers::{parse_relation_attribute, validate_relation_scalar_compatibility};

const BUILTIN_TYPES: &[&str] = &[
    "String", "Cuid", "Int", "Float", "Boolean", "DateTime", "Json", "Bytes", "Uuid", "Page",
];

pub(crate) fn validate_schema(
    path: &str,
    source: &str,
    schema: &Schema,
) -> Result<(), SchemaError> {
    let mut type_names = BTreeSet::new();
    for builtin in BUILTIN_TYPES {
        type_names.insert((*builtin).to_owned());
    }
    for ty in &schema.types {
        ensure_unique(
            &mut type_names,
            &ty.name,
            ty.span,
            "duplicate type name",
            source,
            path,
        )?;
    }
    for enum_decl in &schema.enums {
        ensure_unique(
            &mut type_names,
            &enum_decl.name,
            enum_decl.span,
            "duplicate enum name",
            source,
            path,
        )?;
    }
    for model in &schema.models {
        ensure_unique(
            &mut type_names,
            &model.name,
            model.span,
            "duplicate model name",
            source,
            path,
        )?;
    }
    if let Some(auth) = &schema.auth {
        ensure_unique(
            &mut type_names,
            &auth.name,
            auth.span,
            "duplicate auth type name",
            source,
            path,
        )?;
    }

    let mut procedure_names = BTreeSet::new();
    for procedure in &schema.procedures {
        if !procedure_names.insert(procedure.name.clone()) {
            return Err(span_error(
                format!("duplicate procedure name `{}`", procedure.name),
                procedure.span,
            ));
        }
    }

    if let Some(datasource) = &schema.datasource {
        let provider = datasource
            .entries
            .iter()
            .find(|entry| entry.key == "provider")
            .map(|entry| entry.value.trim_matches('"'));

        if let Some(provider) = provider
            && provider != "postgresql"
        {
            return Err(span_error(
                format!("unsupported datasource provider `{provider}`; expected `postgresql`"),
                datasource.span,
            ));
        }
    }

    let model_names = schema
        .models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<BTreeSet<_>>();
    let page_item_type_names = schema
        .models
        .iter()
        .map(|model| model.name.clone())
        .chain(schema.types.iter().map(|ty| ty.name.clone()))
        .collect::<BTreeSet<_>>();

    for model in &schema.models {
        let mut fields = BTreeMap::new();
        let mut has_primary_key = false;
        let mut saw_emit_attribute = false;
        let mut saw_paged_attribute = false;
        for field in &model.fields {
            if fields.insert(field.name.clone(), field.span).is_some() {
                return Err(span_error(
                    format!("duplicate field `{}` on model `{}`", field.name, model.name),
                    field.span,
                ));
            }
            if field
                .attributes
                .iter()
                .any(|attribute| attribute.raw.starts_with("@id"))
            {
                has_primary_key = true;
            }
            validate_custom_field_attribute(
                field,
                "model",
                &model.name,
                CustomFieldSupport::Rejected,
            )?;
            validate_type_ref(
                &type_names,
                &page_item_type_names,
                &field.ty,
                field.span,
                false,
            )?;

            let relation_attribute = field
                .attributes
                .iter()
                .find(|attribute| attribute.raw.starts_with("@relation("));
            if model_names.contains(field.ty.name.as_str()) {
                let relation_attribute = relation_attribute.ok_or_else(|| {
                    span_error(
                        format!(
                            "relation field `{}` on model `{}` must declare @relation(fields:[...],references:[...])",
                            field.name, model.name,
                        ),
                        field.span,
                    )
                })?;
                let relation = parse_relation_attribute(&relation_attribute.raw)
                    .map_err(|message| span_error(message, field.span))?;
                if relation.fields.len() != 1 || relation.references.len() != 1 {
                    return Err(span_error(
                        format!(
                            "relation field `{}` on model `{}` must declare exactly one local field and one reference in this slice",
                            field.name, model.name,
                        ),
                        field.span,
                    ));
                }

                let local_field = model
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == relation.fields[0])
                    .ok_or_else(|| {
                        span_error(
                            format!(
                                "relation field `{}` on model `{}` references unknown local field `{}`",
                                field.name, model.name, relation.fields[0],
                            ),
                            field.span,
                        )
                    })?;
                if model_names.contains(local_field.ty.name.as_str()) {
                    return Err(span_error(
                        format!(
                            "relation field `{}` on model `{}` must use a scalar local field, found relation field `{}`",
                            field.name, model.name, local_field.name,
                        ),
                        field.span,
                    ));
                }

                let target_model = schema
                    .models
                    .iter()
                    .find(|candidate| candidate.name == field.ty.name)
                    .ok_or_else(|| {
                        span_error(
                            format!(
                                "relation field `{}` on model `{}` references unknown target model `{}`",
                                field.name, model.name, field.ty.name,
                            ),
                            field.span,
                        )
                    })?;
                let target_field = target_model
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == relation.references[0])
                    .ok_or_else(|| {
                        span_error(
                            format!(
                                "relation field `{}` on model `{}` references unknown target field `{}` on `{}`",
                                field.name, model.name, relation.references[0], target_model.name,
                            ),
                            field.span,
                        )
                    })?;
                if model_names.contains(target_field.ty.name.as_str()) {
                    return Err(span_error(
                        format!(
                            "relation field `{}` on model `{}` must reference a scalar target field, found relation field `{}`",
                            field.name, model.name, target_field.name,
                        ),
                        field.span,
                    ));
                }
                validate_relation_scalar_compatibility(field, model, local_field, target_field)?;
            } else if relation_attribute.is_some() {
                return Err(span_error(
                    format!(
                        "scalar field `{}` on model `{}` cannot declare @relation(...)",
                        field.name, model.name,
                    ),
                    field.span,
                ));
            }
        }

        for attribute in &model.attributes {
            if attribute.raw.starts_with("@@emit(") {
                if saw_emit_attribute {
                    return Err(span_error(
                        format!(
                            "model `{}` must not declare more than one @@emit(...) attribute",
                            model.name
                        ),
                        attribute.span,
                    ));
                }
                parse_emit_attribute(&attribute.raw)
                    .map_err(|message| span_error(message, attribute.span))?;
                saw_emit_attribute = true;
            } else if attribute.raw.starts_with("@@paged") {
                if attribute.raw != "@@paged" {
                    return Err(span_error(
                        format!(
                            "model `{}` uses unsupported paging directive `{}`; use bare `@@paged` in this slice",
                            model.name, attribute.raw,
                        ),
                        attribute.span,
                    ));
                }
                if saw_paged_attribute {
                    return Err(span_error(
                        format!(
                            "model `{}` must not declare more than one @@paged attribute",
                            model.name
                        ),
                        attribute.span,
                    ));
                }
                saw_paged_attribute = true;
            }
        }

        if !has_primary_key {
            return Err(span_error(
                format!("model `{}` is missing an @id field", model.name),
                model.span,
            ));
        }
    }

    for ty in &schema.types {
        let mut fields = BTreeSet::new();
        for field in &ty.fields {
            if !fields.insert(field.name.clone()) {
                return Err(span_error(
                    format!("duplicate field `{}` on type `{}`", field.name, ty.name),
                    field.span,
                ));
            }
            validate_custom_field_attribute(field, "type", &ty.name, CustomFieldSupport::TypeOnly)?;
            validate_type_ref(
                &type_names,
                &page_item_type_names,
                &field.ty,
                field.span,
                false,
            )?;
        }
    }

    for enum_decl in &schema.enums {
        let mut variants = BTreeSet::new();
        for variant in &enum_decl.variants {
            if !variants.insert(variant.name.clone()) {
                return Err(span_error(
                    format!(
                        "duplicate variant `{}` on enum `{}`",
                        variant.name, enum_decl.name
                    ),
                    variant.span,
                ));
            }
        }
    }

    if let Some(auth) = &schema.auth {
        let mut fields = BTreeSet::new();
        for field in &auth.fields {
            if !fields.insert(field.name.clone()) {
                return Err(span_error(
                    format!(
                        "duplicate field `{}` on auth block `{}`",
                        field.name, auth.name
                    ),
                    field.span,
                ));
            }
            validate_custom_field_attribute(
                field,
                "auth block",
                &auth.name,
                CustomFieldSupport::Rejected,
            )?;
            validate_type_ref(
                &type_names,
                &page_item_type_names,
                &field.ty,
                field.span,
                false,
            )?;
        }
    }

    for procedure in &schema.procedures {
        for arg in &procedure.args {
            validate_type_ref(
                &type_names,
                &page_item_type_names,
                &arg.ty,
                procedure.span,
                false,
            )?;
        }
        validate_type_ref(
            &type_names,
            &page_item_type_names,
            &procedure.return_type,
            procedure.span,
            true,
        )?;
    }

    let _ = (path, source);
    Ok(())
}

#[derive(Clone, Copy)]
enum CustomFieldSupport {
    Rejected,
    TypeOnly,
}

fn validate_custom_field_attribute(
    field: &Field,
    owner_kind: &str,
    owner_name: &str,
    support: CustomFieldSupport,
) -> Result<(), SchemaError> {
    let mut custom_count = 0usize;
    for attribute in &field.attributes {
        if !attribute.raw.starts_with("@custom") {
            continue;
        }
        custom_count += 1;
        if attribute.raw != "@custom" {
            return Err(span_error(
                format!(
                    "field `{}` on {} `{}` uses unsupported custom field directive `{}`; use bare `@custom` in this slice",
                    field.name, owner_kind, owner_name, attribute.raw,
                ),
                field.span,
            ));
        }
        if matches!(support, CustomFieldSupport::Rejected) {
            return Err(span_error(
                format!(
                    "field `{}` on {} `{}` cannot use `@custom`; resolver-backed custom fields are currently only supported on `type` declarations",
                    field.name, owner_kind, owner_name,
                ),
                field.span,
            ));
        }
    }

    if custom_count > 1 {
        return Err(span_error(
            format!(
                "field `{}` on {} `{}` declares `@custom` more than once",
                field.name, owner_kind, owner_name,
            ),
            field.span,
        ));
    }

    Ok(())
}

fn ensure_unique(
    names: &mut BTreeSet<String>,
    name: &str,
    span: SourceSpan,
    message: &str,
    _source: &str,
    _path: &str,
) -> Result<(), SchemaError> {
    if !names.insert(name.to_owned()) {
        return Err(span_error(format!("{message} `{name}`"), span));
    }
    Ok(())
}

fn validate_type_ref(
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
    type_ref: &TypeRef,
    span: SourceSpan,
    allow_page: bool,
) -> Result<(), SchemaError> {
    if type_ref.is_page() {
        if !allow_page {
            return Err(span_error(
                "built-in `Page<T>` is currently only supported as a procedure return type"
                    .to_owned(),
                span,
            ));
        }
        if type_ref.arity != cratestack_core::TypeArity::Required {
            return Err(span_error(
                "built-in `Page<T>` cannot be optional or list-valued".to_owned(),
                span,
            ));
        }
        let Some(item) = type_ref.page_item() else {
            return Err(span_error(
                "built-in `Page<T>` requires exactly one inner type".to_owned(),
                span,
            ));
        };
        if item.is_page() {
            return Err(span_error(
                "nested `Page<Page<T>>` return types are unsupported".to_owned(),
                span,
            ));
        }
        if item.arity != cratestack_core::TypeArity::Required {
            return Err(span_error(
                "built-in `Page<T>` requires a required model or type item".to_owned(),
                span,
            ));
        }
        if !item.generic_args.is_empty() {
            return Err(span_error(
                "built-in `Page<T>` only supports a direct model or type item".to_owned(),
                span,
            ));
        }
        if !page_item_type_names.contains(&item.name) {
            return Err(span_error(
                format!(
                    "built-in `Page<T>` only supports declared model or type items; `{}` is unsupported",
                    item.name
                ),
                span,
            ));
        }
        return Ok(());
    }

    if !type_ref.generic_args.is_empty() {
        return Err(span_error(
            format!("unsupported generic type `{}`", type_ref.name),
            span,
        ));
    }
    if !type_names.contains(&type_ref.name) {
        return Err(span_error(
            format!("unknown type `{}`", type_ref.name),
            span,
        ));
    }
    Ok(())
}
