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
    // These two "this field is wrapped in an `Option`" conditions are
    // independent, not a single boolean: `treat_as_optional` means "the
    // field may be entirely omitted from this update" (every field on
    // `Update{Model}Input` gets this wrapper), while `TypeArity::Optional`
    // means "the column itself is nullable" (`Some(None)` is a real,
    // distinct value: "set this column to NULL"). A nullable field on an
    // update input is `Option<Option<T>>` and needs two unwraps, not one —
    // see cratestack#537.
    let nullable = matches!(field.ty.arity, TypeArity::Optional);

    let calls = validators
        .iter()
        .enumerate()
        .map(|(idx, v)| emit_one(field, scalar, idx, v));

    match (treat_as_optional, nullable) {
        (true, true) => quote! {
            // `None` = field omitted (skip); `Some(None)` = explicit
            // "set to NULL" (also skip — nothing to length/range-check
            // about the absence of a value, matching how a nullable
            // field on `Create{Model}Input` is allowed to be null in the
            // first place); `Some(Some(value))` = a real new value, so
            // this is where validators actually run.
            if let Some(Some(value)) = self.#field_ident.as_ref() {
                let _ = value;
                #(#calls)*
            }
        },
        (true, false) | (false, true) => quote! {
            if let Some(value) = self.#field_ident.as_ref() {
                let _ = value;
                #(#calls)*
            }
        },
        (false, false) => quote! {
            {
                let value = &self.#field_ident;
                let _ = value;
                #(#calls)*
            }
        },
    }
}

fn emit_one(field: &Field, scalar: &str, idx: usize, v: &FieldValidator) -> TokenStream {
    let field_name = &field.name;
    match v {
        FieldValidator::Length { min, max } => emit_length(field_name, scalar, *min, *max),
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

// Dispatches on the field's scalar the same way `emit_range` dispatches
// `Int`/`Decimal` — see cratestack#572. `String` and `Bytes` are the only
// scalars the parser accepts `@length` on
// (`crates/cratestack-parser/src/validate/validators.rs::check_length`);
// `Bytes` generates as `Vec<u8>` and needs `&[u8]`, not `&str`, so a
// single `validate_length(field, value, ..)` call can't type-check both.
fn emit_length(field_name: &str, scalar: &str, min: Option<u32>, max: Option<u32>) -> TokenStream {
    let min_tok = optional_usize(min.map(|n| n as usize));
    let max_tok = optional_usize(max.map(|n| n as usize));
    match scalar {
        "Bytes" => quote! {
            ::cratestack::validate_length_bytes(#field_name, value, #min_tok, #max_tok)?;
        },
        // "String" and anything else the parser might loosen in the
        // future: keep the pre-existing `&str` call as the default so an
        // unrecognized-but-string-shaped scalar doesn't silently no-op.
        _ => quote! {
            ::cratestack::validate_length(#field_name, value, #min_tok, #max_tok)?;
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
