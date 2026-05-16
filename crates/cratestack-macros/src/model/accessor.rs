//! `Cratestack` and `BoundCratestack` method accessors. Each model
//! contributes one method on each that hands out a `ModelDelegate` /
//! `ScopedModelDelegate` for that model.

use cratestack_core::Model;
use quote::quote;

use crate::shared::{ident, is_primary_key, rust_type_tokens, to_snake_case};

pub(crate) fn generate_model_accessor(model: &Model) -> proc_macro2::TokenStream {
    let method_ident = ident(&to_snake_case(&model.name));
    let model_ident = ident(&model.name);
    let descriptor_ident = ident(&format!(
        "{}_MODEL",
        to_snake_case(&model.name).to_uppercase()
    ));
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .expect("validated model must have primary key");
    let primary_key_type = rust_type_tokens(&primary_key.ty);

    quote! {
        pub fn #method_ident(&self) -> ::cratestack::ModelDelegate<'_, models::#model_ident, #primary_key_type> {
            ::cratestack::ModelDelegate::new(&self.runtime, &models::#descriptor_ident)
        }
    }
}

pub(crate) fn generate_bound_model_accessor(model: &Model) -> proc_macro2::TokenStream {
    let method_ident = ident(&to_snake_case(&model.name));
    let model_ident = ident(&model.name);
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .expect("validated model must have primary key");
    let primary_key_type = rust_type_tokens(&primary_key.ty);

    quote! {
        pub fn #method_ident(&self) -> ::cratestack::ScopedModelDelegate<'_, models::#model_ident, #primary_key_type> {
            self.inner.#method_ident().bind(self.ctx.clone())
        }
    }
}
