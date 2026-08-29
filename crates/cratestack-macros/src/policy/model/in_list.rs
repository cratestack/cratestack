//! `field in [A, B, C]` / `field not in [A, B, C]` set-membership terms
//! for model `@@allow`/`@@deny` read policies (issue #666).
//!
//! The element type is whatever [`parse_policy_literal`] already
//! accepts — required `Boolean`/`Int`/`String`/enum fields — so this is
//! a new *shape*, not a new literal kind. Enum variants are the
//! motivating case (`purpose in [product_image, product_thumbnail]`),
//! which is what #666 asked for; restricting the shape to enums would
//! have been more code than allowing every type the equality arm
//! already supports.
//!
//! Why a dedicated `ReadPredicate` rather than desugaring to
//! `Or(FieldEqLiteral, ...)`: the desugaring is what schema authors do
//! today by hand, and it repeats the column name once per element and
//! renders as a nested `Or` tree. A flat `column IN (...)` is what a
//! hand-written policy query would say, and it is what #666's Expected
//! Result asks for ("mirroring how a hand-authored SQL `WHERE column =
//! ANY($1)` would express 'one of these purposes'").

use cratestack_core::{EnumDecl, Model};
use quote::quote;

use super::predicates::{find_model_field, generate_scalar_in_predicate, parse_policy_literal};
use super::relation_path::resolve_relation_policy_field;
use crate::shared::to_snake_case;

/// Recognise `lhs in [...]` / `lhs not in [...]`, returning the
/// left-hand side, the bracket contents, and whether it was negated.
///
/// Anchored on the first `[` and the term's final `]` rather than on a
/// substring search for `" in "`, so a value containing the keyword
/// (`purpose in ["shipped in transit"]`) cannot be mis-split.
pub(super) fn split_in_term(term: &str) -> Option<(&str, &str, bool)> {
    let open = term.find('[')?;
    let (head, list) = term.split_at(open);
    let list = list.strip_prefix('[')?.strip_suffix(']')?;

    let head = strip_keyword(head.trim_end(), "in")?;
    match strip_keyword(head, "not") {
        Some(lhs) => Some((lhs, list, true)),
        None => Some((head, list, false)),
    }
}

/// Strip a trailing whole-word `keyword`, returning what precedes it.
///
/// The whitespace check is load-bearing: without it a field named
/// `join` would strip to `jo` and be reported as an unknown model
/// field, which is a confusing way to say "this is not an `in` term".
fn strip_keyword<'a>(head: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = head.strip_suffix(keyword)?;
    if !rest.ends_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_end();
    (!rest.is_empty()).then_some(rest)
}

pub(super) fn parse_model_in_comparison(
    lhs: &str,
    list_source: &str,
    model: &Model,
    models: &[Model],
    enums: &[EnumDecl],
    negate: bool,
) -> Result<proc_macro2::TokenStream, String> {
    let elements = split_list_elements(list_source, lhs)?;

    if let Some(relation_field) = resolve_relation_policy_field(model, models, lhs)? {
        let values = lower_elements(&elements, relation_field.target_field, enums)?;
        let predicate =
            generate_scalar_in_predicate(relation_field.target_column.as_str(), values, negate);
        return Ok(super::predicates::wrap_relation_predicate(
            &relation_field,
            predicate,
        ));
    }

    let field_decl = find_model_field(model, lhs)?;
    let values = lower_elements(&elements, field_decl, enums)?;
    Ok(generate_scalar_in_predicate(
        to_snake_case(lhs).as_str(),
        values,
        negate,
    ))
}

fn lower_elements(
    elements: &[&str],
    field: &cratestack_core::Field,
    enums: &[EnumDecl],
) -> Result<proc_macro2::TokenStream, String> {
    let literals = elements
        .iter()
        .map(|element| parse_policy_literal(element, field, enums))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(quote! { &[#(#literals),*] })
}

/// Split the bracket contents on commas, ignoring commas inside a
/// quoted string literal so `["a,b", "c"]` is two elements, not three.
///
/// An empty list is rejected rather than lowered to a constant `FALSE`:
/// it is far more likely a truncated edit than an intentional
/// "match nothing", and SQL has no valid `IN ()` form to render it to.
fn split_list_elements<'a>(source: &'a str, lhs: &str) -> Result<Vec<&'a str>, String> {
    let mut elements = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<char> = None;

    for (index, character) in source.char_indices() {
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => {}
            None if character == '"' || character == '\'' => quote = Some(character),
            None if character == ',' => {
                elements.push(source[start..index].trim());
                start = index + character.len_utf8();
            }
            None => {}
        }
    }
    if quote.is_some() {
        return Err(format!(
            "unterminated string literal in the `in` list for `{lhs}`"
        ));
    }
    elements.push(source[start..].trim());

    if let Some(position) = elements.iter().position(|element| element.is_empty()) {
        return Err(if elements.len() == 1 {
            format!("`{lhs} in []` is empty; an `in` list needs at least one value")
        } else {
            format!(
                "empty value at position {} in the `in` list for `{lhs}` (a trailing or doubled comma)",
                position + 1
            )
        });
    }

    Ok(elements)
}

#[cfg(test)]
mod tests {
    use super::{split_in_term, split_list_elements};

    #[test]
    fn splits_a_plain_in_term() {
        assert_eq!(
            split_in_term("purpose in [product_image, kyc_selfie]"),
            Some(("purpose", "product_image, kyc_selfie", false))
        );
    }

    #[test]
    fn splits_a_negated_in_term() {
        assert_eq!(
            split_in_term("purpose not in [kyc_selfie]"),
            Some(("purpose", "kyc_selfie", true))
        );
    }

    /// Decisive test for the whitespace guard in [`strip_keyword`]: a
    /// field whose name merely *ends* in `in` is not an `in` term.
    #[test]
    fn a_field_named_join_is_not_an_in_term() {
        assert_eq!(split_in_term("join [a]"), None);
    }

    #[test]
    fn a_term_without_brackets_is_not_an_in_term() {
        assert_eq!(split_in_term("purpose == product_image"), None);
    }

    #[test]
    fn commas_inside_a_string_literal_do_not_split() {
        assert_eq!(
            split_list_elements("\"a,b\", \"c\"", "label").as_deref(),
            Ok(["\"a,b\"", "\"c\""].as_slice())
        );
    }

    #[test]
    fn an_empty_list_is_rejected() {
        let error = split_list_elements("", "purpose").expect_err("empty list must be rejected");
        assert!(error.contains("at least one value"), "got: {error}");
    }

    #[test]
    fn a_trailing_comma_is_rejected() {
        let error =
            split_list_elements("a, ", "purpose").expect_err("trailing comma must be rejected");
        assert!(error.contains("position 2"), "got: {error}");
    }
}
