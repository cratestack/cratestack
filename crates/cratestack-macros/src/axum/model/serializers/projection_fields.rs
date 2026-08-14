//! `project_<model>_model_value` and its two (currently dead —
//! `includeFields[<relation>]` validates but never applies, a
//! pre-existing gap unrelated to cratestack#430) supporting helpers.
//!
//! Builds each field individually rather than routing the record
//! through `serde_json::to_value` (cratestack#430): `serde_json::Value`
//! always reports itself human-readable, which permanently collapses
//! any field whose `Serialize` impl branches on that hint (`Uuid`, …)
//! to its string form before the real wire codec ever runs.
//! `::cratestack::ProjectedValue::leaf` keeps each field's *original*
//! value instead, deferring that branch to the real target serializer
//! at encode time. See `cratestack-axum::projection` for the full
//! writeup.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model};
use quote::quote;

use crate::shared::{ident, is_server_only_field, scalar_model_fields};

use super::super::prep::ModelHandlerPrep;

/// One `object.insert(name, ProjectedValue::leaf(record.field.clone()))`
/// per non-`@server_only` scalar field — mirrors what `#[serde(skip_
/// serializing)]` did for those fields under the old `serde_json::
/// to_value` path (see `struct_field_definition` in `model/struct_only.rs`).
/// Every arity (required, optional, list) goes through the same
/// `leaf()` call: `Option<T>`'s own `Serialize` impl already encodes
/// `None` correctly under every codec (`serialize_none()`), so there is
/// no per-arity branching left to do here — unlike the old code, which
/// had to strip `null` map entries as a workaround for a `serde_json::
/// Value::Null`-specific quirk (see `ProjectedValue::Null`'s doc).
fn field_insert_tokens(
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> Vec<proc_macro2::TokenStream> {
    scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !is_server_only_field(field))
        .map(|field: &Field| {
            let field_ident = ident(&field.name);
            let field_name = &field.name;
            quote! {
                object.insert(
                    #field_name.to_owned(),
                    ::cratestack::ProjectedValue::leaf(record.#field_ident.clone()),
                );
            }
        })
        .collect()
}

pub(in super::super) fn build_projection_helpers(
    p: &ModelHandlerPrep,
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let project_object_fields_ident = &p.project_object_fields_ident;
    let project_serialized_value_ident = &p.project_serialized_value_ident;
    let project_model_value_ident = &p.project_model_value_ident;
    let model_ident = &p.model_ident;
    let field_inserts = field_insert_tokens(model, model_names);

    quote! {
        fn #project_object_fields_ident(
            object: ::std::collections::BTreeMap<String, ::cratestack::ProjectedValue>,
            fields: &[String],
            context: &str,
        ) -> Result<::std::collections::BTreeMap<String, ::cratestack::ProjectedValue>, CratestackError> {
            let mut object = object;
            let mut projected = ::std::collections::BTreeMap::new();
            for field in fields {
                let value = object.remove(field).ok_or_else(|| {
                    CratestackError::Internal(format!(
                        "serialized relation '{}' is missing field '{}'",
                        context,
                        field,
                    ))
                })?;
                projected.insert(field.clone(), value);
            }
            Ok(projected)
        }

        fn #project_serialized_value_ident(
            value: ::cratestack::ProjectedValue,
            fields: Option<&[String]>,
            context: &str,
        ) -> Result<::cratestack::ProjectedValue, CratestackError> {
            let Some(fields) = fields else {
                return Ok(value);
            };

            match value {
                ::cratestack::ProjectedValue::Null => Ok(::cratestack::ProjectedValue::Null),
                ::cratestack::ProjectedValue::Object(object) => Ok(::cratestack::ProjectedValue::Object(
                    #project_object_fields_ident(object, fields, context)?,
                )),
                ::cratestack::ProjectedValue::Array(values) => {
                    let mut projected = Vec::with_capacity(values.len());
                    for value in values {
                        projected.push(#project_serialized_value_ident(value, Some(fields), context)?);
                    }
                    Ok(::cratestack::ProjectedValue::Array(projected))
                }
                ::cratestack::ProjectedValue::Leaf(_) => Err(CratestackError::Internal(format!(
                    "included relation '{}' must serialize to an object, array, or null",
                    context,
                ))),
            }
        }

        fn #project_model_value_ident(
            record: &super::models::#model_ident,
            fields: Option<&[String]>,
        ) -> Result<::std::collections::BTreeMap<String, ::cratestack::ProjectedValue>, CratestackError> {
            let mut object = ::std::collections::BTreeMap::new();
            #(#field_inserts)*

            let Some(fields) = fields else {
                return Ok(object);
            };

            let mut projected = ::std::collections::BTreeMap::new();
            for field in fields {
                if let Some(value) = object.remove(field) {
                    projected.insert(field.clone(), value);
                }
                // Every recognised column was inserted above (including
                // `None`s, which `ProjectedValue::leaf` encodes as a
                // real wire null rather than being omitted) — a `field`
                // absent here was already rejected by selection
                // validation, so this silently skips instead of
                // erroring, matching the previous behaviour.
            }
            Ok(projected)
        }
    }
}
