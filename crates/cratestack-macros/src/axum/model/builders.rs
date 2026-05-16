//! Filter-expression / order-by / selection-validation helper-fn
//! tokens spliced alongside the per-model axum handlers. Each
//! `quote!{}` block emits one `fn` consumed by the handlers in
//! [`super::handlers_list`] / [`super::handlers_crud`].

use quote::quote;

use super::prep::ModelHandlerPrep;

pub(super) struct RelationArmCollections {
    pub(super) query_filter_arms: Vec<proc_macro2::TokenStream>,
    pub(super) relation_filter_guards: Vec<proc_macro2::TokenStream>,
    pub(super) order_by_arms: Vec<proc_macro2::TokenStream>,
    pub(super) relation_order_by_arms: Vec<proc_macro2::TokenStream>,
    pub(super) relation_include_arms: Vec<proc_macro2::TokenStream>,
    pub(super) relation_include_path_validation_arms: Vec<proc_macro2::TokenStream>,
    pub(super) relation_include_fields_validation_arms: Vec<proc_macro2::TokenStream>,
}

pub(super) fn build_query_helpers(
    p: &ModelHandlerPrep,
    arms: &RelationArmCollections,
) -> proc_macro2::TokenStream {
    let filter_expr_builder_ident = &p.filter_expr_builder_ident;
    let query_expr_builder_ident = &p.query_expr_builder_ident;
    let model_name = &p.model_name;
    let query_filter_arms = &arms.query_filter_arms;
    let relation_filter_guards = &arms.relation_filter_guards;

    quote! {
        fn #filter_expr_builder_ident(
            key: &str,
            value: &str,
        ) -> Result<::cratestack::FilterExpr, CoolError> {
            #(#relation_filter_guards)*
            let (field_name, operator) = key
                .split_once("__")
                .map(|(field_name, operator)| (field_name, operator))
                .unwrap_or((key, "eq"));

            match (field_name, operator) {
                #(#query_filter_arms)*
                _ => Err(CoolError::BadRequest(format!(
                    "unsupported query filter '{}' for {}",
                    key,
                    #model_name,
                ))),
            }
        }

        fn #query_expr_builder_ident(
            expr: &::cratestack::QueryExpr,
        ) -> Result<::cratestack::FilterExpr, CoolError> {
            match expr {
                ::cratestack::QueryExpr::Predicate { key, value } => #filter_expr_builder_ident(key, value),
                ::cratestack::QueryExpr::All(filters) => Ok(::cratestack::FilterExpr::all(
                    filters
                        .iter()
                        .map(#query_expr_builder_ident)
                        .collect::<Result<Vec<_>, CoolError>>()?,
                )),
                ::cratestack::QueryExpr::Any(filters) => Ok(::cratestack::FilterExpr::any(
                    filters
                        .iter()
                        .map(#query_expr_builder_ident)
                        .collect::<Result<Vec<_>, CoolError>>()?,
                )),
                ::cratestack::QueryExpr::Not(filter) => {
                    Ok(#query_expr_builder_ident(filter)?.not())
                }
            }
        }
    }
}

pub(super) fn build_validate_helpers(
    p: &ModelHandlerPrep,
    arms: &RelationArmCollections,
) -> proc_macro2::TokenStream {
    let validate_selection_ident = &p.validate_selection_ident;
    let validate_include_path_ident = &p.validate_include_path_ident;
    let validate_include_fields_path_ident = &p.validate_include_fields_path_ident;
    let model_ident = &p.model_ident;
    let primary_key_type = &p.primary_key_type;
    let model_name = &p.model_name;
    let relation_include_path_validation_arms = &arms.relation_include_path_validation_arms;
    let relation_include_fields_validation_arms = &arms.relation_include_fields_validation_arms;

    quote! {
        fn #validate_selection_ident(
            selection: &ModelSelectionQuery,
            descriptor: &::cratestack::ModelDescriptor<super::models::#model_ident, #primary_key_type>,
        ) -> Result<(), CoolError> {
            if let Some(fields) = &selection.fields {
                for field in fields {
                    if !descriptor.allowed_fields.contains(&field.as_str()) {
                        return Err(CoolError::Validation(format!(
                            "unsupported fields selection '{}' for {}",
                            field,
                            #model_name,
                        )));
                    }
                }
            }

            for include in &selection.includes {
                #validate_include_path_ident(include, descriptor)?;
            }

            for (include, fields) in &selection.include_fields {
                if !selection.includes.iter().any(|selected| selected == include) {
                    return Err(CoolError::Validation(format!(
                        "includeFields[{}] requires include={} for {}",
                        include,
                        include,
                        #model_name,
                    )));
                }

                #validate_include_fields_path_ident(include, fields, descriptor)?;
            }

            Ok(())
        }

        fn #validate_include_path_ident(
            include: &str,
            descriptor: &::cratestack::ModelDescriptor<super::models::#model_ident, #primary_key_type>,
        ) -> Result<(), CoolError> {
            let (direct, rest) = include
                .split_once('.')
                .map(|(direct, rest)| (direct, Some(rest)))
                .unwrap_or((include, None));
            if !descriptor.allowed_includes.contains(&direct) {
                return Err(CoolError::Validation(format!(
                    "unsupported include selection '{}' for {}",
                    include,
                    #model_name,
                )));
            }

            match (direct, rest) {
                #(#relation_include_path_validation_arms)*
                _ => Ok(()),
            }
        }

        fn #validate_include_fields_path_ident(
            include: &str,
            fields: &[String],
            descriptor: &::cratestack::ModelDescriptor<super::models::#model_ident, #primary_key_type>,
        ) -> Result<(), CoolError> {
            let (direct, rest) = include
                .split_once('.')
                .map(|(direct, rest)| (direct, Some(rest)))
                .unwrap_or((include, None));
            if !descriptor.allowed_includes.contains(&direct) {
                return Err(CoolError::Validation(format!(
                    "unsupported includeFields selection '{}' for {}",
                    include,
                    #model_name,
                )));
            }

            match (direct, rest) {
                #(#relation_include_fields_validation_arms)*
                _ => Ok(()),
            }
        }
    }
}
