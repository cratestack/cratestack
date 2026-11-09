//! Keyset-cursor helpers for the generated `list` route
//! (`docs/design/cursor-pagination.md`): the per-model sort plan, the
//! typed comparison leaves the keyset predicate is built from, and the
//! page splitter that mints `nextCursor`. The schema-independent core
//! (codec, sort parsing, predicate expansion) lives in `cratestack-core` /
//! `cratestack-sql`; only what needs the model's field types is here.

use std::collections::BTreeSet;

use cratestack_core::{Model, TypeArity};
use quote::quote;

use crate::shared::{ident, query_scalar_parser_tokens, queryable_model_fields};

use super::prep::ModelHandlerPrep;

/// Scalar types with a total order a keyset bound can compare (mirrors
/// `supports_comparison`, which additionally demands a required arity —
/// the cursor machinery handles `NULL` itself, so optional is fine here).
fn cursor_eligible(field: &cratestack_core::Field) -> bool {
    matches!(field.ty.arity, TypeArity::Required | TypeArity::Optional)
        && matches!(
            field.ty.name.as_str(),
            "String" | "Cuid" | "Int" | "BigInt" | "Float" | "DateTime" | "Decimal" | "Uuid"
        )
}

pub(super) fn plan_ident(p: &ModelHandlerPrep) -> syn::Ident {
    ident(&format!("plan_{}_cursor", p.accessor_ident))
}

pub(super) fn filter_ident(p: &ModelHandlerPrep) -> syn::Ident {
    ident(&format!("build_{}_cursor_filter", p.accessor_ident))
}

pub(super) fn split_ident(p: &ModelHandlerPrep) -> syn::Ident {
    ident(&format!("split_{}_cursor_page", p.accessor_ident))
}

/// Request-level refusals shared by REST and RPC (one dispatch body):
/// `cursor` and `offset` are exclusive, and a bare-array (non-`@@paged`)
/// list has nowhere to carry `nextCursor`.
pub(super) fn cursor_request_guards(p: &ModelHandlerPrep) -> proc_macro2::TokenStream {
    let model_name = &p.model_name;
    let list_response_type = &p.list_response_type;
    let refuse = |message: proc_macro2::TokenStream| {
        quote! {
            ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = canonical_route, cratestack_model = #model_name, cratestack_operation = "list", "cratestack model list rejected an invalid cursor request");
            return ::cratestack::encode_transport_result_with_status_for::<_, #list_response_type>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(CratestackError::BadRequest(#message)));
        }
    };
    let both = refuse(quote! { "cursor and offset cannot both be provided".to_owned() });
    let unpaged = (!p.paged).then(|| {
        let body = refuse(quote! { format!("cursor pagination requires a @@paged model; {} is not paged", #model_name) });
        quote! { if query.cursor.is_some() { #body } }
    });
    quote! {
        if query.cursor.is_some() && query.offset.is_some() { #both }
        #unpaged
    }
}

pub(super) fn build_cursor_helpers(
    p: &ModelHandlerPrep,
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let (plan_ident, split_ident, filter_ident) = (plan_ident(p), split_ident(p), filter_ident(p));
    let leaf_ident = ident(&format!("build_{}_cursor_leaf", p.accessor_ident));
    let field_module_ident = &p.field_module_ident;
    let model_ident = &p.model_ident;
    let model_name = &p.model_name;
    let primary_key_name = model
        .fields
        .iter()
        .find(|field| crate::shared::is_primary_key(field))
        .map(|field| field.name.clone())
        .unwrap_or_default();

    let fields: Vec<_> = queryable_model_fields(model, model_names)
        .into_iter()
        .filter(|field| cursor_eligible(field))
        .collect();
    let nullable_arms = fields.iter().map(|field| {
        let name = &field.name;
        let nullable = field.ty.arity == TypeArity::Optional;
        quote! { #name => Some(#nullable), }
    });
    let value_arms = fields.iter().map(|field| {
        let name = &field.name;
        let field_ident = ident(&field.name);
        quote! { #name => ::cratestack::cursor_scalar(&last.#field_ident)?, }
    });
    let leaf_arms = fields.iter().filter_map(|field| {
        let name = &field.name;
        let field_fn = ident(&field.name);
        let parser = query_scalar_parser_tokens(&field.ty, quote! { value }, name, enum_names)?;
        let is_null = (field.ty.arity == TypeArity::Optional).then(|| {
            quote! {
                (#name, "isNull") => Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().is_null())),
            }
        });
        Some(quote! {
            (#name, "eq") => Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().eq((#parser)?))),
            (#name, "gt") => Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().gt((#parser)?))),
            (#name, "lt") => Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().lt((#parser)?))),
            #is_null
        })
    });

    quote! {
        /// Effective keyset order for `query.sort`: the requested columns
        /// plus the primary key as tiebreaker. Rejects (400) anything a
        /// keyset bound cannot compare.
        pub(super) fn #plan_ident(
            query: &ModelListQuery,
        ) -> Result<Vec<::cratestack::KeysetColumn>, CratestackError> {
            ::cratestack::parse_keyset_sort(query.sort.as_deref(), #primary_key_name, |name| match name {
                #(#nullable_arms)*
                _ => None,
            })
        }

        /// One typed comparison of the keyset bound. The value goes through
        /// the same per-type parser as a `?col__gt=` filter, so a forged
        /// cursor can say nothing a query string could not.
        fn #leaf_ident(
            column: &str,
            op: &str,
            value: &str,
        ) -> Result<::cratestack::FilterExpr, CratestackError> {
            match (column, op) {
                #(#leaf_arms)*
                _ => Err(CratestackError::BadRequest(format!(
                    "cursor pagination does not support sort field '{}' for {}",
                    column,
                    #model_name,
                ))),
            }
        }

        /// The keyset bound for `cursor` over `columns` (a non-empty
        /// cursor); `None` for the empty first-page token.
        pub(super) fn #filter_ident(
            columns: &[::cratestack::KeysetColumn],
            cursor: &str,
        ) -> Result<Option<::cratestack::FilterExpr>, CratestackError> {
            if cursor.is_empty() {
                return Ok(None);
            }
            let values = ::cratestack::decode_cursor(cursor, &::cratestack::keyset_order_spec(columns), columns.len())?;
            ::cratestack::keyset_after(columns, &values, |column, leaf| match leaf {
                ::cratestack::KeysetLeaf::Eq(value) => #leaf_ident(column, "eq", value),
                ::cratestack::KeysetLeaf::Gt(value) => #leaf_ident(column, "gt", value),
                ::cratestack::KeysetLeaf::Lt(value) => #leaf_ident(column, "lt", value),
                ::cratestack::KeysetLeaf::IsNull => #leaf_ident(column, "isNull", ""),
            })
            .map(Some)
        }

        /// Trim the `limit + 1` probe row off a cursor-mode result and mint
        /// the next cursor from the last row kept. `Ok(None)` outside
        /// cursor mode or on the last page.
        pub(super) fn #split_ident(
            query: &ModelListQuery,
            records: &mut Vec<super::models::#model_ident>,
        ) -> Result<Option<String>, CratestackError> {
            if query.cursor.is_none() {
                return Ok(None);
            }
            let limit = usize::try_from(query.limit.unwrap_or(::cratestack::MAX_LIST_LIMIT)).unwrap_or(0);
            if records.len() <= limit {
                return Ok(None);
            }
            records.truncate(limit);
            let Some(last) = records.last() else {
                return Ok(None);
            };
            let columns = #plan_ident(query)?;
            let mut values = Vec::with_capacity(columns.len());
            for column in &columns {
                values.push(match column.name.as_str() {
                    #(#value_arms)*
                    other => return Err(CratestackError::Internal(format!("cursor column '{other}' has no value accessor"))),
                });
            }
            Ok(Some(::cratestack::encode_cursor(&::cratestack::keyset_order_spec(&columns), &values)))
        }
    }
}
