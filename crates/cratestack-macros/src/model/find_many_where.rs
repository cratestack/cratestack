//! Per-model `<Model>Where` struct — one optional `FieldFilterInput<V>`
//! per filterable scalar field, plus a `to_filters()` method that turns
//! whichever operators the caller set into real `FilterExpr`s via the
//! model's own field accessors (`super::<model_snake>::<field>()`), the
//! same `FieldRef` calls the untyped REST `?where=` route already makes.
//! Split out from `inputs.rs` per the repo's 200-LoC file convention.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, TypeArity};
use quote::quote;

use crate::shared::{
    generated_doc_attr, ident, rust_type_tokens, scalar_model_fields, to_snake_case,
};

/// Types `query_scalar_parser_tokens` (the untyped REST `?where=` route's
/// own value parser) already proves can round-trip through a filter —
/// `Json`/`Bytes`/enum/custom-`type` fields are excluded here the same
/// way that function excludes them, rather than speculatively generating
/// `.eq()`/`.ne()` calls whose `IntoSqlValue` support isn't confirmed.
fn is_filterable_scalar(field: &Field) -> bool {
    matches!(
        field.ty.name.as_str(),
        "String" | "Cuid" | "Int" | "Float" | "Boolean" | "Uuid" | "DateTime" | "Decimal"
    )
}

/// `lt`/`lte`/`gt`/`gte` — every filterable scalar except `Boolean`.
/// Unlike the untyped REST route's own `supports_comparison`, not gated
/// to `Required` arity: `FieldRef<M, T>`'s comparison methods never
/// actually inspect `T` (see `cratestack-sql::filter::field_ref`), so
/// there's no technical reason to withhold them from optional fields —
/// this is a deliberate, real improvement over the untyped route, not an
/// inconsistency with it.
fn supports_ordering_ops(field: &Field) -> bool {
    is_filterable_scalar(field) && field.ty.name != "Boolean"
}

/// `contains`/`startsWith` — `String`/`Cuid` only (the only two types
/// `FieldRef::contains`/`starts_with` are actually implemented for; a
/// `Uuid` field's `FieldRef<M, uuid::Uuid>` has no such impl).
fn supports_string_ops(field: &Field) -> bool {
    matches!(field.ty.name.as_str(), "String" | "Cuid")
}

fn scalar_type_tokens(field: &Field) -> proc_macro2::TokenStream {
    let scalar_ty = cratestack_core::TypeRef {
        name: field.ty.name.clone(),
        name_span: field.ty.name_span,
        arity: TypeArity::Required,
        generic_args: Vec::new(),
    };
    rust_type_tokens(&scalar_ty)
}

pub(crate) fn generate_where_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let where_ident = ident(&format!("{}Where", model.name));
    let module_ident = ident(&to_snake_case(&model.name));
    let docs = generated_doc_attr(format!(
        "Generated `where` filter for `{}` — every operator set is combined with implicit AND. \
         Used by `FindMany<{}>` procedure arguments.",
        model.name, model.name
    ));
    let fields = scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| is_filterable_scalar(field))
        .collect::<Vec<_>>();

    let field_defs = fields.iter().map(|field| {
        let field_ident = ident(&field.name);
        let scalar_type = scalar_type_tokens(field);
        quote! {
            pub #field_ident: Option<::cratestack::FieldFilterInput<#scalar_type>>,
        }
    });

    let filter_pushes = fields
        .iter()
        .map(|field| build_field_push(field, &module_ident));

    quote! {
        #docs
        #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct #where_ident {
            #(#field_defs)*
        }

        impl #where_ident {
            pub fn to_filters(&self) -> Vec<::cratestack::FilterExpr> {
                let mut filters = Vec::new();
                #(#filter_pushes)*
                filters
            }
        }
    }
}

fn build_field_push(field: &Field, module_ident: &syn::Ident) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let field_fn = ident(&field.name);
    let mut ops = vec![quote! {
        if let Some(value) = &filter.eq {
            filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().eq(value.clone())));
        }
        if let Some(value) = &filter.ne {
            filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().ne(value.clone())));
        }
        if let Some(values) = &filter.in_ {
            filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().in_(values.clone())));
        }
    }];

    if supports_ordering_ops(field) {
        ops.push(quote! {
            if let Some(value) = &filter.lt {
                filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().lt(value.clone())));
            }
            if let Some(value) = &filter.lte {
                filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().lte(value.clone())));
            }
            if let Some(value) = &filter.gt {
                filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().gt(value.clone())));
            }
            if let Some(value) = &filter.gte {
                filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().gte(value.clone())));
            }
        });
    }

    if supports_string_ops(field) {
        ops.push(quote! {
            if let Some(value) = &filter.contains {
                filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().contains(value.clone())));
            }
            if let Some(value) = &filter.starts_with {
                filters.push(::cratestack::FilterExpr::from(super::#module_ident::#field_fn().starts_with(value.clone())));
            }
        });
    }

    if field.ty.arity == TypeArity::Optional {
        ops.push(quote! {
            if let Some(is_null) = filter.is_null {
                filters.push(if is_null {
                    ::cratestack::FilterExpr::from(super::#module_ident::#field_fn().is_null())
                } else {
                    ::cratestack::FilterExpr::from(super::#module_ident::#field_fn().is_not_null())
                });
            }
        });
    }

    quote! {
        if let Some(filter) = &self.#field_ident {
            #(#ops)*
        }
    }
}
