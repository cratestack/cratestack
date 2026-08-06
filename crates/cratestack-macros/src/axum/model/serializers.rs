//! Serialization helpers + the list-builder body.
//!
//! [`projection_fields`] carries `project_<model>_model_value` itself
//! (split out to keep this file under the repo's 200-LoC convention) —
//! see that module's doc for why it builds each field individually
//! rather than routing the record through `serde_json::to_value`
//! (cratestack#430).

mod projection_fields;

use quote::quote;

pub(super) use projection_fields::build_projection_helpers;

use super::builders::RelationArmCollections;
use super::prep::ModelHandlerPrep;

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
            Box<dyn ::core::future::Future<Output = Result<::cratestack::ProjectedValue, CoolError>> + Send + 'a>,
        > {
            Box::pin(async move {
                let mut object = #project_model_value_ident(record, selection.fields.as_deref())?;

                for include in selection.direct_includes() {
                    match include.as_str() {
                        #(#relation_include_arms)*
                        _ => unreachable!("validated include should be supported"),
                    }
                }

                Ok(::cratestack::ProjectedValue::Object(object))
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
    let order_catalog_ident = &p.order_catalog_ident;

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

                    request = if field_name.contains('.') {
                        let target = ::cratestack::resolve_order_target(&#order_catalog_ident, field_name)
                            .ok_or_else(|| CoolError::Validation(format!(
                                "unsupported sort field '{}' for {}",
                                field_name,
                                #model_name,
                            )))?;
                        let root = target.hops[0];
                        request.order_by(::cratestack::OrderClause::relation_scalar(
                            root.parent_table,
                            root.parent_column,
                            root.related_table,
                            root.related_column,
                            ::cratestack::order_value_sql(&target.hops, target.column),
                            if descending {
                                ::cratestack::SortDirection::Desc
                            } else {
                                ::cratestack::SortDirection::Asc
                            },
                        ))
                    } else {
                        if !descriptor.allowed_sorts.contains(&field_name) {
                            return Err(CoolError::Validation(format!(
                                "unsupported sort field '{}' for {}",
                                field_name,
                                #model_name,
                            )));
                        }

                        match field_name {
                            #(#order_by_arms)*
                            _ => unreachable!("validated sort should be supported"),
                        }
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
