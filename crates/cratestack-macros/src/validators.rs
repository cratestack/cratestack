//! Generate the body of `validate(&self) -> Result<(), CoolError>` for
//! the given input fields, based on `@length`, `@range`, `@regex`,
//! `@email`, `@uri`, `@iso4217` attributes.

mod emit;
mod parse;

use cratestack_core::Field;
use proc_macro2::TokenStream;
use quote::quote;

use emit::emit_field_validators;
use parse::{parse_length_args, parse_range_args, parse_regex_arg};

#[derive(Debug, Clone)]
pub(super) enum FieldValidator {
    Length { min: Option<u32>, max: Option<u32> },
    Range { min: Option<i64>, max: Option<i64> },
    Regex { pattern: String },
    Email,
    Uri,
    Iso4217,
}

/// Generate the body of `validate(&self) -> Result<(), CoolError>` for
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
            Some(emit_field_validators(field, &validators, treat_as_optional))
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
