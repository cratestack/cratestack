//! Flat, per-model relation-path emission.
//!
//! Replaces the old recursive emitter, which materialised one `Path` type
//! per *distinct path* through the relation graph — exponential in graph
//! connectivity (cratestack#252). Here every model emits exactly one
//! arrival type (`RelPath`), one pending-quantifier type (`RelToMany`) and
//! one `Field` per scalar, so total emitted code is linear in
//! `models × fields` regardless of how densely the graph is connected.
//!
//! The traversed path lives in a runtime `Vec<RelationHop>` carried by the
//! builder and folded into a `FilterExpr`/`OrderClause` at the leaf — see
//! `cratestack_sql::relation_path`.

use cratestack_core::{Field, Model};
use quote::quote;

use crate::shared::{
    ident, model_name_set, relation_model_fields, rust_type_tokens, scalar_model_fields,
    to_snake_case,
};

use super::filter_builders;
use super::types::{RelationLink, relation_link};

/// Tokens for a `RelationHop` const expression describing one edge.
fn hop_tokens(
    link: &RelationLink,
    quantifier: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let parent_table = link.parent_table.as_str();
    let parent_column = link.parent_column.as_str();
    let related_table = link.related_table.as_str();
    let related_column = link.related_column.as_str();
    quote! {
        ::cratestack::RelationHop::new(
            #parent_table,
            #parent_column,
            #related_table,
            #related_column,
            #quantifier,
        )
    }
}

/// The shared `RelPath` / `RelToMany` types plus per-scalar `Field`
/// modules for `model`. Emitted once inside the model's own field module.
pub(crate) fn generate_model_path_types(
    model: &Model,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let model_names = model_name_set(models);

    let scalar_accessors = scalar_model_fields(model, &model_names)
        .into_iter()
        .map(|field| {
            let method = ident(&field.name);
            let module = ident(&field.name);
            quote! {
                #[allow(non_snake_case)]
                pub fn #method(self) -> self::#module::Field<O> {
                    self::#module::Field::__from_hops(self.hops)
                }
            }
        })
        .collect::<Vec<_>>();

    let mut relation_accessors = Vec::new();
    for relation_field in relation_model_fields(model, &model_names) {
        let link = relation_link(model, relation_field, models)?;
        let method = ident(&relation_field.name);
        let target_module = ident(&to_snake_case(&relation_field.ty.name));
        if link.is_to_many {
            // Quantifier not chosen yet; record it as ToOne and let
            // `RelToMany::some/every/none` rewrite the last hop.
            let hop = hop_tokens(&link, quote! { ::cratestack::RelationQuantifier::ToOne });
            relation_accessors.push(quote! {
                #[allow(non_snake_case)]
                pub fn #method(self) -> super::#target_module::RelToMany {
                    let mut hops = self.hops;
                    hops.push(#hop);
                    super::#target_module::RelToMany::__from_hops(hops)
                }
            });
        } else {
            let hop = hop_tokens(&link, quote! { ::cratestack::RelationQuantifier::ToOne });
            relation_accessors.push(quote! {
                #[allow(non_snake_case)]
                pub fn #method(self) -> super::#target_module::RelPath<O> {
                    let mut hops = self.hops;
                    hops.push(#hop);
                    super::#target_module::RelPath::__from_hops(hops)
                }
            });
        }
    }

    let field_modules = scalar_model_fields(model, &model_names)
        .into_iter()
        .map(generate_scalar_field_module)
        .collect::<Vec<_>>();

    Ok(quote! {
        /// A relation path that has arrived at this model. One type per
        /// model — the traversed path is runtime data, not type structure.
        pub struct RelPath<O = ::cratestack::Orderable> {
            hops: ::std::vec::Vec<::cratestack::RelationHop>,
            __marker: ::core::marker::PhantomData<O>,
        }

        impl<O> RelPath<O> {
            #[doc(hidden)]
            pub fn __from_hops(hops: ::std::vec::Vec<::cratestack::RelationHop>) -> Self {
                Self { hops, __marker: ::core::marker::PhantomData }
            }

            #(#scalar_accessors)*
            #(#relation_accessors)*
        }

        /// A to-many hop whose quantifier has not been chosen yet.
        pub struct RelToMany {
            hops: ::std::vec::Vec<::cratestack::RelationHop>,
        }

        impl RelToMany {
            #[doc(hidden)]
            pub fn __from_hops(hops: ::std::vec::Vec<::cratestack::RelationHop>) -> Self {
                Self { hops }
            }

            fn __quantified(
                self,
                quantifier: ::cratestack::RelationQuantifier,
            ) -> RelPath<::cratestack::Unorderable> {
                let mut hops = self.hops;
                if let Some(last) = hops.last_mut() {
                    *last = last.with_quantifier(quantifier);
                }
                RelPath::__from_hops(hops)
            }

            pub fn some(self) -> RelPath<::cratestack::Unorderable> {
                self.__quantified(::cratestack::RelationQuantifier::Some)
            }

            pub fn every(self) -> RelPath<::cratestack::Unorderable> {
                self.__quantified(::cratestack::RelationQuantifier::Every)
            }

            pub fn none(self) -> RelPath<::cratestack::Unorderable> {
                self.__quantified(::cratestack::RelationQuantifier::None)
            }
        }

        #(#field_modules)*
    })
}

/// `pub mod <field> { pub struct Field<O> { .. } }` — filter methods for
/// every marker, ordering methods only for `Orderable`.
fn generate_scalar_field_module(field: &Field) -> proc_macro2::TokenStream {
    let module_ident = ident(&field.name);
    let field_type = rust_type_tokens(&field.ty);
    let column = to_snake_case(&field.name);
    let mut methods = Vec::new();

    filter_builders::append_required_builder_methods(&mut methods, field, &field_type, &column);
    filter_builders::append_boolean_builder_methods(&mut methods, field, &field_type, &column);
    filter_builders::append_required_text_builder_methods(
        &mut methods,
        field,
        &field_type,
        &column,
    );
    filter_builders::append_optional_builder_methods(&mut methods, field, &field_type, &column);
    filter_builders::append_optional_string_builder_methods(
        &mut methods,
        field,
        &field_type,
        &column,
    );

    quote! {
        pub mod #module_ident {
            pub use super::*;

            pub struct Field<O = ::cratestack::Orderable> {
                pub(super) hops: ::std::vec::Vec<::cratestack::RelationHop>,
                pub(super) __marker: ::core::marker::PhantomData<O>,
            }

            impl<O> Field<O> {
                #[doc(hidden)]
                pub fn __from_hops(hops: ::std::vec::Vec<::cratestack::RelationHop>) -> Self {
                    Self { hops, __marker: ::core::marker::PhantomData }
                }

                #(#methods)*
            }

            impl Field<::cratestack::Orderable> {
                pub fn asc(self) -> ::cratestack::OrderClause {
                    __order_clause(&self.hops, #column, ::cratestack::SortDirection::Asc)
                }

                pub fn desc(self) -> ::cratestack::OrderClause {
                    __order_clause(&self.hops, #column, ::cratestack::SortDirection::Desc)
                }
            }

            fn __order_clause(
                hops: &[::cratestack::RelationHop],
                column: &'static str,
                direction: ::cratestack::SortDirection,
            ) -> ::cratestack::OrderClause {
                let root = hops[0];
                ::cratestack::OrderClause::relation_scalar(
                    root.parent_table,
                    root.parent_column,
                    root.related_table,
                    root.related_column,
                    ::cratestack::order_value_sql(hops, column),
                    direction,
                )
            }
        }
    }
}
