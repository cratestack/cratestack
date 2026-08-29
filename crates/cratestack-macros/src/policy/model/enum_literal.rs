//! The enum arm of [`super::predicates::parse_policy_literal`] (issue
//! #666) — split into its own file to keep `predicates.rs` under this
//! crate's 200-LoC file convention.

use cratestack_core::{EnumDecl, Field, TypeArity};
use quote::quote;

/// A required enum-typed field compared against a bareword variant
/// name, e.g. `purpose == product_image`. Enum columns are stored as
/// `TEXT` holding the variant name verbatim (see
/// `crates/cratestack-macros/src/types/enums.rs`'s `as_str`/`Display`
/// impl and `cratestack-migrate`'s `CHECK (col IN (...))` emission for
/// enum columns), so a validated variant name lowers straight to the
/// existing `PolicyLiteral::String` — no new `PolicyLiteral` variant or
/// separate SQL-pushing path needed; equality/inequality reuse
/// `FieldEqLiteral`/`FieldNeLiteral` exactly as the String case does.
///
/// This arm is equality/inequality only. Set membership
/// (`purpose in [product_image, product_thumbnail]`) is a separate
/// *shape*, not a separate literal kind, and lives in [`super::in_list`]
/// — it needed the new multi-value `ReadPredicate::FieldInLiterals`
/// variant and its own SQL emitters, which is why it was deferred out
/// of the equality work and closed afterwards. Every element of an `in`
/// list still lands here one at a time, so variant validation and the
/// required-arity rule below apply identically to both shapes.
pub(super) fn parse_enum_policy_literal(
    rhs: &str,
    field: &Field,
    enums: &[EnumDecl],
) -> Result<proc_macro2::TokenStream, String> {
    if field.ty.arity != TypeArity::Required {
        return Err(format!(
            "literal read policy support for enum field `{}` requires the field to be required; optional/list enum fields are not supported",
            field.name
        ));
    }
    let enum_decl = enums
        .iter()
        .find(|enum_decl| enum_decl.name == field.ty.name)
        .expect("caller already matched field.ty.name against a known enum name");
    let variant = enum_decl
        .variants
        .iter()
        .find(|variant| variant.name == rhs)
        .ok_or_else(|| {
            let known = enum_decl
                .variants
                .iter()
                .map(|variant| variant.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "unknown variant `{rhs}` for enum `{}` in read policy for field `{}`; expected one of: {known}",
                enum_decl.name, field.name
            )
        })?;
    let value = variant.name.as_str();
    Ok(quote! { ::cratestack::PolicyLiteral::String(#value) })
}
