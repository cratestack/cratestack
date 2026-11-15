//! Lowering a parsed policy expression to the tokens that build the runtime
//! policy value. The grammar itself lives in `cratestack-policy`, which the
//! policy analysis (`cratestack contract ops`) reads too.

use quote::quote;

pub(super) use cratestack_policy::{PolicyAst, parse_policy_ast};

pub(super) fn generate_policy_ast_tokens<TermFn>(
    ast: &PolicyAst,
    term_fn: &TermFn,
    and_path: proc_macro2::TokenStream,
    or_path: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream, String>
where
    TermFn: Fn(&str) -> Result<proc_macro2::TokenStream, String>,
{
    match ast {
        PolicyAst::Term(term) => term_fn(term),
        PolicyAst::And(parts) => {
            let generated = parts
                .iter()
                .map(|part| {
                    generate_policy_ast_tokens(part, term_fn, and_path.clone(), or_path.clone())
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(quote! { #and_path(&[#(#generated),*]) })
        }
        PolicyAst::Or(parts) => {
            let generated = parts
                .iter()
                .map(|part| {
                    generate_policy_ast_tokens(part, term_fn, and_path.clone(), or_path.clone())
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(quote! { #or_path(&[#(#generated),*]) })
        }
    }
}
