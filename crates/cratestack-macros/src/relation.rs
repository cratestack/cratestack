use std::collections::BTreeSet;

use cratestack_core::{Field, Model, TypeArity};
use quote::quote;

use crate::shared::{
    find_model, ident, is_relation_field, model_name_set, pluralize, relation_model_fields,
    rust_type_tokens, scalar_model_fields, to_snake_case,
};

mod filter_builders;

#[derive(Clone)]
pub(crate) struct RelationLink {
    pub(crate) parent_table: String,
    pub(crate) parent_column: String,
    pub(crate) related_table: String,
    pub(crate) related_column: String,
    pub(crate) is_to_many: bool,
}

#[derive(Clone, Copy)]
enum RelationFilterWrapperKind {
    ToOne,
    Some,
    Every,
    None,
}

#[derive(Clone)]
struct RelationPathSegment {
    link: RelationLink,
    kind: RelationFilterWrapperKind,
}

type RelationModuleEntry = (proc_macro2::TokenStream, proc_macro2::TokenStream);

pub(crate) struct ParsedRelationAttribute {
    pub(crate) fields: Vec<String>,
    pub(crate) references: Vec<String>,
}

pub(crate) fn relation_visit_key(model: &Model, relation_field: &Field) -> String {
    format!("{}.{}", model.name, relation_field.name)
}

pub(crate) fn relation_link(
    model: &Model,
    relation_field: &Field,
    models: &[Model],
) -> Result<RelationLink, String> {
    let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            relation_field.name, model.name, relation_field.ty.name,
        )
    })?;
    let parent_table = pluralize(&to_snake_case(&model.name));
    let related_table = pluralize(&to_snake_case(&target_model.name));
    let relation = parse_relation_attribute(relation_field).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` must declare @relation(fields:[...],references:[...])",
            relation_field.name, model.name,
        )
    })?;
    if relation.fields.len() != 1 || relation.references.len() != 1 {
        return Err(format!(
            "relation field `{}` on `{}` must declare exactly one local field and one reference in this slice",
            relation_field.name, model.name,
        ));
    }

    let local_field = model
        .fields
        .iter()
        .find(|field| field.name == relation.fields[0])
        .ok_or_else(|| {
            format!(
                "relation field `{}` on `{}` references unknown local field `{}`",
                relation_field.name, model.name, relation.fields[0],
            )
        })?;
    let target_field = target_model
        .fields
        .iter()
        .find(|field| field.name == relation.references[0])
        .ok_or_else(|| {
            format!(
                "relation field `{}` on `{}` references unknown target field `{}` on `{}`",
                relation_field.name, model.name, relation.references[0], target_model.name,
            )
        })?;
    if local_field.ty.name != target_field.ty.name {
        return Err(format!(
            "relation field `{}` on `{}` links incompatible scalar types: local field `{}` is `{}` but referenced field `{}` is `{}`",
            relation_field.name,
            model.name,
            local_field.name,
            local_field.ty.name,
            target_field.name,
            target_field.ty.name,
        ));
    }

    Ok(RelationLink {
        parent_table,
        parent_column: to_snake_case(&local_field.name),
        related_table,
        related_column: to_snake_case(&target_field.name),
        is_to_many: relation_field.ty.arity == TypeArity::List,
    })
}

pub(crate) fn parse_relation_attribute(field: &Field) -> Option<ParsedRelationAttribute> {
    let raw = field
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@relation("))?
        .raw
        .as_str();
    let inner = raw.strip_prefix("@relation(")?.strip_suffix(')')?;

    let mut fields = None;
    let mut references = None;
    for entry in split_top_level(inner, ',') {
        let (key, value) = entry.split_once(':')?;
        match key.trim() {
            "fields" => fields = Some(parse_relation_list(value.trim())?),
            "references" => references = Some(parse_relation_list(value.trim())?),
            _ => return None,
        }
    }

    Some(ParsedRelationAttribute {
        fields: fields?,
        references: references?,
    })
}

fn parse_relation_list(value: &str) -> Option<Vec<String>> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    let values = inner
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn split_top_level(input: &str, separator: char) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in input.char_indices() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            ch if ch == separator && depth == 0 => {
                entries.push(input[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    entries.push(input[start..].trim());
    entries
        .into_iter()
        .filter(|entry| !entry.is_empty())
        .collect()
}

pub(crate) fn collect_allowed_sort_keys(
    model: &Model,
    models: &[Model],
) -> Result<Vec<String>, String> {
    let table_name = pluralize(&to_snake_case(&model.name));
    collect_relation_order_targets(model, models, &table_name, "").map(|targets| {
        targets
            .into_iter()
            .filter_map(|(key, _)| key.strip_prefix('.').map(str::to_owned))
            .collect()
    })
}

pub(crate) fn generate_relation_query_guard(
    model: &Model,
    relation_field: &Field,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let model_name = &model.name;
    let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            relation_field.name, model.name, relation_field.ty.name,
        )
    })?;
    let target_filter_builder_ident = ident(&format!(
        "build_{}_filter_expr",
        to_snake_case(&target_model.name)
    ));
    let relation_prefix = format!("{}.", relation_field.name);
    let relation_link = relation_link(model, relation_field, models)?;
    let parent_table = relation_link.parent_table;
    let parent_column = relation_link.parent_column;
    let related_table = relation_link.related_table;
    let related_column = relation_link.related_column;

    if relation_link.is_to_many {
        let relation_field_name = &relation_field.name;

        return Ok(quote! {
            if let Some(rest) = key.strip_prefix(#relation_prefix) {
                let (operator, nested_key) = rest.split_once('.').ok_or_else(|| {
                    CoolError::BadRequest(format!(
                        "to-many relation filter '{}.{}' must use one of some, every, or none before the target field",
                        #model_name,
                        #relation_field_name,
                    ))
                })?;
                return match operator {
                    "some" => Ok(::cratestack::FilterExpr::relation_some(
                        #parent_table,
                        #parent_column,
                        #related_table,
                        #related_column,
                        #target_filter_builder_ident(nested_key, value)?,
                    )),
                    "every" => Ok(::cratestack::FilterExpr::relation_every(
                        #parent_table,
                        #parent_column,
                        #related_table,
                        #related_column,
                        #target_filter_builder_ident(nested_key, value)?,
                    )),
                    "none" => Ok(::cratestack::FilterExpr::relation_none(
                        #parent_table,
                        #parent_column,
                        #related_table,
                        #related_column,
                        #target_filter_builder_ident(nested_key, value)?,
                    )),
                    _ => Err(CoolError::BadRequest(format!(
                        "unsupported to-many relation operator '{}' for {}.{}; expected some, every, or none",
                        operator,
                        #model_name,
                        #relation_field_name,
                    ))),
                };
            }
        });
    }

    Ok(quote! {
        if let Some(rest) = key.strip_prefix(#relation_prefix) {
            return Ok(::cratestack::FilterExpr::relation(
                #parent_table,
                #parent_column,
                #related_table,
                #related_column,
                #target_filter_builder_ident(rest, value)?,
            ));
        }
    })
}

pub(crate) fn generate_relation_order_by_arms(
    model: &Model,
    relation_field: &Field,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let arms = collect_relation_order_by_arms(model, relation_field, models, None)?;

    Ok(quote! {
        #(#arms)*
    })
}

fn collect_relation_order_by_arms(
    model: &Model,
    relation_field: &Field,
    models: &[Model],
    prefix: Option<&str>,
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    let relation_link = relation_link(model, relation_field, models)?;
    if relation_link.is_to_many {
        return Ok(Vec::new());
    }

    let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            relation_field.name, model.name, relation_field.ty.name,
        )
    })?;
    let key_prefix = match prefix {
        Some(prefix) => format!("{}.{}", prefix, relation_field.name),
        None => relation_field.name.clone(),
    };
    let targets = collect_relation_order_targets(
        target_model,
        models,
        relation_link.related_table.as_str(),
        &key_prefix,
    )?;

    Ok(targets
        .into_iter()
        .map(|(key, value_sql)| {
            let parent_table = relation_link.parent_table.as_str();
            let parent_column = relation_link.parent_column.as_str();
            let related_table = relation_link.related_table.as_str();
            let related_column = relation_link.related_column.as_str();
            quote! {
                #key => {
                    request.order_by(::cratestack::OrderClause::relation_scalar(
                        #parent_table,
                        #parent_column,
                        #related_table,
                        #related_column,
                        #value_sql,
                        if descending {
                            ::cratestack::SortDirection::Desc
                        } else {
                            ::cratestack::SortDirection::Asc
                        },
                    ))
                }
            }
        })
        .collect())
}

pub(crate) fn generate_relation_order_module(
    model: &Model,
    relation_field: &Field,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let relation_link = relation_link(model, relation_field, models)?;
    let root_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            relation_field.name, model.name, relation_field.ty.name,
        )
    })?;

    if relation_link.is_to_many {
        return generate_relation_quantifier_container_module(
            model,
            root_model,
            relation_field,
            &[],
            &[relation_visit_key(model, relation_field)],
            models,
        );
    }

    let wrappers = vec![RelationPathSegment {
        link: relation_link.clone(),
        kind: RelationFilterWrapperKind::ToOne,
    }];
    let visited = vec![relation_visit_key(model, relation_field)];

    // Codegen sugar for `.find_many().include(...)`: emit an
    // `as_include()` method on the root-level relation `Path` so call
    // sites can write `cool.parent().find_many().include(
    // parent_module::relation_name().as_include())` without
    // hand-rolling the `RelationInclude` literal. Only emitted for
    // to-one relations whose `@relation(references:[...])` is the
    // related model's primary key (the schema norm); other shapes
    // silently skip the method so the build stays clean while the
    // hand-built literal continues to work.
    let as_include_method =
        generate_as_include_method(model, relation_field, root_model, models)?;
    let root_extra: Vec<proc_macro2::TokenStream> =
        as_include_method.into_iter().collect();

    generate_relation_order_module_recursive(
        &relation_link,
        root_model,
        root_model,
        relation_link.related_table.as_str(),
        &[],
        relation_field,
        &wrappers,
        &visited,
        models,
        &root_extra,
    )
}

/// Emit the per-relation `as_include()` method body. Returns `None`
/// when the relation isn't eligible for the typed `.include(...)`
/// shortcut — for example, when the schema references a non-PK column,
/// or when the related model has no `@id` field. Eligible shape: a
/// to-one relation whose `@relation(references:[<col>])` names the
/// related model's primary key. The call site already gates on
/// `is_to_many` before reaching here, so we only need to validate the
/// PK alignment in this helper.
fn generate_as_include_method(
    model: &Model,
    relation_field: &Field,
    related_model: &Model,
    _models: &[Model],
) -> Result<Option<proc_macro2::TokenStream>, String> {
    let parsed = match parse_relation_attribute(relation_field) {
        Some(p) => p,
        None => return Ok(None),
    };
    if parsed.fields.len() != 1 || parsed.references.len() != 1 {
        return Ok(None);
    }
    let fk_field_name = &parsed.fields[0];
    let ref_field_name = &parsed.references[0];

    let related_pk = match related_model
        .fields
        .iter()
        .find(|field| crate::shared::is_primary_key(field))
    {
        Some(pk) => pk,
        None => return Ok(None),
    };
    // Only support reference-equals-PK for v1. Other shapes (unique
    // non-PK references) need a different `RelationInclude` field set
    // and stay on the hand-built literal path.
    if ref_field_name != &related_pk.name {
        return Ok(None);
    }

    let fk_field = match model.fields.iter().find(|field| &field.name == fk_field_name) {
        Some(f) => f,
        None => return Ok(None),
    };

    let parent_ident = ident(&model.name);
    let related_ident = ident(&related_model.name);
    let related_pk_type = rust_type_tokens(&related_pk.ty);
    let related_descriptor_ident = ident(&format!(
        "{}_MODEL",
        to_snake_case(&related_model.name).to_uppercase(),
    ));
    let fk_field_ident = ident(&fk_field.name);

    // Optional FK ⇒ field type is already `Option<RelPK>`. Required
    // FK ⇒ wrap in `Some(...)` so the function pointer's return type
    // is the same shape in both cases.
    let fk_extract_body = if fk_field.ty.arity == TypeArity::Optional {
        quote::quote! { m.#fk_field_ident.clone() }
    } else {
        quote::quote! { ::std::option::Option::Some(m.#fk_field_ident.clone()) }
    };

    Ok(Some(quote! {
        /// Build a `RelationInclude` for this to-one relation, ready
        /// to feed into a `.find_many().include(...)` chain. Equivalent
        /// to hand-rolling the struct literal that names the parent
        /// FK extractor and the related descriptor.
        pub fn as_include(self) -> ::cratestack::RelationInclude<
            super::#parent_ident,
            super::#related_ident,
            #related_pk_type,
        > {
            ::cratestack::RelationInclude {
                parent_fk_extract: |m: &super::#parent_ident| #fk_extract_body,
                related_descriptor: &super::#related_descriptor_ident,
            }
        }
    }))
}

pub(crate) fn generate_relation_include_arm(
    model: &Model,
    relation_field: &Field,
    models: &[Model],
    _project_serialized_value_ident: &syn::Ident,
) -> Result<proc_macro2::TokenStream, String> {
    let relation = parse_relation_attribute(relation_field).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` must declare @relation(fields:[...],references:[...])",
            relation_field.name, model.name,
        )
    })?;
    if relation.fields.len() != 1 || relation.references.len() != 1 {
        return Err(format!(
            "relation field `{}` on `{}` must declare exactly one local field and one reference in this slice",
            relation_field.name, model.name,
        ));
    }

    let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            relation_field.name, model.name, relation_field.ty.name,
        )
    })?;
    let include_name = &relation_field.name;
    let model_name = &model.name;
    let target_accessor_ident = ident(&to_snake_case(&target_model.name));
    let target_field_module_ident = ident(&to_snake_case(&target_model.name));
    let target_field_fn_ident = ident(&relation.references[0]);
    let local_field_ident = ident(&relation.fields[0]);
    let target_serialize_ident = ident(&format!(
        "serialize_{}_model_value",
        to_snake_case(&target_model.name)
    ));

    if relation_field.ty.arity == TypeArity::List {
        return Ok(quote! {
            #include_name => {
                let child_selection = selection.selection_for_include(#include_name).ok_or_else(|| {
                    CoolError::Internal(format!(
                        "validated selection for '{}.{}' is missing child selection",
                        #model_name,
                        #include_name,
                    ))
                })?;
                let related_records = db
                    .#target_accessor_ident()
                    .find_many()
                    .where_(super::#target_field_module_ident::#target_field_fn_ident().eq(record.#local_field_ident.clone()))
                    .run(ctx)
                    .await?;
                let mut related_value = Vec::with_capacity(related_records.len());
                for related_record in &related_records {
                    related_value.push(#target_serialize_ident(db, ctx, related_record, &child_selection).await?);
                }
                let related_value = ::cratestack::serde_json::Value::Array(related_value);
                object.insert(#include_name.to_owned(), related_value);
            }
        });
    }

    let local_field = model
        .fields
        .iter()
        .find(|field| field.name == relation.fields[0])
        .ok_or_else(|| {
            format!(
                "relation field `{}` on `{}` references unknown local field `{}`",
                relation_field.name, model.name, relation.fields[0],
            )
        })?;

    if local_field.ty.arity == TypeArity::Optional {
        Ok(quote! {
            #include_name => {
                let child_selection = selection.selection_for_include(#include_name).ok_or_else(|| {
                    CoolError::Internal(format!(
                        "validated selection for '{}.{}' is missing child selection",
                        #model_name,
                        #include_name,
                    ))
                })?;
                let related_value = match record.#local_field_ident.clone() {
                    Some(value) => {
                        let related_record = db
                            .#target_accessor_ident()
                            .find_many()
                            .where_(super::#target_field_module_ident::#target_field_fn_ident().eq(value))
                            .run(ctx)
                            .await?
                            .into_iter()
                            .next();
                        match related_record {
                            Some(related_record) => #target_serialize_ident(db, ctx, &related_record, &child_selection).await?,
                            None => ::cratestack::serde_json::Value::Null,
                        }
                    }
                    None => ::cratestack::serde_json::Value::Null,
                };
                object.insert(#include_name.to_owned(), related_value);
            }
        })
    } else {
        Ok(quote! {
            #include_name => {
                let child_selection = selection.selection_for_include(#include_name).ok_or_else(|| {
                    CoolError::Internal(format!(
                        "validated selection for '{}.{}' is missing child selection",
                        #model_name,
                        #include_name,
                    ))
                })?;
                let related_record = db
                    .#target_accessor_ident()
                    .find_many()
                    .where_(super::#target_field_module_ident::#target_field_fn_ident().eq(record.#local_field_ident.clone()))
                    .run(ctx)
                    .await?
                    .into_iter()
                    .next();
                let related_value = match related_record {
                    Some(related_record) => #target_serialize_ident(db, ctx, &related_record, &child_selection).await?,
                    None => ::cratestack::serde_json::Value::Null,
                };
                object.insert(#include_name.to_owned(), related_value);
            }
        })
    }
}

pub(crate) fn generate_relation_include_path_validation_arm(
    relation_field: &Field,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let include_name = &relation_field.name;
    let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` references unknown model `{}`",
            relation_field.name, relation_field.ty.name,
        )
    })?;
    let target_validate_include_path_ident = ident(&format!(
        "validate_{}_include_path",
        to_snake_case(&target_model.name)
    ));
    let target_descriptor_ident = ident(&format!(
        "{}_MODEL",
        to_snake_case(&target_model.name).to_uppercase()
    ));

    Ok(quote! {
        (#include_name, Some(rest)) => {
            #target_validate_include_path_ident(rest, &super::models::#target_descriptor_ident)
        }
    })
}

pub(crate) fn generate_relation_include_fields_validation_arm(
    relation_field: &Field,
    model: &Model,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let include_name = &relation_field.name;
    let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` references unknown model `{}`",
            relation_field.name, relation_field.ty.name,
        )
    })?;
    let model_names = model_name_set(models);
    let allowed_fields = scalar_model_fields(target_model, &model_names)
        .into_iter()
        .map(|field| {
            let name = &field.name;
            quote! { #name }
        })
        .collect::<Vec<_>>();
    let target_validate_include_fields_path_ident = ident(&format!(
        "validate_{}_include_fields_path",
        to_snake_case(&target_model.name)
    ));
    let target_descriptor_ident = ident(&format!(
        "{}_MODEL",
        to_snake_case(&target_model.name).to_uppercase()
    ));
    let parent_model_name = &model.name;

    Ok(quote! {
        (#include_name, Some(rest)) => {
            #target_validate_include_fields_path_ident(rest, fields, &super::models::#target_descriptor_ident)
        }
        (#include_name, None) => {
            for field in fields {
                match field.as_str() {
                    #(#allowed_fields)|* => {}
                    _ => return Err(CoolError::Validation(format!(
                        "unsupported includeFields[{}] selection '{}' for {}.{}",
                        include,
                        field,
                        #parent_model_name,
                        #include_name,
                    ))),
                }
            }
            Ok(())
        }
    })
}

fn generate_relation_order_module_recursive(
    root_link: &RelationLink,
    root_model: &Model,
    current_model: &Model,
    root_table: &str,
    path_prefix: &[String],
    relation_field: &Field,
    wrappers: &[RelationPathSegment],
    visited: &[String],
    models: &[Model],
    root_extra_path_methods: &[proc_macro2::TokenStream],
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(&relation_field.name);
    let model_names = model_name_set(models);
    let allow_ordering = wrappers_allow_ordering(wrappers);
    let scalar_fns = generate_relation_scalar_order_functions(
        current_model,
        &model_names,
        root_link,
        root_model,
        root_table,
        path_prefix,
        models,
        allow_ordering,
    )?;
    let scalar_filter_fns = generate_relation_filter_functions(current_model, wrappers, models)?;
    let scalar_builder_modules = generate_relation_scalar_builder_modules(
        current_model,
        &model_names,
        wrappers,
        allow_ordering,
        root_link,
        root_model,
        root_table,
        path_prefix,
        models,
    )?;
    let scalar_path_methods = scalar_model_fields(current_model, &model_names)
        .into_iter()
        .map(generate_scalar_relation_path_method)
        .collect::<Vec<_>>();
    let relation_entries = collect_recursive_relation_entries(
        current_model,
        &model_names,
        visited,
        wrappers,
        path_prefix,
        root_link,
        root_model,
        root_table,
        models,
    )?;
    let relation_path_methods = relation_entries
        .iter()
        .map(|(method, _)| method.clone())
        .collect::<Vec<_>>();
    let relation_modules = relation_entries
        .into_iter()
        .map(|(_, module)| module)
        .collect::<Vec<_>>();

    Ok(quote! {
        pub mod #module_ident {
            pub use super::*;

            pub struct Path;

            impl Path {
                #(#scalar_path_methods)*
                #(#relation_path_methods)*
                #(#root_extra_path_methods)*
            }

            #(#scalar_fns)*
            #(#scalar_filter_fns)*
            #(#scalar_builder_modules)*
            #(#relation_modules)*
        }
    })
}

fn wrappers_allow_ordering(wrappers: &[RelationPathSegment]) -> bool {
    wrappers
        .iter()
        .all(|segment| matches!(segment.kind, RelationFilterWrapperKind::ToOne))
}

fn generate_relation_scalar_order_functions(
    current_model: &Model,
    model_names: &BTreeSet<&str>,
    root_link: &RelationLink,
    root_model: &Model,
    root_table: &str,
    path_prefix: &[String],
    models: &[Model],
    allow_ordering: bool,
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    if !allow_ordering {
        return Ok(Vec::new());
    }

    scalar_model_fields(current_model, model_names)
        .into_iter()
        .map(|field| {
            let asc_ident = ident(&format!("{}_asc", field.name));
            let desc_ident = ident(&format!("{}_desc", field.name));
            let mut path = path_prefix.to_vec();
            path.push(field.name.clone());
            let value_sql =
                relation_order_value_sql_for_path(root_model, models, root_table, &path)?;
            let parent_table = root_link.parent_table.as_str();
            let parent_column = root_link.parent_column.as_str();
            let related_table = root_link.related_table.as_str();
            let related_column = root_link.related_column.as_str();

            Ok(quote! {
                #[allow(non_snake_case)]
                pub fn #asc_ident() -> ::cratestack::OrderClause {
                    ::cratestack::OrderClause::relation_scalar(
                        #parent_table,
                        #parent_column,
                        #related_table,
                        #related_column,
                        #value_sql,
                        ::cratestack::SortDirection::Asc,
                    )
                }

                #[allow(non_snake_case)]
                pub fn #desc_ident() -> ::cratestack::OrderClause {
                    ::cratestack::OrderClause::relation_scalar(
                        #parent_table,
                        #parent_column,
                        #related_table,
                        #related_column,
                        #value_sql,
                        ::cratestack::SortDirection::Desc,
                    )
                }
            })
        })
        .collect()
}

fn generate_relation_scalar_builder_modules(
    current_model: &Model,
    model_names: &BTreeSet<&str>,
    wrappers: &[RelationPathSegment],
    allow_ordering: bool,
    root_link: &RelationLink,
    root_model: &Model,
    root_table: &str,
    path_prefix: &[String],
    models: &[Model],
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    scalar_model_fields(current_model, model_names)
        .into_iter()
        .map(|field| {
            generate_scalar_relation_builder_module(
                field,
                wrappers,
                allow_ordering,
                root_link,
                root_model,
                root_table,
                path_prefix,
                models,
            )
        })
        .collect()
}

fn collect_recursive_relation_entries(
    current_model: &Model,
    model_names: &BTreeSet<&str>,
    visited: &[String],
    wrappers: &[RelationPathSegment],
    path_prefix: &[String],
    root_link: &RelationLink,
    root_model: &Model,
    root_table: &str,
    models: &[Model],
) -> Result<Vec<RelationModuleEntry>, String> {
    relation_model_fields(current_model, model_names)
        .into_iter()
        .map(|nested_relation| {
            build_recursive_relation_entry(
                current_model,
                nested_relation,
                visited,
                wrappers,
                path_prefix,
                root_link,
                root_model,
                root_table,
                models,
            )
        })
        .collect::<Result<Vec<_>, String>>()
        .map(|entries| entries.into_iter().flatten().collect())
}

#[allow(clippy::too_many_arguments)]
fn build_recursive_relation_entry(
    current_model: &Model,
    nested_relation: &Field,
    visited: &[String],
    wrappers: &[RelationPathSegment],
    path_prefix: &[String],
    root_link: &RelationLink,
    root_model: &Model,
    root_table: &str,
    models: &[Model],
) -> Result<Option<RelationModuleEntry>, String> {
    let nested_link = relation_link(current_model, nested_relation, models)?;
    let nested_key = relation_visit_key(current_model, nested_relation);
    if visited.contains(&nested_key) {
        return Ok(None);
    }

    if nested_link.is_to_many {
        let nested_model = find_model_or_err(current_model, nested_relation, models)?;
        let module = generate_relation_quantifier_container_module(
            current_model,
            nested_model,
            nested_relation,
            wrappers,
            visited,
            models,
        )?;
        return Ok(Some((
            generate_nested_relation_path_method(nested_relation),
            module,
        )));
    }

    let nested_model = find_model_or_err(current_model, nested_relation, models)?;
    let mut nested_path = path_prefix.to_vec();
    nested_path.push(nested_relation.name.clone());
    let mut nested_wrappers = wrappers.to_vec();
    nested_wrappers.push(RelationPathSegment {
        link: nested_link,
        kind: RelationFilterWrapperKind::ToOne,
    });
    let mut nested_visited = visited.to_vec();
    nested_visited.push(nested_key);
    let module = generate_relation_order_module_recursive(
        root_link,
        root_model,
        nested_model,
        root_table,
        &nested_path,
        nested_relation,
        &nested_wrappers,
        &nested_visited,
        models,
        &[],
    )?;
    Ok(Some((
        generate_nested_relation_path_method(nested_relation),
        module,
    )))
}

fn find_model_or_err<'a>(
    current_model: &Model,
    relation_field: &Field,
    models: &'a [Model],
) -> Result<&'a Model, String> {
    find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            relation_field.name, current_model.name, relation_field.ty.name,
        )
    })
}

fn generate_relation_quantifier_container_module(
    parent_model: &Model,
    target_model: &Model,
    relation_field: &Field,
    parent_wrappers: &[RelationPathSegment],
    visited: &[String],
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(&relation_field.name);
    let some = generate_relation_quantifier_module(
        parent_model,
        target_model,
        relation_field,
        parent_wrappers,
        RelationFilterWrapperKind::Some,
        "some",
        visited,
        models,
    )?;
    let every = generate_relation_quantifier_module(
        parent_model,
        target_model,
        relation_field,
        parent_wrappers,
        RelationFilterWrapperKind::Every,
        "every",
        visited,
        models,
    )?;
    let none = generate_relation_quantifier_module(
        parent_model,
        target_model,
        relation_field,
        parent_wrappers,
        RelationFilterWrapperKind::None,
        "none",
        visited,
        models,
    )?;

    Ok(quote! {
        pub mod #module_ident {
            pub use super::*;

            pub struct Path;

            impl Path {
                pub fn some(self) -> self::some::Path {
                    self::some::Path
                }

                pub fn every(self) -> self::every::Path {
                    self::every::Path
                }

                pub fn none(self) -> self::none::Path {
                    self::none::Path
                }
            }

            #some
            #every
            #none
        }
    })
}

fn generate_relation_quantifier_module(
    parent_model: &Model,
    target_model: &Model,
    relation_field: &Field,
    parent_wrappers: &[RelationPathSegment],
    kind: RelationFilterWrapperKind,
    module_name: &str,
    visited: &[String],
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(module_name);
    let link = relation_link(parent_model, relation_field, models)?;
    let mut wrappers = parent_wrappers.to_vec();
    wrappers.push(RelationPathSegment { link, kind });
    let scalar_filter_fns = generate_relation_filter_functions(target_model, &wrappers, models)?;
    let model_names = model_name_set(models);
    let scalar_builder_modules = scalar_model_fields(target_model, &model_names)
        .into_iter()
        .map(|field| {
            generate_scalar_relation_builder_module(
                field,
                &wrappers,
                false,
                &wrappers[0].link,
                target_model,
                wrappers[0].link.related_table.as_str(),
                &[],
                models,
            )
        })
        .collect::<Result<Vec<_>, String>>()?;
    let scalar_path_methods = scalar_model_fields(target_model, &model_names)
        .into_iter()
        .map(generate_scalar_relation_path_method)
        .collect::<Vec<_>>();
    let relation_entries = collect_quantifier_relation_entries(
        target_model,
        &model_names,
        visited,
        &wrappers,
        models,
    )?;
    let relation_path_methods = relation_entries
        .iter()
        .map(|(method, _)| method.clone())
        .collect::<Vec<_>>();
    let relation_modules = relation_entries
        .into_iter()
        .map(|(_, module)| module)
        .collect::<Vec<_>>();

    Ok(quote! {
        pub mod #module_ident {
            pub use super::*;

            pub struct Path;

            impl Path {
                #(#scalar_path_methods)*
                #(#relation_path_methods)*
            }

            #(#scalar_filter_fns)*
            #(#scalar_builder_modules)*
            #(#relation_modules)*
        }
    })
}

fn collect_quantifier_relation_entries(
    target_model: &Model,
    model_names: &BTreeSet<&str>,
    visited: &[String],
    wrappers: &[RelationPathSegment],
    models: &[Model],
) -> Result<Vec<RelationModuleEntry>, String> {
    relation_model_fields(target_model, model_names)
        .into_iter()
        .map(|nested_relation| {
            build_quantifier_relation_entry(
                target_model,
                nested_relation,
                visited,
                wrappers,
                models,
            )
        })
        .collect::<Result<Vec<_>, String>>()
        .map(|entries| entries.into_iter().flatten().collect())
}

fn build_quantifier_relation_entry(
    target_model: &Model,
    nested_relation: &Field,
    visited: &[String],
    wrappers: &[RelationPathSegment],
    models: &[Model],
) -> Result<Option<RelationModuleEntry>, String> {
    let nested_key = relation_visit_key(target_model, nested_relation);
    if visited.contains(&nested_key) {
        return Ok(None);
    }
    let mut nested_visited = visited.to_vec();
    nested_visited.push(nested_key);
    let nested_model = find_model_or_err(target_model, nested_relation, models)?;
    let nested_link = relation_link(target_model, nested_relation, models)?;

    if nested_link.is_to_many {
        let module = generate_relation_quantifier_container_module(
            target_model,
            nested_model,
            nested_relation,
            wrappers,
            &nested_visited,
            models,
        )?;
        return Ok(Some((
            generate_nested_relation_path_method(nested_relation),
            module,
        )));
    }

    let root_link = wrappers[0].link.clone();
    let mut nested_wrappers = wrappers.to_vec();
    nested_wrappers.push(RelationPathSegment {
        link: nested_link,
        kind: RelationFilterWrapperKind::ToOne,
    });
    let module = generate_relation_order_module_recursive(
        &root_link,
        target_model,
        nested_model,
        root_link.related_table.as_str(),
        &[nested_relation.name.clone()],
        nested_relation,
        &nested_wrappers,
        &nested_visited,
        models,
        &[],
    )?;
    Ok(Some((
        generate_nested_relation_path_method(nested_relation),
        module,
    )))
}

fn generate_scalar_relation_path_method(field: &Field) -> proc_macro2::TokenStream {
    let method_ident = ident(&field.name);
    let module_ident = ident(&field.name);

    quote! {
        #[allow(non_snake_case)]
        pub fn #method_ident(self) -> self::#module_ident::Field {
            self::#module_ident::Field
        }
    }
}

fn generate_nested_relation_path_method(field: &Field) -> proc_macro2::TokenStream {
    let method_ident = ident(&field.name);
    let module_ident = ident(&field.name);

    quote! {
        #[allow(non_snake_case)]
        pub fn #method_ident(self) -> self::#module_ident::Path {
            self::#module_ident::Path
        }
    }
}

fn generate_scalar_relation_builder_module(
    field: &Field,
    wrappers: &[RelationPathSegment],
    allow_ordering: bool,
    root_link: &RelationLink,
    root_model: &Model,
    root_table: &str,
    path_prefix: &[String],
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(&field.name);
    let field_type = rust_type_tokens(&field.ty);
    let column = to_snake_case(&field.name);
    let mut methods = Vec::new();

    filter_builders::append_required_builder_methods(
        &mut methods,
        field,
        wrappers,
        &field_type,
        &column,
    );
    filter_builders::append_boolean_builder_methods(
        &mut methods,
        field,
        wrappers,
        &field_type,
        &column,
    );
    filter_builders::append_required_text_builder_methods(
        &mut methods,
        field,
        wrappers,
        &field_type,
        &column,
    );
    filter_builders::append_optional_builder_methods(
        &mut methods,
        field,
        wrappers,
        &field_type,
        &column,
    );
    filter_builders::append_optional_string_builder_methods(
        &mut methods,
        field,
        wrappers,
        &field_type,
        &column,
    );

    if allow_ordering {
        let mut path = path_prefix.to_vec();
        path.push(field.name.clone());
        let value_sql = relation_order_value_sql_for_path(root_model, models, root_table, &path)?;
        let parent_table = root_link.parent_table.as_str();
        let parent_column = root_link.parent_column.as_str();
        let related_table = root_link.related_table.as_str();
        let related_column = root_link.related_column.as_str();
        methods.push(quote! {
            pub fn asc(self) -> ::cratestack::OrderClause {
                ::cratestack::OrderClause::relation_scalar(
                    #parent_table,
                    #parent_column,
                    #related_table,
                    #related_column,
                    #value_sql,
                    ::cratestack::SortDirection::Asc,
                )
            }
        });
        methods.push(quote! {
            pub fn desc(self) -> ::cratestack::OrderClause {
                ::cratestack::OrderClause::relation_scalar(
                    #parent_table,
                    #parent_column,
                    #related_table,
                    #related_column,
                    #value_sql,
                    ::cratestack::SortDirection::Desc,
                )
            }
        });
    }

    Ok(quote! {
        pub mod #module_ident {
            pub use super::*;

            pub struct Field;

            impl Field {
                #(#methods)*
            }
        }
    })
}

fn generate_relation_filter_functions(
    model: &Model,
    wrappers: &[RelationPathSegment],
    models: &[Model],
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    let model_names = model_name_set(models);
    scalar_model_fields(model, &model_names)
        .into_iter()
        .map(|field| generate_scalar_relation_filter_functions(field, wrappers))
        .collect::<Result<Vec<_>, String>>()
        .map(|groups| groups.into_iter().flatten().collect())
}

fn generate_scalar_relation_filter_functions(
    field: &Field,
    wrappers: &[RelationPathSegment],
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    let field_type = rust_type_tokens(&field.ty);
    let column = to_snake_case(&field.name);
    let mut fns = Vec::new();

    filter_builders::append_required_filter_functions(
        &mut fns,
        field,
        wrappers,
        &field_type,
        &column,
    );
    filter_builders::append_boolean_filter_functions(
        &mut fns,
        field,
        wrappers,
        &field_type,
        &column,
    );
    filter_builders::append_required_text_filter_functions(
        &mut fns,
        field,
        wrappers,
        &field_type,
        &column,
    );
    filter_builders::append_optional_filter_functions(
        &mut fns,
        field,
        wrappers,
        &field_type,
        &column,
    );
    filter_builders::append_optional_string_filter_functions(
        &mut fns,
        field,
        wrappers,
        &field_type,
        &column,
    );

    Ok(fns)
}

fn wrap_filter_expr_tokens(
    base_expr: proc_macro2::TokenStream,
    wrappers: &[RelationPathSegment],
) -> proc_macro2::TokenStream {
    wrappers.iter().rev().fold(base_expr, |inner, wrapper| {
        let parent_table = wrapper.link.parent_table.as_str();
        let parent_column = wrapper.link.parent_column.as_str();
        let related_table = wrapper.link.related_table.as_str();
        let related_column = wrapper.link.related_column.as_str();
        match wrapper.kind {
            RelationFilterWrapperKind::ToOne => quote! {
                ::cratestack::FilterExpr::relation(
                    #parent_table,
                    #parent_column,
                    #related_table,
                    #related_column,
                    #inner,
                )
            },
            RelationFilterWrapperKind::Some => quote! {
                ::cratestack::FilterExpr::relation_some(
                    #parent_table,
                    #parent_column,
                    #related_table,
                    #related_column,
                    #inner,
                )
            },
            RelationFilterWrapperKind::Every => quote! {
                ::cratestack::FilterExpr::relation_every(
                    #parent_table,
                    #parent_column,
                    #related_table,
                    #related_column,
                    #inner,
                )
            },
            RelationFilterWrapperKind::None => quote! {
                ::cratestack::FilterExpr::relation_none(
                    #parent_table,
                    #parent_column,
                    #related_table,
                    #related_column,
                    #inner,
                )
            },
        }
    })
}

fn collect_relation_order_targets(
    model: &Model,
    models: &[Model],
    current_table: &str,
    prefix: &str,
) -> Result<Vec<(String, String)>, String> {
    fn collect_relation_order_targets_inner(
        model: &Model,
        models: &[Model],
        current_table: &str,
        prefix: &str,
        visited: &[String],
    ) -> Result<Vec<(String, String)>, String> {
        let model_names = model_name_set(models);
        let mut targets = scalar_model_fields(model, &model_names)
            .into_iter()
            .map(|field| {
                (
                    format!("{}.{}", prefix, field.name),
                    format!("{}.{}", current_table, to_snake_case(&field.name)),
                )
            })
            .collect::<Vec<_>>();

        for relation_field in relation_model_fields(model, &model_names) {
            let visit_key = relation_visit_key(model, relation_field);
            if visited.contains(&visit_key) {
                continue;
            }
            let relation_link = relation_link(model, relation_field, models)?;
            if relation_link.is_to_many {
                continue;
            }
            let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
                format!(
                    "relation field `{}` on `{}` references unknown model `{}`",
                    relation_field.name, model.name, relation_field.ty.name,
                )
            })?;
            let mut next_visited = visited.to_vec();
            next_visited.push(visit_key);
            let nested_targets = collect_relation_order_targets_inner(
                target_model,
                models,
                relation_link.related_table.as_str(),
                &format!("{}.{}", prefix, relation_field.name),
                &next_visited,
            )?;
            targets.extend(nested_targets.into_iter().map(|(key, nested_sql)| {
                (
                    key,
                    format!(
                        "(SELECT {} FROM {} WHERE {}.{} = {}.{} LIMIT 1)",
                        nested_sql,
                        relation_link.related_table,
                        relation_link.related_table,
                        relation_link.related_column,
                        current_table,
                        relation_link.parent_column,
                    ),
                )
            }));
        }

        Ok(targets)
    }

    collect_relation_order_targets_inner(model, models, current_table, prefix, &[])
}

fn relation_order_value_sql_for_path(
    model: &Model,
    models: &[Model],
    current_table: &str,
    path: &[String],
) -> Result<String, String> {
    let Some((segment, rest)) = path.split_first() else {
        return Err(format!(
            "empty relation order path on model `{}`",
            model.name
        ));
    };
    let field = model
        .fields
        .iter()
        .find(|field| field.name == *segment)
        .ok_or_else(|| format!("unknown field `{segment}` on model `{}`", model.name))?;
    let model_names = model_name_set(models);

    if !is_relation_field(&model_names, field) {
        if !rest.is_empty() {
            return Err(format!(
                "scalar field `{}` on model `{}` cannot continue relation order path",
                field.name, model.name,
            ));
        }
        return Ok(format!("{}.{}", current_table, to_snake_case(&field.name)));
    }

    let relation_link = relation_link(model, field, models)?;
    if relation_link.is_to_many {
        return Err(format!(
            "relation field `{}` on `{}` cannot be used in orderBy because it is to-many",
            field.name, model.name,
        ));
    }
    if rest.is_empty() {
        return Err(format!(
            "relation field `{}` on `{}` must target a scalar field for orderBy",
            field.name, model.name,
        ));
    }

    let target_model = find_model(models, &field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            field.name, model.name, field.ty.name,
        )
    })?;
    let nested_sql = relation_order_value_sql_for_path(
        target_model,
        models,
        relation_link.related_table.as_str(),
        rest,
    )?;

    Ok(format!(
        "(SELECT {} FROM {} WHERE {}.{} = {}.{} LIMIT 1)",
        nested_sql,
        relation_link.related_table,
        relation_link.related_table,
        relation_link.related_column,
        current_table,
        relation_link.parent_column,
    ))
}

#[cfg(test)]
mod tests {
    use cratestack_core::{Attribute, Field, SourceSpan, TypeRef};

    use super::{
        RelationFilterWrapperKind, RelationLink, RelationPathSegment, parse_relation_attribute,
        split_top_level, wrappers_allow_ordering,
    };

    fn span() -> SourceSpan {
        SourceSpan {
            start: 0,
            end: 0,
            line: 1,
        }
    }

    fn field_with_relation(raw: &str) -> Field {
        Field {
            docs: Vec::new(),
            name: "author".to_owned(),
            name_span: span(),
            ty: TypeRef {
                name: "User".to_owned(),
                name_span: span(),
                arity: cratestack_core::TypeArity::Required,
                generic_args: Vec::new(),
            },
            attributes: vec![Attribute {
                raw: raw.to_owned(),
                span: span(),
            }],
            span: span(),
        }
    }

    fn segment(kind: RelationFilterWrapperKind) -> RelationPathSegment {
        RelationPathSegment {
            link: RelationLink {
                parent_table: "posts".to_owned(),
                parent_column: "author_id".to_owned(),
                related_table: "users".to_owned(),
                related_column: "id".to_owned(),
                is_to_many: false,
            },
            kind,
        }
    }

    #[test]
    fn split_top_level_ignores_nested_brackets() {
        let items = split_top_level("fields:[userId], references:[id], map:[a,b(c,d)]", ',');
        assert_eq!(
            items,
            vec!["fields:[userId]", "references:[id]", "map:[a,b(c,d)]"]
        );
    }

    #[test]
    fn parse_relation_attribute_extracts_fields_and_references() {
        let field = field_with_relation("@relation(fields:[userId], references:[id])");
        let parsed = parse_relation_attribute(&field).expect("relation attribute should parse");
        assert_eq!(parsed.fields, vec!["userId".to_owned()]);
        assert_eq!(parsed.references, vec!["id".to_owned()]);
    }

    #[test]
    fn parse_relation_attribute_rejects_unknown_keys() {
        let field = field_with_relation("@relation(fields:[userId], ref:[id])");
        assert!(parse_relation_attribute(&field).is_none());
    }

    #[test]
    fn wrappers_allow_ordering_only_for_to_one_paths() {
        assert!(wrappers_allow_ordering(&[segment(
            RelationFilterWrapperKind::ToOne
        )]));
        assert!(!wrappers_allow_ordering(&[segment(
            RelationFilterWrapperKind::Some
        )]));
    }
}
