//! The `execute` arm of an `@isolation` tool: the same transaction-running
//! `invoke_with_db` the REST and RPC dispatch use, handing the registry
//! method the transaction-bound `IsolatedCratestack` instead of the pool
//! handle (docs/design/procedure-isolation.md §2). The closure is cloned per
//! attempt because a serialization failure runs it again. Its body is the
//! REST/RPC one (`crate::axum::isolated_attempt_body`), so a computed-bearing
//! output is composed inside the attempt here too (§6).

use std::collections::BTreeSet;

use quote::quote;

use crate::axum::isolated_attempt_body;

pub(super) fn isolated_execute_arm(
    procedure: &cratestack_core::Procedure,
    variant: &syn::Ident,
    module: &syn::Ident,
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    let (resolvers, body) = isolated_attempt_body(procedure, module, bearing);
    quote! {
        Call::#variant(args) => {
            let registry = state.registry.clone();
            #resolvers
            let call_ctx = ctx.clone();
            let call_args = args.clone();
            let result = super::procedures::#module::invoke_with_db(
                &state.db,
                &args,
                ctx,
                move |tx_db, authorized| async move {
                    #body
                },
            )
            .await
            // Only this tool's own exhausted retries release the key (§6).
            .map_err(::cratestack::CratestackError::__generated_claim_transaction_abort);
            ::cratestack::mcp::encode_output(&result?)
        }
    }
}
