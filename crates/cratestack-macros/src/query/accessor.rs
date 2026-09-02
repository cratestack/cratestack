//! `Cratestack::queries().<query_snake>(args, ctx)` — the discoverable
//! call shape (cratestack#867), mirroring `runtime.views().<view>()`.
//!
//! This is a **forwarder, not a second entry point**: its whole body is a
//! call to the query module's own `run`, which is where the `@allow`
//! check lives. That distinction matters and is the reason this file is
//! allowed to exist at all under design §6 — the hazard cratestack#512
//! closed was a call shape that *skipped* the check, not one that goes
//! through it. Anything added here must keep that property: forward to
//! `run`, never re-implement what it does.
//!
//! It exists because the acceptance bar is that a caller reaches the query
//! without naming any `sqlx` type *and* without having to know the
//! generated module path by heart; `db.queries().loyalty_fee_summary(...)`
//! is discoverable from the handle by autocomplete, the free function is
//! not.

use cratestack_core::Query;
use quote::quote;

use crate::shared::{doc_attrs, ident, to_snake_case};

pub(crate) fn generate_query_accessor(query: &Query) -> proc_macro2::TokenStream {
    let method_ident = ident(&to_snake_case(&query.name));
    let module_ident = method_ident.clone();
    let docs = doc_attrs(&query.docs);

    quote! {
        #docs
        ///
        /// Forwards to this query's generated `run`, which is where the
        /// `@allow`/`@deny` check happens — see that function's doc
        /// comment, including the note that the policy gates *whether the
        /// call is permitted*, not which rows the SQL matches.
        pub async fn #method_ident(
            &self,
            args: &#module_ident::Args,
            ctx: &::cratestack::CratestackContext,
        ) -> Result<#module_ident::Output, ::cratestack::CratestackError> {
            #module_ident::run(self.db, args, ctx).await
        }
    }
}
