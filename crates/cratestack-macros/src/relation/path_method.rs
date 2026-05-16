//! `Path::<field>` method emitters — scalar variants return a
//! `Field` zero-sized type; nested-relation variants return a `Path`.

use cratestack_core::Field;
use quote::quote;

use crate::shared::ident;

pub(super) fn generate_scalar_relation_path_method(field: &Field) -> proc_macro2::TokenStream {
    let method_ident = ident(&field.name);
    let module_ident = ident(&field.name);

    quote! {
        #[allow(non_snake_case)]
        pub fn #method_ident(self) -> self::#module_ident::Field {
            self::#module_ident::Field
        }
    }
}

pub(super) fn generate_nested_relation_path_method(field: &Field) -> proc_macro2::TokenStream {
    let method_ident = ident(&field.name);
    let module_ident = ident(&field.name);

    quote! {
        #[allow(non_snake_case)]
        pub fn #method_ident(self) -> self::#module_ident::Path {
            self::#module_ident::Path
        }
    }
}
