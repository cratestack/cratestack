//! Emit the per-field validator calls into the `validate()` body.

use cratestack_core::{Field, TypeArity};
use proc_macro2::TokenStream;
use quote::quote;

use crate::shared::ident;

use super::FieldValidator;

pub(super) fn emit_field_validators(
    field: &Field,
    validators: &[FieldValidator],
    treat_as_optional: bool,
) -> TokenStream {
    let field_ident = ident(&field.name);
    let scalar = field.ty.name.as_str();
    let is_optional = treat_as_optional || matches!(field.ty.arity, TypeArity::Optional);

    let calls = validators
        .iter()
        .enumerate()
        .map(|(idx, v)| emit_one(field, scalar, idx, v));

    if is_optional {
        quote! {
            if let Some(value) = self.#field_ident.as_ref() {
                let _ = value;
                #(#calls)*
            }
        }
    } else {
        quote! {
            {
                let value = &self.#field_ident;
                let _ = value;
                #(#calls)*
            }
        }
    }
}

fn emit_one(field: &Field, scalar: &str, idx: usize, v: &FieldValidator) -> TokenStream {
    let field_name = &field.name;
    match v {
        FieldValidator::Length { min, max } => {
            let min_tok = optional_usize(min.map(|n| n as usize));
            let max_tok = optional_usize(max.map(|n| n as usize));
            quote! {
                ::cratestack::validate_length(#field_name, value, #min_tok, #max_tok)?;
            }
        }
        FieldValidator::Range { min, max } => emit_range(field_name, scalar, *min, *max),
        FieldValidator::Regex { pattern } => emit_regex(field, idx, pattern),
        FieldValidator::Email => quote! {
            ::cratestack::validate_email(#field_name, value)?;
        },
        FieldValidator::Uri => quote! {
            ::cratestack::validate_uri(#field_name, value)?;
        },
        FieldValidator::Iso4217 => quote! {
            ::cratestack::validate_iso4217(#field_name, value)?;
        },
    }
}

fn emit_range(field_name: &str, scalar: &str, min: Option<i64>, max: Option<i64>) -> TokenStream {
    let min_tok = optional_i64(min);
    let max_tok = optional_i64(max);
    match scalar {
        "Int" => quote! {
            ::cratestack::validate_range_i64(#field_name, *value, #min_tok, #max_tok)?;
        },
        // Decimal bounds in `.cstack` are specified as integers (the
        // parser only accepts i64 literals); the runtime helper promotes
        // them to Decimal for comparison. That's enough for banking use
        // cases like `amount Decimal @range(min: 0)` — fractional bounds
        // need a separate syntax change, tracked outside this PR.
        "Decimal" => quote! {
            ::cratestack::validate_range_decimal(#field_name, value, #min_tok, #max_tok)?;
        },
        // Unknown scalar: the parser shouldn't have accepted the attribute
        // in the first place; we'd rather emit nothing than a type-
        // confused call.
        _ => quote! {},
    }
}

fn emit_regex(field: &Field, idx: usize, pattern: &str) -> TokenStream {
    let field_name = &field.name;
    let regex_ident = ident(&format!(
        "__VALIDATOR_REGEX_{}_{}",
        field.name.to_uppercase(),
        idx
    ));
    quote! {
        static #regex_ident: ::std::sync::LazyLock<::cratestack::regex::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::cratestack::regex::Regex::new(#pattern)
                    .expect("compile-validated @regex pattern must compile")
            });
        if !#regex_ident.is_match(value) {
            return Err(::cratestack::CoolError::Validation(format!(
                "field '{}' does not match required pattern", #field_name,
            )));
        }
    }
}

fn optional_usize(value: Option<usize>) -> TokenStream {
    match value {
        Some(n) => quote! { Some(#n) },
        None => quote! { None },
    }
}

fn optional_i64(value: Option<i64>) -> TokenStream {
    match value {
        Some(n) => quote! { Some(#n) },
        None => quote! { None },
    }
}
