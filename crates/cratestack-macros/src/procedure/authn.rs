//! The `AUTHN` constant of a generated procedure module (ADR 0022,
//! cratestack#1244): the procedure's `@authn` requirement, read with
//! `cratestack_core`'s one reader, or `None`. The module's authorize step
//! passes it to `::cratestack::require_authn` after `@allow`/`@deny`.

use cratestack_core::Procedure;
use cratestack_core::schema::{authn_attribute, parse_authn_attribute};
use quote::quote;

pub(super) fn authn_const_tokens(
    procedure: &Procedure,
) -> Result<proc_macro2::TokenStream, String> {
    let Some(attribute) = authn_attribute(&procedure.attributes) else {
        return Ok(quote! {
            /// No `@authn` requirement.
            pub const AUTHN: Option<::cratestack::AuthnRequirement> = None;
        });
    };
    let parsed = parse_authn_attribute(&attribute.raw)
        .map_err(|reason| format!("procedure `{}`: {reason}", procedure.name))?;
    let max_age = match parsed.max_age {
        Some(seconds) => quote! { Some(#seconds) },
        None => quote! { None },
    };
    let (amr, acr) = (&parsed.amr, &parsed.acr);
    Ok(quote! {
        /// The procedure's `@authn` requirement (ADR 0022).
        pub const AUTHN: Option<::cratestack::AuthnRequirement> =
            Some(::cratestack::AuthnRequirement {
                max_age: #max_age,
                amr: &[#(#amr),*],
                acr: &[#(#acr),*],
            });
    })
}
