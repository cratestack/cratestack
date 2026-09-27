//! Defence in depth for policy attributes (GHSA-69g4-xvcm-vm2j).
//!
//! The policy readers (`super::procedure`, `crate::procedure::authorizer`,
//! `super::model`) recognise an attribute only in its exact spelling and
//! return `None` for anything else, which their callers skip. The parser
//! now refuses every other spelling, but a skipped policy fails open, so
//! the generator does not rely on that: it re-reads every attribute
//! *loosely* — any case, whitespace after the `@`s, anywhere on the text
//! outside strings and argument groups — and refuses to generate code
//! when an attribute that reads as a policy was not turned into one.
//!
//! Two independent checks, so that disabling either leaves the other:
//!
//! 1. [`check_attribute`]: an attribute whose own name reads as a policy
//!    must have been read by the exact reader. It also refuses an
//!    attribute carrying an invisible character anywhere, strings
//!    included (maintainer decision 3): `hasRole("ad\u{200B}min")` is read,
//!    but compares against a value the reviewer never saw. In a policy that
//!    includes every variation selector (`hasRole("banné\u{FE0F}")`), and
//!    the error quotes the attribute escaped, never with the raw character.
//! 2. [`check_count`]: across the declaration, the policy names in must
//!    equal the policies out — which also catches a policy hidden behind
//!    another attribute on the same raw text (`@no_idempotency @deny(x)`).

use cratestack_core::Attribute;
use cratestack_core::schema::attribute_text::{
    describe_invisible_character, escape_for_diagnostic, invisible_character,
    invisible_character_in_policy, loose_attribute_names,
};

/// How many attribute names in `raw` read as one of `names`.
pub(crate) fn policy_names_in(raw: &str, names: &[&str]) -> usize {
    loose_attribute_names(raw)
        .iter()
        .filter(|name| names.contains(&name.as_str()))
        .count()
}

/// Check 1 for one attribute of `owner`: `produced` is how many policies
/// the exact readers made from it. An attribute this check reads as a
/// policy refuses every variation selector, whatever `core` reads its name
/// as. The attribute is quoted with its control and invisible characters
/// escaped, never raw.
pub(crate) fn check_attribute(
    owner: &str,
    attribute: &Attribute,
    names: &[&str],
    produced: usize,
) -> Result<(), String> {
    let own_name = loose_attribute_names(&attribute.raw).into_iter().next();
    let is_policy = own_name
        .as_ref()
        .is_some_and(|name| names.contains(&name.as_str()));
    let quoted = escape_for_diagnostic(&attribute.raw);
    let invisible = if is_policy {
        invisible_character_in_policy(&attribute.raw)
    } else {
        invisible_character(&attribute.raw)
    };
    match own_name {
        Some(name) if produced == 0 && is_policy => Err(format!(
            "{owner}: `{quoted}` reads as a `{name}` policy attribute, but not in the exact form \
             the generator applies, so it would be skipped and the {owner} would be more \
             permissive than written. Refusing to generate it (GHSA-69g4-xvcm-vm2j); write it \
             exactly as documented"
        )),
        _ => match invisible {
            Some((_, ch)) => Err(format!(
                "{owner}: `{quoted}` contains {}, an invisible character, so it does not \
                 read the way it displays. Refusing to generate it (GHSA-69g4-xvcm-vm2j); \
                 remove the character",
                describe_invisible_character(ch)
            )),
            None => Ok(()),
        },
    }
}

/// Check 2 for `owner`: `named` policy names read loosely, `produced`
/// policies generated.
pub(crate) fn check_count(owner: &str, named: usize, produced: usize) -> Result<(), String> {
    if named == produced {
        return Ok(());
    }
    Err(format!(
        "{owner}: its attributes name {named} policy rule(s) but {produced} were generated, so \
         at least one would be silently skipped and the {owner} would be more permissive than \
         written. Refusing to generate it (GHSA-69g4-xvcm-vm2j); write each policy attribute \
         exactly as documented, as an attribute of its own"
    ))
}
