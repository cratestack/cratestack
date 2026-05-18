//! View accessor methods. Each view contributes one `fn <view_snake>()`
//! on the `Views<'_>` sub-accessor returned by `runtime.views()`.
//!
//! The accessor's return type is picked per view:
//!
//! - **Has `@id`** → `ViewDelegate<'_, V, PK>` (exposes `find_many`,
//!   `find_unique`, and `refresh()` on materialized views).
//! - **`@@no_unique`** → `ViewDelegateNoUnique<'_, V>` (exposes only
//!   `find_many`). `find_unique` is *absent* at the type level — a
//!   call like `runtime.views().<v>().find_unique(())` is a compile
//!   error, not a runtime "WHERE  = $1" footgun.

use cratestack_core::View;
use quote::quote;

use crate::shared::{ident, rust_type_tokens, to_snake_case};

pub(crate) fn generate_view_accessor(view: &View) -> proc_macro2::TokenStream {
    let method_ident = ident(&to_snake_case(&view.name));
    let view_ident = ident(&view.name);
    let descriptor_ident = ident(&format!(
        "{}_VIEW",
        to_snake_case(&view.name).to_uppercase()
    ));

    if view.no_unique() {
        quote! {
            pub fn #method_ident(&self) -> ::cratestack::ViewDelegateNoUnique<'_, super::models::#view_ident> {
                ::cratestack::ViewDelegateNoUnique::new(self.runtime, &super::models::#descriptor_ident)
            }
        }
    } else {
        let pk_field = view
            .fields
            .iter()
            .find(|field| field.attributes.iter().any(|attr| attr.raw == "@id"))
            .expect("validated view has @id when not @@no_unique");
        let primary_key_type = rust_type_tokens(&pk_field.ty);
        quote! {
            pub fn #method_ident(&self) -> ::cratestack::ViewDelegate<'_, super::models::#view_ident, #primary_key_type> {
                ::cratestack::ViewDelegate::new(self.runtime, &super::models::#descriptor_ident)
            }
        }
    }
}
