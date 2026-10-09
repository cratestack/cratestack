//! Policy literal parsing: the right-hand side of `field == <literal>`,
//! `field != <literal>` and each element of `field in [...]` in a model
//! `@@allow`/`@@deny`, lowered to a `::cratestack::PolicyLiteral`. Split out
//! of `predicates.rs` per the repo's 200-LoC file convention.

use cratestack_core::{EnumDecl, Field, TypeArity};
use quote::quote;

use crate::policy::auth::parse_string_literal;

use super::enum_literal::parse_enum_policy_literal;

pub(super) fn parse_policy_literal(
    rhs: &str,
    field: &Field,
    enums: &[EnumDecl],
) -> Result<proc_macro2::TokenStream, String> {
    match field.ty.name.as_str() {
        "Boolean" if field.ty.arity == TypeArity::Required => match rhs {
            "true" => Ok(quote! { ::cratestack::PolicyLiteral::Bool(true) }),
            "false" => Ok(quote! { ::cratestack::PolicyLiteral::Bool(false) }),
            _ => Err(format!(
                "expected boolean literal for field `{}`",
                field.name
            )),
        },
        "Int" if field.ty.arity == TypeArity::Required => rhs
            .parse::<i64>()
            .map(|value| quote! { ::cratestack::PolicyLiteral::Int(#value) })
            .map_err(|_| format!("expected integer literal for field `{}`", field.name)),
        // `PolicyLiteral::Int` is an `i64` and a `BigInt` column is an `INT8`,
        // so the literal needs no variant of its own; the comparison against
        // a `SqlValue::BigInt` is `cratestack-sqlx`'s `sql_value_matches_literal`.
        // A schema-authored literal is read as `i64` here (the wire grammar
        // governs what a client sends, not what a schema author writes), and
        // one outside `i64` is refused at expansion.
        "BigInt" if field.ty.arity == TypeArity::Required => rhs
            .parse::<i64>()
            .map(|value| quote! { ::cratestack::PolicyLiteral::Int(#value) })
            .map_err(|_| format!("expected integer literal for field `{}`", field.name)),
        "String" if field.ty.arity == TypeArity::Required => {
            let value = parse_string_literal(rhs)
                .ok_or_else(|| format!("expected string literal for field `{}`", field.name))?;
            Ok(quote! { ::cratestack::PolicyLiteral::String(#value) })
        }
        type_name if enums.iter().any(|enum_decl| enum_decl.name == type_name) => {
            parse_enum_policy_literal(rhs, field, enums)
        }
        _ => Err(format!(
            "literal read policy support is currently limited to required Boolean, Int, BigInt, String, and required Enum fields; `{}` is unsupported",
            field.name
        )),
    }
}
