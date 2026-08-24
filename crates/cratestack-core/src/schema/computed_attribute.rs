//! Parsing for the `@computed(params: <Type>?)` field attribute's
//! parenthesized argument. Bare `@computed` needs no parsing; this
//! module only handles the parameterized form. Mirrors
//! [`super::composite_key`]'s shape: syntax parsing lives here in
//! `cratestack-core` so `cratestack-parser`'s semantic checker and any
//! other consumer (codegen, later) share one implementation.

use super::model::Field;

/// The parenthesized argument of a `@computed(...)` attribute, however
/// it parses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputedParamsArg<'a> {
    /// `params: <Type>?` — the only argument form the language accepts.
    Optional(&'a str),
    /// `params: <Type>` without the trailing `?` — a recognized shape
    /// (a plausible typo, not garbage), but required computed params
    /// aren't supported yet: v1 always makes params optional, since a
    /// required param would make plain CRUD reads unsatisfiable and
    /// there's no wire slot for one on non-read paths.
    Required(&'a str),
    /// Any other argument spelling (wrong keyword, missing colon, not a
    /// valid identifier, ...).
    Unsupported,
}

/// Parses the text between (but not including) the parens of a
/// `@computed(...)` attribute into its recognized shape.
/// Whitespace-tolerant around the `:` and before the trailing `?`.
pub fn parse_computed_params_arg(inner: &str) -> ComputedParamsArg<'_> {
    let inner = inner.trim();
    let Some(rest) = inner.strip_prefix("params") else {
        return ComputedParamsArg::Unsupported;
    };
    let Some(rest) = rest.trim_start().strip_prefix(':') else {
        return ComputedParamsArg::Unsupported;
    };
    let rest = rest.trim();

    if let Some(name) = rest.strip_suffix('?') {
        let name = name.trim_end();
        return if is_valid_type_name(name) {
            ComputedParamsArg::Optional(name)
        } else {
            ComputedParamsArg::Unsupported
        };
    }

    if is_valid_type_name(rest) {
        ComputedParamsArg::Required(rest)
    } else {
        ComputedParamsArg::Unsupported
    }
}

fn is_valid_type_name(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// The params type name off a field's `@computed(params: <Type>?)`
/// attribute, or `None` for a bare `@computed` field (or a field with no
/// `@computed` attribute at all). Assumes the attribute is already
/// well-formed — per-declaration validation (e.g.
/// `cratestack-parser`'s `validate_computed_field_attribute`) must run
/// first and reject anything else. Callers that still need to
/// *validate* the argument form should parse the raw attribute text
/// with [`parse_computed_params_arg`] instead.
pub fn computed_params_type_name(field: &Field) -> Option<&str> {
    let attribute = field
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@computed"))?;
    let inner = attribute
        .raw
        .strip_prefix("@computed(")
        .and_then(|rest| rest.strip_suffix(')'))?;
    match parse_computed_params_arg(inner) {
        ComputedParamsArg::Optional(name) => Some(name),
        ComputedParamsArg::Required(_) | ComputedParamsArg::Unsupported => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_optional_params() {
        assert_eq!(
            parse_computed_params_arg("params: ProxyParams?"),
            ComputedParamsArg::Optional("ProxyParams")
        );
    }

    #[test]
    fn parses_optional_params_whitespace_tolerant() {
        assert_eq!(
            parse_computed_params_arg("params :ProxyParams ?"),
            ComputedParamsArg::Optional("ProxyParams")
        );
    }

    #[test]
    fn parses_required_params_missing_question_mark() {
        assert_eq!(
            parse_computed_params_arg("params: ProxyParams"),
            ComputedParamsArg::Required("ProxyParams")
        );
    }

    #[test]
    fn rejects_missing_colon() {
        assert_eq!(
            parse_computed_params_arg("params ProxyParams?"),
            ComputedParamsArg::Unsupported
        );
    }

    #[test]
    fn rejects_unrelated_argument() {
        assert_eq!(
            parse_computed_params_arg("lazy"),
            ComputedParamsArg::Unsupported
        );
    }
}
