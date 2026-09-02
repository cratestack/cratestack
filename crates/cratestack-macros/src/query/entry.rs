//! The single generated entry point for a `query` block: `run`
//! (cratestack#867).
//!
//! **This function is the whole security argument.** Design §6: a `query`
//! has no user-implemented counterpart to `ProcedureRegistry`, so there is
//! no second, directly-callable place for execution to happen — which
//! means the `@allow`/`@deny` check can be, and is, unconditional inside
//! this one body, with nothing to bypass it *by construction*. That is
//! strictly stronger than `procedure`'s pre-cratestack#512 shape, and it
//! costs nothing to keep, **as long as no "unchecked"/raw twin is ever
//! added next to it** (design §7 lists that as an explicit exclusion, not
//! an oversight).
//!
//! Forward requirement, recorded here because this is where it would be
//! violated: if a future revision ever splits execution the way
//! `procedure` splits `authorize_with_db`/`invoke_with_db` — e.g. to batch
//! several queries' authorization ahead of running any of them — that
//! split re-creates exactly the two-call-shape gap cratestack#512 closed,
//! and **must** adopt the same unconstructible `Authorized` witness at
//! that point. Not discretionary.
//!
//! Binding: every declared parameter goes through `.bind(...)` in
//! declaration order, so `$1` is the first declared parameter. The SQL
//! text is a `const` spliced verbatim from the schema and is never
//! formatted, concatenated or substituted into — there is no code path in
//! this file that could place a caller-supplied value into the statement
//! text.

use cratestack_core::{Query, TypeArity};
use quote::quote;

use crate::shared::ident;

/// `run`, plus the `SQL` const it executes.
pub(super) fn generate_query_entry(
    query: &Query,
    element_type: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let sql = query.sql().unwrap_or_default();
    let binds = query.args.iter().map(|arg| {
        let field_ident = ident(&arg.name);
        quote! { .bind(args.#field_ident.clone()) }
    });
    let fetch = match query.result_type.arity {
        TypeArity::List => quote! { fetch_all },
        // `Optional` is rejected by the parser, so `Required` is the only
        // other arity that reaches here; `fetch_one` surfaces "no rows" as
        // `CratestackError::NotFound` through `cratestack_error_from_sqlx`.
        _ => quote! { fetch_one },
    };

    quote! {
        /// The SQL body exactly as written in the schema. Executed
        /// verbatim; parameters are bound, never interpolated.
        pub const SQL: &str = #sql;

        /// Run this query.
        ///
        /// Checks this query's `@allow`/`@deny` policy against `args` and
        /// `ctx` first, and returns `CratestackError::Forbidden` without
        /// touching the database if it does not pass. A query that
        /// declares no `@allow` at all denies everyone — deny-by-default,
        /// the same rule models and procedures follow.
        ///
        /// **The policy gates whether this call is permitted; it does not
        /// filter which rows the SQL matches.** Nothing injects a
        /// `deleted_at IS NULL` predicate or a row-level `@allow` filter
        /// into a `query` body the way `push_scoped_conditions` does for
        /// every generated read. If this query reads a soft-delete-enabled
        /// model's table, deleted rows count toward its results unless the
        /// SQL says otherwise — you own every `WHERE`/`FILTER` predicate
        /// here. See `docs/design/declarative-custom-query.md` §6.
        pub async fn run(
            db: &super::super::Cratestack,
            args: &Args,
            ctx: &::cratestack::CratestackContext,
        ) -> Result<Output, ::cratestack::CratestackError> {
            let started = ::std::time::Instant::now();
            ::cratestack::authorize_procedure(ALLOW_POLICIES, DENY_POLICIES, args, ctx)
                .inspect_err(|error| {
                    ::cratestack::tracing::warn!(
                        target: "cratestack",
                        cratestack_query = NAME,
                        cratestack_operation = "query",
                        cratestack_authenticated = ctx.is_authenticated(),
                        cratestack_error = error.code(),
                        cratestack_duration_ms = started.elapsed().as_millis() as u64,
                        "cratestack query authorization failed",
                    );
                })?;

            let result = ::cratestack::sqlx::query_as::<_, #element_type>(SQL)
                #(#binds)*
                .#fetch(db.pool())
                .await
                .map_err(::cratestack::cratestack_error_from_sqlx);

            match &result {
                Ok(_) => ::cratestack::tracing::debug!(
                    target: "cratestack",
                    cratestack_query = NAME,
                    cratestack_operation = "query",
                    cratestack_authenticated = ctx.is_authenticated(),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    "cratestack query completed",
                ),
                Err(error) => ::cratestack::tracing::warn!(
                    target: "cratestack",
                    cratestack_query = NAME,
                    cratestack_operation = "query",
                    cratestack_authenticated = ctx.is_authenticated(),
                    cratestack_error = error.code(),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    "cratestack query failed",
                ),
            }

            result
        }
    }
}
