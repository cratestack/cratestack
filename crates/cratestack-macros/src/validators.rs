use cratestack_core::{Field, TypeArity};
use proc_macro2::TokenStream;
use quote::quote;

use crate::shared::ident;

#[derive(Debug, Clone)]
enum FieldValidator {
    Length { min: Option<u32>, max: Option<u32> },
    Range { min: Option<i64>, max: Option<i64> },
    Regex { pattern: String },
    Email,
    Uri,
    Iso4217,
}

/// Generate the body of `validate(&self) -> Result<(), CoolError>` for the
/// given input fields. Returns `None` if no field declares a validator (the
/// trait's default impl is fine).
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

fn emit_field_validators(
    field: &Field,
    validators: &[FieldValidator],
    treat_as_optional: bool,
) -> TokenStream {
    let field_ident = ident(&field.name);
    let field_name = &field.name;
    let scalar = field.ty.name.as_str();
    let is_optional = treat_as_optional || matches!(field.ty.arity, TypeArity::Optional);

    let calls = validators.iter().enumerate().map(|(idx, v)| match v {
        FieldValidator::Length { min, max } => {
            let min_tok = match min {
                Some(n) => {
                    let n = *n as usize;
                    quote! { Some(#n) }
                }
                None => quote! { None },
            };
            let max_tok = match max {
                Some(n) => {
                    let n = *n as usize;
                    quote! { Some(#n) }
                }
                None => quote! { None },
            };
            quote! {
                ::cratestack::validate_length(#field_name, value, #min_tok, #max_tok)?;
            }
        }
        FieldValidator::Range { min, max } => {
            // Only Int is plumbed through Phase 1 — Decimal range is wired
            // when the Decimal scalar lands.
            if scalar != "Int" {
                return quote! {};
            }
            let min_tok = match min {
                Some(n) => {
                    let n: i64 = *n;
                    quote! { Some(#n) }
                }
                None => quote! { None },
            };
            let max_tok = match max {
                Some(n) => {
                    let n: i64 = *n;
                    quote! { Some(#n) }
                }
                None => quote! { None },
            };
            quote! {
                ::cratestack::validate_range_i64(#field_name, *value, #min_tok, #max_tok)?;
            }
        }
        FieldValidator::Regex { pattern } => {
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
        FieldValidator::Email => quote! {
            ::cratestack::validate_email(#field_name, value)?;
        },
        FieldValidator::Uri => quote! {
            ::cratestack::validate_uri(#field_name, value)?;
        },
        FieldValidator::Iso4217 => quote! {
            ::cratestack::validate_iso4217(#field_name, value)?;
        },
    });

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

// Local re-implementations of the parser argument helpers — keeps this
// crate from depending on internals of cratestack-parser. The shapes are
// trivial; if they drift we'll lift them into a shared crate.
fn parse_length_args(raw: &str) -> Result<(Option<u32>, Option<u32>), String> {
    let inner = strip_attr_parens(raw, "length")?;
    let (mut min, mut max) = (None, None);
    for part in split_kv_args(&inner) {
        let (key, value) = split_kv(&part)?;
        let parsed: u32 = value.parse().map_err(|_| format!("bad u32: {value}"))?;
        match key.as_str() {
            "min" => min = Some(parsed),
            "max" => max = Some(parsed),
            _ => return Err(format!("unknown @length arg: {key}")),
        }
    }
    Ok((min, max))
}

fn parse_range_args(raw: &str) -> Result<(Option<i64>, Option<i64>), String> {
    let inner = strip_attr_parens(raw, "range")?;
    let (mut min, mut max) = (None, None);
    for part in split_kv_args(&inner) {
        let (key, value) = split_kv(&part)?;
        let parsed: i64 = value.parse().map_err(|_| format!("bad i64: {value}"))?;
        match key.as_str() {
            "min" => min = Some(parsed),
            "max" => max = Some(parsed),
            _ => return Err(format!("unknown @range arg: {key}")),
        }
    }
    Ok((min, max))
}

fn parse_regex_arg(raw: &str) -> Result<String, String> {
    let inner = strip_attr_parens(raw, "regex")?;
    let trimmed = inner.trim();
    let stripped = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| "expected quoted string".to_owned())?;
    Ok(stripped.to_owned())
}

fn strip_attr_parens(raw: &str, name: &str) -> Result<String, String> {
    let prefix = format!("@{name}(");
    let trimmed = raw.strip_prefix(&prefix).ok_or("malformed")?;
    let inner = trimmed.strip_suffix(')').ok_or("missing close paren")?;
    Ok(inner.to_owned())
}

fn split_kv_args(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

fn split_kv(part: &str) -> Result<(String, String), String> {
    let (key, value) = part.split_once(':').ok_or("expected key: value")?;
    Ok((key.trim().to_owned(), value.trim().to_owned()))
}
