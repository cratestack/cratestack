use std::collections::BTreeSet;

use cratestack_core::{EnumDecl, TypeDecl};
use quote::quote;

use crate::shared::{
    doc_attrs, field_definition, ident, is_custom_field, rust_type_tokens_with_scope, schema_lit,
    to_snake_case, value_tokens,
};

pub(crate) fn generate_type_struct(
    ty: &TypeDecl,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let type_ident = ident(&ty.name);
    let docs = doc_attrs(&ty.docs);
    let fields = ty
        .fields
        .iter()
        .map(|field| field_definition(field, false, false));
    let arg_matches = ty.fields.iter().map(|field| {
        let field_name = &field.name;
        let field_ident = ident(&field.name);
        let value = value_tokens(quote! { self.#field_ident.clone() }, &field.ty, enum_names);
        quote! {
            #field_name => Some(#value),
        }
    });

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #type_ident {
            #(#fields)*
        }

        impl ::cratestack::ProcedureArgs for #type_ident {
            fn procedure_arg_value(&self, field: &str) -> Option<::cratestack::Value> {
                match field {
                    #(#arg_matches)*
                    _ => None,
                }
            }
        }
    }
}

pub(crate) fn generate_enum_type(enum_decl: &EnumDecl) -> proc_macro2::TokenStream {
    let enum_ident = ident(&enum_decl.name);
    let docs = doc_attrs(&enum_decl.docs);
    let variants = enum_decl.variants.iter().map(|variant| {
        let variant_ident = ident(&variant.name);
        let variant_docs = doc_attrs(&variant.docs);
        let name = schema_lit(&variant.name);
        quote! {
            #variant_docs
            #[serde(rename = #name)]
            #variant_ident,
        }
    });
    let as_str_arms = enum_decl.variants.iter().map(|variant| {
        let variant_ident = ident(&variant.name);
        let name = schema_lit(&variant.name);
        quote! {
            Self::#variant_ident => #name,
        }
    });
    let parse_arms = enum_decl.variants.iter().map(|variant| {
        let variant_ident = ident(&variant.name);
        let name = schema_lit(&variant.name);
        quote! {
            #name => Ok(Self::#variant_ident),
        }
    });
    let enum_name = schema_lit(&enum_decl.name);

    quote! {
        #docs
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        pub enum #enum_ident {
            #(#variants)*
        }

        impl #enum_ident {
            pub const fn as_str(self) -> &'static str {
                match self {
                    #(#as_str_arms)*
                }
            }
        }

        impl ::core::fmt::Display for #enum_ident {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl ::core::str::FromStr for #enum_ident {
            type Err = String;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    #(#parse_arms)*
                    _ => Err(format!("unknown enum variant `{value}` for `{}`", #enum_name)),
                }
            }
        }

        impl ::cratestack::IntoSqlValue for #enum_ident {
            fn into_sql_value(self) -> ::cratestack::SqlValue {
                ::cratestack::SqlValue::String(self.to_string())
            }
        }
    }
}

pub(crate) fn generate_custom_field_descriptors(ty: &TypeDecl) -> Vec<proc_macro2::TokenStream> {
    ty.fields
        .iter()
        .filter(|field| is_custom_field(field))
        .map(|field| {
            let owner = schema_lit(&ty.name);
            let field_name = schema_lit(&field.name);
            let resolver_method = schema_lit(&format!(
                "resolve_{}_{}",
                to_snake_case(&ty.name),
                to_snake_case(&field.name)
            ));
            quote! {
                CustomFieldDescriptor {
                    owner: #owner,
                    field: #field_name,
                    resolver_method: #resolver_method,
                }
            }
        })
        .collect()
}

pub(crate) fn generate_custom_field_resolver_methods(
    ty: &TypeDecl,
) -> Vec<proc_macro2::TokenStream> {
    let type_ident = ident(&ty.name);
    ty.fields
        .iter()
        .filter(|field| is_custom_field(field))
        .map(|field| {
            let method_ident = ident(&format!(
                "resolve_{}_{}",
                to_snake_case(&ty.name),
                to_snake_case(&field.name)
            ));
            let return_type = rust_type_tokens_with_scope(&field.ty, true);

            quote! {
                fn #method_ident(
                    &self,
                    source: &super::#type_ident,
                    ctx: &::cratestack::CoolContext,
                ) -> impl ::core::future::Future<Output = Result<#return_type, ::cratestack::CoolError>> + Send;
            }
        })
        .collect()
}
