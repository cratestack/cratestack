//! Projection/serialization helpers + the list-builder body.

use quote::quote;

use super::builders::RelationArmCollections;
use super::prep::ModelHandlerPrep;

pub(super) fn build_projection_helpers(p: &ModelHandlerPrep) -> proc_macro2::TokenStream {
    let project_object_fields_ident = &p.project_object_fields_ident;
    let project_serialized_value_ident = &p.project_serialized_value_ident;
    let project_model_value_ident = &p.project_model_value_ident;
    let model_ident = &p.model_ident;
    let model_name = &p.model_name;

    quote! {
        fn #project_object_fields_ident(
            object: ::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>,
            fields: &[String],
            context: &str,
        ) -> Result<::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>, CoolError> {
            let mut projected = ::cratestack::serde_json::Map::new();
            for field in fields {
                let value = object.get(field).cloned().ok_or_else(|| {
                    CoolError::Internal(format!(
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
            value: ::cratestack::serde_json::Value,
            fields: Option<&[String]>,
            context: &str,
        ) -> Result<::cratestack::serde_json::Value, CoolError> {
            let Some(fields) = fields else {
                return Ok(value);
            };

            match value {
                ::cratestack::serde_json::Value::Null => Ok(::cratestack::serde_json::Value::Null),
                ::cratestack::serde_json::Value::Object(object) => Ok(::cratestack::serde_json::Value::Object(
                    #project_object_fields_ident(object, fields, context)?,
                )),
                ::cratestack::serde_json::Value::Array(values) => {
                    let mut projected = Vec::with_capacity(values.len());
                    for value in values {
                        projected.push(#project_serialized_value_ident(value, Some(fields), context)?);
                    }
                    Ok(::cratestack::serde_json::Value::Array(projected))
                }
                _ => Err(CoolError::Internal(format!(
                    "included relation '{}' must serialize to an object, array, or null",
                    context,
                ))),
            }
        }

        fn #project_model_value_ident(
            record: &super::models::#model_ident,
            fields: Option<&[String]>,
        ) -> Result<::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>, CoolError> {
            let value = ::cratestack::serde_json::to_value(record)
                .map_err(|error| CoolError::Internal(format!("failed to serialize {}: {error}", #model_name)))?;
            let mut object = value.as_object().cloned().ok_or_else(|| {
                CoolError::Internal(format!("generated {} serialization must be a JSON object", #model_name))
            })?;
            // Strip `null` entries — minicbor-serde encodes null as
            // CBOR empty array, corrupting nullable columns. Client
            // `#[serde(default)]` recovers `None` from absent keys.
            object.retain(|_, v| !v.is_null());

            let Some(fields) = fields else {
                return Ok(object);
            };

            let mut projected = ::cratestack::serde_json::Map::new();
            for field in fields {
                if let Some(value) = object.get(field).cloned() {
                    projected.insert(field.clone(), value);
                }
                // Field absent from `object` means the row's column was
                // NULL and we stripped it above. Skip silently — the
                // client struct's `#[serde(default)]` restores `None`.
            }
            Ok(projected)
        }
    }
}

pub(super) fn build_serialize_helper(
    p: &ModelHandlerPrep,
    arms: &RelationArmCollections,
) -> proc_macro2::TokenStream {
    let serialize_model_value_ident = &p.serialize_model_value_ident;
    let project_model_value_ident = &p.project_model_value_ident;
    let model_ident = &p.model_ident;
    let relation_include_arms = &arms.relation_include_arms;

    quote! {
        fn #serialize_model_value_ident<'a>(
            db: &'a super::Cratestack,
            ctx: &'a ::cratestack::CoolContext,
            record: &'a super::models::#model_ident,
            selection: &'a ModelSelectionQuery,
        ) -> ::core::pin::Pin<
            Box<dyn ::core::future::Future<Output = Result<::cratestack::serde_json::Value, CoolError>> + Send + 'a>,
        > {
            Box::pin(async move {
                let mut object = #project_model_value_ident(record, selection.fields.as_deref())?;

                for include in selection.direct_includes() {
                    match include.as_str() {
                        #(#relation_include_arms)*
                        _ => unreachable!("validated include should be supported"),
                    }
                }

                Ok(::cratestack::serde_json::Value::Object(object))
            })
        }
    }
}

pub(super) fn build_list_builder(
    p: &ModelHandlerPrep,
    arms: &RelationArmCollections,
) -> proc_macro2::TokenStream {
    let list_builder_ident = &p.list_builder_ident;
    let model_ident = &p.model_ident;
    let model_name = &p.model_name;
    let accessor_ident = &p.accessor_ident;
    let primary_key_type = &p.primary_key_type;
    let query_expr_builder_ident = &p.query_expr_builder_ident;
    let order_by_arms = &arms.order_by_arms;
    let relation_order_by_arms = &arms.relation_order_by_arms;

    quote! {
        fn #list_builder_ident<'a>(
            db: &'a super::Cratestack,
            query: &ModelListQuery,
            apply_paging: bool,
        ) -> Result<::cratestack::FindMany<'a, super::models::#model_ident, #primary_key_type>, CoolError> {
            let descriptor = db.#accessor_ident().descriptor();
            let mut request = db.#accessor_ident().find_many();

            for filter in &query.filters {
                request = request.where_expr(#query_expr_builder_ident(filter)?);
            }

            if let Some(sort) = &query.sort {
                for raw_term in sort.split(',') {
                    let raw_term = raw_term.trim();
                    if raw_term.is_empty() {
                        return Err(CoolError::BadRequest(
                            "sort must not contain empty fields".to_owned(),
                        ));
                    }

                    let (descending, field_name) = match raw_term.strip_prefix('-') {
                        Some(field_name) => (true, field_name),
                        None => (false, raw_term),
                    };

                    if !descriptor.allowed_sorts.contains(&field_name) {
                        return Err(CoolError::Validation(format!(
                            "unsupported sort field '{}' for {}",
                            field_name,
                            #model_name,
                        )));
                    }

                    request = match field_name {
                        #(#order_by_arms)*
                        #(#relation_order_by_arms)*
                        _ => unreachable!("validated sort should be supported"),
                    };
                }
            }

            if apply_paging {
                if let Some(limit) = query.limit {
                    request = request.limit(limit);
                }
                if let Some(offset) = query.offset {
                    request = request.offset(offset);
                }
            }

            Ok(request)
        }
    }
}
