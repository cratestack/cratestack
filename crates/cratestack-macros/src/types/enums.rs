//! Generators for server and client enum types declared in `.cstack`.

use cratestack_core::EnumDecl;
use quote::quote;

use super::enums_ordinal::storage_items;
use super::enums_parts::{
    accepted_variants_lit, as_str_arms, derives, ordinal_impls_if_any, parse_arms,
    server_sql_value, variant_tokens,
};
use crate::shared::{doc_attrs, ident, schema_lit};

pub(crate) fn generate_enum_type(enum_decl: &EnumDecl) -> proc_macro2::TokenStream {
    enum_type(
        enum_decl,
        storage_items(enum_decl),
        server_sql_value(enum_decl),
    )
}

/// The client's `IntoSqlValue` mirrors the server's deliberately.
/// `<Model>Where`'s `to_filters()` is emitted by BOTH composers, and since
/// #928 made enum fields filterable it calls `FieldRef::eq/ne/in_`, which
/// bound `V: IntoSqlValue`. Without this, `include_client_schema!` fails to
/// compile with E0277 for any schema having an enum-typed model field — a
/// shape no client fixture had, which is why CI stayed green. It is always
/// the name, ordinal enum or not: on a client it only builds filters, and a
/// query string carries names (O2).
pub(crate) fn generate_client_enum_type(enum_decl: &EnumDecl) -> proc_macro2::TokenStream {
    enum_type(
        enum_decl,
        quote! {},
        quote! { ::cratestack::SqlValue::String(self.to_string()) },
    )
}

fn enum_type(
    enum_decl: &EnumDecl,
    hidden_items: proc_macro2::TokenStream,
    sql_value: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let enum_ident = ident(&enum_decl.name);
    let docs = doc_attrs(&enum_decl.docs);
    let derives = derives(enum_decl);
    let variants = variant_tokens(enum_decl);
    let as_str_arms = as_str_arms(enum_decl);
    let parse_arms = parse_arms(enum_decl);
    let enum_name = schema_lit(&enum_decl.name);
    let accepted_variants = accepted_variants_lit(enum_decl);
    let ordinal_impls = ordinal_impls_if_any(enum_decl);

    quote! {
        #docs
        #derives
        pub enum #enum_ident {
            #(#variants)*
        }

        impl #enum_ident {
            pub const fn as_str(self) -> &'static str {
                match self {
                    #(#as_str_arms)*
                }
            }

            #hidden_items
        }

        #ordinal_impls

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
                    _ => Err(format!(
                        "unknown enum variant `{value}` for `{}`; expected one of: {}",
                        #enum_name, #accepted_variants,
                    )),
                }
            }
        }

        impl ::cratestack::IntoSqlValue for #enum_ident {
            fn into_sql_value(self) -> ::cratestack::SqlValue {
                #sql_value
            }
        }
    }
}
