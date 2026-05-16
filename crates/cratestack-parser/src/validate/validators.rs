use crate::diagnostics::{SchemaError, span_error};
use crate::validate::validator_args::{parse_length_args, parse_range_args, parse_regex_arg};

/// Recognise the validation attribute family (`@length`, `@range`, `@regex`,
/// `@email`, `@uri`, `@iso4217`) and reject combinations that don't match the
/// field's scalar type. This is parse-time only — runtime enforcement happens
/// in generated `validate` impls on Create/Update inputs.
pub(super) fn validate_validator_attributes(
    model_name: &str,
    field: &cratestack_core::Field,
) -> Result<(), SchemaError> {
    let scalar = field.ty.name.as_str();
    for attribute in &field.attributes {
        let raw = attribute.raw.as_str();
        let (name, has_args) = if let Some(open) = raw.find('(') {
            (&raw[1..open], true)
        } else {
            (&raw[1..], false)
        };
        match name {
            "length" => check_length(model_name, field, scalar, raw, has_args)?,
            "range" => check_range(model_name, field, scalar, raw, has_args)?,
            "regex" => check_regex(model_name, field, scalar, raw, has_args)?,
            "email" | "uri" | "iso4217" => {
                check_string_only(model_name, field, scalar, name, has_args)?
            }
            _ => {} // unknown attribute; left to other validators
        }
    }
    Ok(())
}

fn check_length(
    model_name: &str,
    field: &cratestack_core::Field,
    scalar: &str,
    raw: &str,
    has_args: bool,
) -> Result<(), SchemaError> {
    if !has_args {
        return Err(span_error(
            format!(
                "field `{}.{}` @length requires arguments like @length(min: 1, max: 200)",
                model_name, field.name,
            ),
            field.span,
        ));
    }
    if scalar != "String" && scalar != "Bytes" {
        return Err(span_error(
            format!(
                "@length on `{}.{}` is only valid on String or Bytes fields",
                model_name, field.name,
            ),
            field.span,
        ));
    }
    parse_length_args(raw).map_err(|message| {
        span_error(
            format!("field `{}.{}`: {message}", model_name, field.name,),
            field.span,
        )
    })?;
    Ok(())
}

fn check_range(
    model_name: &str,
    field: &cratestack_core::Field,
    scalar: &str,
    raw: &str,
    has_args: bool,
) -> Result<(), SchemaError> {
    if !has_args {
        return Err(span_error(
            format!(
                "field `{}.{}` @range requires arguments like @range(min: 0, max: 100)",
                model_name, field.name,
            ),
            field.span,
        ));
    }
    if scalar != "Int" && scalar != "Decimal" {
        return Err(span_error(
            format!(
                "@range on `{}.{}` is only valid on Int or Decimal fields",
                model_name, field.name,
            ),
            field.span,
        ));
    }
    parse_range_args(raw).map_err(|message| {
        span_error(
            format!("field `{}.{}`: {message}", model_name, field.name,),
            field.span,
        )
    })?;
    Ok(())
}

fn check_regex(
    model_name: &str,
    field: &cratestack_core::Field,
    scalar: &str,
    raw: &str,
    has_args: bool,
) -> Result<(), SchemaError> {
    if !has_args {
        return Err(span_error(
            format!(
                "field `{}.{}` @regex requires a string argument",
                model_name, field.name,
            ),
            field.span,
        ));
    }
    if scalar != "String" {
        return Err(span_error(
            format!(
                "@regex on `{}.{}` is only valid on String fields",
                model_name, field.name,
            ),
            field.span,
        ));
    }
    parse_regex_arg(raw).map_err(|message| {
        span_error(
            format!("field `{}.{}`: {message}", model_name, field.name,),
            field.span,
        )
    })?;
    Ok(())
}

fn check_string_only(
    model_name: &str,
    field: &cratestack_core::Field,
    scalar: &str,
    name: &str,
    has_args: bool,
) -> Result<(), SchemaError> {
    if has_args {
        return Err(span_error(
            format!(
                "field `{}.{}` @{name} does not take arguments",
                model_name, field.name,
            ),
            field.span,
        ));
    }
    if scalar != "String" {
        return Err(span_error(
            format!(
                "@{name} on `{}.{}` is only valid on String fields",
                model_name, field.name,
            ),
            field.span,
        ));
    }
    Ok(())
}
