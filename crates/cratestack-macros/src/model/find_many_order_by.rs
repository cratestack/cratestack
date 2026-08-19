//! Per-model `<Model>SortField` enum + `<Model>OrderByClause` struct —
//! a typed `{ field, direction }` pair, list-valued on the `FindMany`
//! input so multi-key sort order survives the wire (a JSON *object*
//! keyed by field name would silently lose ordering across languages —
//! `serde_json::Map` alphabetizes keys unless the `preserve_order`
//! feature is on). Split out from `find_many_where.rs` per the repo's
//! 200-LoC file convention.
//!
//! Scoped to the model's own scalar fields only — no relation-path
//! sorting (`sort=author.name` in the untyped REST route) in this pass;
//! see this module's own doc in the PR description for the rationale.

use std::collections::BTreeSet;

use cratestack_core::Model;
use quote::quote;

use crate::builder::{BuilderField, generate_builder};
use crate::shared::{generated_doc_attr, ident, scalar_model_fields, to_snake_case};

pub(crate) fn generate_order_by_types(
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let sort_field_ident = ident(&format!("{}SortField", model.name));
    let order_by_ident = ident(&format!("{}OrderByClause", model.name));
    let module_ident = ident(&to_snake_case(&model.name));
    let fields = scalar_model_fields(model, model_names);

    let variant_idents = fields
        .iter()
        .map(|field| ident(&to_pascal_case(&field.name)))
        .collect::<Vec<_>>();
    let field_fns = fields.iter().map(|field| ident(&field.name));

    let match_arms = variant_idents.iter().zip(field_fns).map(|(variant, field_fn)| {
        quote! {
            (#sort_field_ident::#variant, ::cratestack::SortDirection::Asc) => super::#module_ident::#field_fn().asc(),
            (#sort_field_ident::#variant, ::cratestack::SortDirection::Desc) => super::#module_ident::#field_fn().desc(),
        }
    });

    let sort_field_docs = generated_doc_attr(format!(
        "Every field `{}` can be sorted by, for `{}OrderByClause`.",
        model.name, model.name
    ));
    let order_by_docs = generated_doc_attr(format!(
        "One `{{ field, direction }}` sort key for `FindMany<{}>` — a `Vec` of these on the \
         `FindMany` input preserves multi-key sort order (unlike a field-keyed JSON object).",
        model.name
    ));

    // Both fields are required — a sort key without a field or a
    // direction isn't a sort key. `#[derive(Copy)]` on the struct doesn't
    // affect the typestate builder: the builder holds `Option<T>`s in a
    // private holder, moved (not copied) between states like every other
    // generated struct's builder.
    let order_by_builder_fields = vec![
        BuilderField::new(ident("field"), quote! { #sort_field_ident }, true),
        BuilderField::new(
            ident("direction"),
            quote! { ::cratestack::SortDirection },
            true,
        ),
    ];
    let builder = generate_builder(&order_by_ident, &order_by_builder_fields);

    quote! {
        #sort_field_docs
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub enum #sort_field_ident {
            #(#variant_idents,)*
        }

        #order_by_docs
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct #order_by_ident {
            pub field: #sort_field_ident,
            pub direction: ::cratestack::SortDirection,
        }

        #builder

        impl #order_by_ident {
            pub fn to_order_clause(&self) -> ::cratestack::OrderClause {
                match (self.field, self.direction) {
                    #(#match_arms)*
                }
            }
        }
    }
}

fn to_pascal_case(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut capitalize_next = true;
    for ch in value.chars() {
        if ch == '_' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}
