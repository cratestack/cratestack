//! The type-level state slots that make `build()` conditional.
//!
//! One slot per *required* field, in declaration order. This type owns
//! every rendering of that list — the declaration with `Unset` defaults,
//! the bare `impl` parameters, a single slot pinned to `Set`, the all-`Set`
//! terminal state, and the `PhantomData` payload — so those orderings
//! cannot drift apart. Split from `builder.rs` per the repo's 200-LoC
//! file convention.

use quote::{format_ident, quote};

use super::BuilderField;

/// The type-level state slots — one per required field, in declaration
/// order. Owns every rendering of that list (declaration with `Unset`
/// defaults, bare `impl` parameters, the all-`Set` terminal state, and the
/// `PhantomData` payload) so the orderings can't drift apart.
pub(crate) struct StateParams {
    params: Vec<syn::Ident>,
    /// For each field, its slot index — `None` for optional fields.
    slots: Vec<Option<usize>>,
}

impl StateParams {
    pub(super) fn new(fields: &[BuilderField]) -> StateParams {
        let mut params = Vec::new();
        let slots = fields
            .iter()
            .map(|field| {
                if !field.required {
                    return None;
                }
                let index = params.len();
                params.push(format_ident!("S{}", index));
                Some(index)
            })
            .collect();
        StateParams { params, slots }
    }

    pub(crate) fn slot(&self, field_index: usize) -> Option<usize> {
        self.slots[field_index]
    }

    pub(super) fn declaration(&self) -> proc_macro2::TokenStream {
        if self.params.is_empty() {
            return quote! {};
        }
        let params = self
            .params
            .iter()
            .map(|param| quote! { #param = ::cratestack::builder::Unset });
        quote! { <#(#params),*> }
    }

    pub(super) fn impl_params(&self) -> proc_macro2::TokenStream {
        self.wrap(self.params.iter().map(|param| quote! { #param }))
    }

    /// `{Builder}<S0, S1, ..>` — the receiver type of the setter impl.
    pub(super) fn applied(&self, builder_ident: &syn::Ident) -> proc_macro2::TokenStream {
        let generics = self.impl_params();
        quote! { #builder_ident #generics }
    }

    /// The same list with slot `set_index` pinned to `Set` — a required
    /// setter's return type.
    pub(crate) fn with_slot_set(
        &self,
        builder_ident: &syn::Ident,
        set_index: usize,
    ) -> proc_macro2::TokenStream {
        let generics = self.wrap(self.params.iter().enumerate().map(|(index, param)| {
            if index == set_index {
                quote! { ::cratestack::builder::Set }
            } else {
                quote! { #param }
            }
        }));
        quote! { #builder_ident #generics }
    }

    /// `{Builder}<Set, Set, ..>` — the one state `build()` is defined on.
    pub(crate) fn all_set(&self, builder_ident: &syn::Ident) -> proc_macro2::TokenStream {
        let generics = self.wrap(
            self.params
                .iter()
                .map(|_| quote! { ::cratestack::builder::Set }),
        );
        quote! { #builder_ident #generics }
    }

    pub(super) fn phantom_ty(&self) -> proc_macro2::TokenStream {
        let params = self.params.iter();
        quote! { (#(#params,)*) }
    }

    fn wrap(
        &self,
        items: impl Iterator<Item = proc_macro2::TokenStream>,
    ) -> proc_macro2::TokenStream {
        if self.params.is_empty() {
            return quote! {};
        }
        let items = items.collect::<Vec<_>>();
        quote! { <#(#items),*> }
    }
}
