//! Generate the body of `validate(&self) -> Result<(), CratestackError>` for
//! the given input fields, based on `@length`, `@range`, `@regex`,
//! `@email`, `@uri`, `@iso4217` attributes.
//!
//! The same field validators run in two places, one emitter for both
//! ([`FieldScope`] only changes how an error names the field): a model's
//! create and update inputs, and the `type`s and `model`s that a client
//! sends as procedure arguments or `@computed` params ([`types`]).

mod emit;
mod parse;
#[cfg(test)]
mod tests_fixture;
#[cfg(test)]
mod tests_models;
#[cfg(test)]
mod tests_types;
mod types;
mod validating;

use cratestack_core::Field;
use proc_macro2::TokenStream;
use quote::quote;

use emit::emit_field_validators;
use parse::{parse_length_args, parse_range_args, parse_regex_arg};

pub(crate) use types::{
    generate_args_validate_impl, generate_model_validate_impl, generate_type_validate_impl,
    procedure_validates_args,
};
pub(crate) use validating::Validating;

/// How an error message names the field that failed.
#[derive(Debug, Clone, Copy)]
pub(super) enum FieldScope {
    /// By its schema name: a model's input struct is the whole payload.
    Input,
    /// By its path in the request body (`args.items[2].name`): a `type`
    /// or `model` value sits anywhere in a procedure's arguments, and the
    /// generated `validate_at` receives where as a `FieldPath`.
    Nested,
}

#[derive(Debug, Clone)]
pub(super) enum FieldValidator {
    Length { min: Option<u32>, max: Option<u32> },
    Range { min: Option<i64>, max: Option<i64> },
    Regex { pattern: String },
    Email,
    Uri,
    Iso4217,
}

/// Generate the body of `validate(&self) -> Result<(), CratestackError>` for
/// the given input fields. Returns `None` if no field declares a
/// validator (the trait's default impl is fine).
pub(crate) fn generate_input_validate_body(
    fields: &[&Field],
    treat_as_optional: bool,
) -> Option<TokenStream> {
    let mut any = false;
    let per_field = fields
        .iter()
        .filter_map(|field| {
            let validators = parse_field_validators(field);
            if validators.is_empty() {
                return None;
            }
            any = true;
            Some(emit_field_validators(
                field,
                &validators,
                treat_as_optional,
                FieldScope::Input,
            ))
        })
        .collect::<Vec<_>>();
    if !any {
        return None;
    }
    Some(quote! {
        #(#per_field)*
        Ok(())
    })
}

fn parse_field_validators(field: &Field) -> Vec<FieldValidator> {
    let mut validators = Vec::new();
    for attribute in &field.attributes {
        let raw = attribute.raw.as_str();
        let (name, has_args) = if let Some(open) = raw.find('(') {
            (&raw[1..open], true)
        } else {
            (&raw[1..], false)
        };
        match (name, has_args) {
            ("length", true) => {
                if let Ok((min, max)) = parse_length_args(raw) {
                    validators.push(FieldValidator::Length { min, max });
                }
            }
            ("range", true) => {
                if let Ok((min, max)) = parse_range_args(raw) {
                    validators.push(FieldValidator::Range { min, max });
                }
            }
            ("regex", true) => {
                if let Ok(pattern) = parse_regex_arg(raw) {
                    validators.push(FieldValidator::Regex { pattern });
                }
            }
            ("email", false) => validators.push(FieldValidator::Email),
            ("uri", false) => validators.push(FieldValidator::Uri),
            ("iso4217", false) => validators.push(FieldValidator::Iso4217),
            _ => {}
        }
    }
    validators
}
