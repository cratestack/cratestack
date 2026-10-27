//! The embedded descriptor's literal and database-filled defaults
//! (cratestack#1234). The embedded runtime binds a literal `@default`
//! itself, since its tables are not built from migrations, and refuses a
//! create that leaves a `dbgenerated()` or registered-function column NULL.
//! Only `include_embedded_schema!` emits these: the server leaves both to
//! the database.

use cratestack_core::schema::default_functions::{DefaultValue, classify_default};
use cratestack_core::schema::default_literal::{LiteralValue, literal_default};
use cratestack_core::schema::default_markers::default_inner;
use cratestack_core::{EnumDecl, Field, Model, TypeArity};
use proc_macro2::{Literal, TokenStream};
use quote::quote;

use crate::shared::{ident, model_name_set, rust_type_tokens, scalar_model_fields, to_snake_case};

pub(super) fn collect_embedded_defaults(
    model: &Model,
    models: &[Model],
    enums: &[EnumDecl],
) -> Result<TokenStream, String> {
    let mut literals = Vec::new();
    let mut database = Vec::new();
    for field in scalar_model_fields(model, &model_name_set(models)) {
        let Some(inner) = default_inner(field) else {
            continue;
        };
        let column = to_snake_case(&field.name);
        match classify_default(inner) {
            DefaultValue::Literal => {
                let value = literal_value(model, field, inner, enums)?;
                literals.push(quote! {
                    ::cratestack::LiteralDefault { column: #column, value: || #value }
                });
            }
            DefaultValue::Marker(_) | DefaultValue::Function(_)
                if matches!(field.ty.arity, TypeArity::Required) =>
            {
                let default = inner.trim();
                database.push(quote! {
                    ::cratestack::DatabaseDefault { column: #column, default: #default }
                });
            }
            _ => {}
        }
    }
    Ok(quote! {
        .with_embedded_defaults(&[#(#literals),*], &[#(#database),*])
    })
}

/// The `SqlValue` the create input would bind for the literal.
fn literal_value(
    model: &Model,
    field: &Field,
    inner: &str,
    enums: &[EnumDecl],
) -> Result<TokenStream, String> {
    let decl = enums.iter().find(|decl| decl.name == field.ty.name);
    // `cratestack check` has refused every literal that does not fit.
    let literal = literal_default(inner, &field.ty, decl).map_err(|reason| {
        format!(
            "field `{}.{}` uses `@default({inner})`, but {reason}",
            model.name, field.name
        )
    })?;
    Ok(match literal.value {
        LiteralValue::Bool(value) => quote! { ::cratestack::SqlValue::Bool(#value) },
        LiteralValue::Int(value) if field.ty.name == "BigInt" => {
            quote! { ::cratestack::SqlValue::BigInt(#value) }
        }
        LiteralValue::Int(value) => quote! { ::cratestack::SqlValue::Int(#value) },
        LiteralValue::Float(value) => {
            let value = Literal::f64_unsuffixed(value);
            quote! { ::cratestack::SqlValue::Float(#value) }
        }
        LiteralValue::Decimal(text) => {
            let mut ty = field.ty.clone();
            ty.arity = TypeArity::Required;
            let decimal = rust_type_tokens(&ty);
            quote! {
                ::cratestack::SqlValue::Decimal(::std::boxed::Box::new(
                    <#decimal as ::std::str::FromStr>::from_str(#text)
                        .expect("a decimal default `cratestack check` accepted"),
                ))
            }
        }
        LiteralValue::Text(text) => {
            quote! { ::cratestack::SqlValue::String(::std::string::String::from(#text)) }
        }
        LiteralValue::Uuid(uuid) => {
            let bits = uuid.as_u128();
            quote! { ::cratestack::SqlValue::Uuid(::cratestack::uuid::Uuid::from_u128(#bits)) }
        }
        LiteralValue::DateTime(at) => {
            let (seconds, nanos) = (at.timestamp(), at.timestamp_subsec_nanos());
            quote! {
                ::cratestack::SqlValue::DateTime(
                    ::cratestack::chrono::DateTime::from_timestamp(#seconds, #nanos)
                        .expect("a timestamp default `cratestack check` accepted"),
                )
            }
        }
        LiteralValue::Enum { variant, .. } => {
            let (enum_ident, variant) = (ident(&field.ty.name), ident(&variant));
            quote! {
                ::cratestack::IntoSqlValue::into_sql_value(super::types::#enum_ident::#variant)
            }
        }
    })
}
