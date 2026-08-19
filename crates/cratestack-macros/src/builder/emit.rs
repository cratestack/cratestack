//! Per-field setter emission and the terminal `build()` impl. Split from
//! `builder.rs` per the repo's 200-LoC file convention; the *why* of the
//! encoding lives in that module's doc.

use quote::{format_ident, quote};

use super::{BuilderField, StateParams};

/// Emit one setter. A required field's setter flips its own state slot to
/// `Set` and therefore returns a *different* builder type; an optional
/// field's setter returns `Self`.
pub(super) fn setter(
    index: usize,
    field: &BuilderField,
    builder_ident: &syn::Ident,
    state: &StateParams,
) -> proc_macro2::TokenStream {
    let setter_ident = setter_ident(&field.ident);
    let tuple_slot = tuple_index(index);
    let docs = &field.docs;
    let setter_ty = &field.setter_ty;
    let (arg_ty, value_expr) = if field.into {
        (
            quote! { impl ::core::convert::Into<#setter_ty> },
            quote! { ::core::convert::Into::into(value) },
        )
    } else {
        (quote! { #setter_ty }, quote! { value })
    };
    let stored = field.store_expr(value_expr);

    match state.slot(index) {
        Some(state_slot) => {
            let return_ty = state.with_slot_set(builder_ident, state_slot);
            quote! {
                #docs
                pub fn #setter_ident(mut self, value: #arg_ty) -> #return_ty {
                    self.fields.#tuple_slot = #stored;
                    #builder_ident {
                        fields: self.fields,
                        __state: ::core::marker::PhantomData,
                    }
                }
            }
        }
        None => quote! {
            #docs
            pub fn #setter_ident(mut self, value: #arg_ty) -> Self {
                self.fields.#tuple_slot = #stored;
                self
            }
        },
    }
}

/// `build()`, defined *only* on the all-`Set` state. Required fields are
/// unwrapped with `expect` on a message that can't fire: reaching this
/// impl at all is proof every slot was filled.
pub(super) fn build_impl(
    target: &syn::Ident,
    builder_ident: &syn::Ident,
    state: &StateParams,
    fields: &[BuilderField],
) -> proc_macro2::TokenStream {
    let self_ty = state.all_set(builder_ident);
    let assignments = fields.iter().enumerate().map(|(index, field)| {
        let field_ident = &field.ident;
        let tuple_slot = tuple_index(index);
        if field.required {
            let unreachable = format!(
                "`{target}::{field_ident}` unset in the fully-set builder state — \
                 cratestack typestate bug, please report it"
            );
            quote! { #field_ident: self.fields.#tuple_slot.expect(#unreachable), }
        } else {
            quote! { #field_ident: self.fields.#tuple_slot.unwrap_or_default(), }
        }
    });

    let docs = format!(
        "Finish building the [`{target}`]. Infallible: this method only exists once every \
         required field has been set, so there is no missing-field case to report."
    );

    quote! {
        impl #self_ty {
            #[doc = #docs]
            pub fn build(self) -> #target {
                #target {
                    #(#assignments)*
                }
            }
        }
    }
}

/// A schema field literally named `build` would collide with the terminal
/// method (both land as inherent methods on the same builder type, and the
/// all-`Set` state matches both impls). Prefix that one case rather than
/// emitting code that doesn't compile.
fn setter_ident(field_ident: &syn::Ident) -> syn::Ident {
    if field_ident == "build" {
        format_ident!("set_build")
    } else {
        field_ident.clone()
    }
}

/// A field's position in the builder's anonymous `fields` tuple. `syn::Index`
/// (not a bare integer literal) so it renders as `self.fields.3`, not
/// `self.fields.3usize` — tuple indices are their own token kind.
fn tuple_index(index: usize) -> syn::Index {
    syn::Index::from(index)
}
