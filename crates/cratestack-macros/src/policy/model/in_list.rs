//! `field in [A, B, C]` / `field not in [A, B, C]` set-membership terms
//! for model `@@allow`/`@@deny` read policies (issue #666).
//!
//! The element type is whatever [`parse_policy_literal`] already
//! accepts — required `Boolean`/`Int`/`BigInt`/`String`/enum fields — so this is
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
use cratestack_policy::split_list_elements;
use quote::quote;

use super::literal::parse_policy_literal;
use super::predicates::{find_model_field, generate_scalar_in_predicate};
use super::relation_path::resolve_relation_policy_field;

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
        field_decl.sql_column_name().as_str(),
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
