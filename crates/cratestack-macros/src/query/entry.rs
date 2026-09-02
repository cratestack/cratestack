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
//! **The second half of that argument is the `READ ONLY` transaction**
//! ([`super::read_only`]), added after cratestack#870's review measured a
//! `query` body *writing*. That module's doc has the reasoning; the short
//! version is that `@allow` gates the call while the write bypasses every
//! guarantee the framework's own write path provides, and that Postgres —
//! not a SQL-text check — is what refuses it.
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

use super::read_only::{query_error_fn, read_only_execution};

/// `run`, plus the `SQL` const it executes.
pub(super) fn generate_query_entry(
    query: &Query,
    element_type: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let sql = query.sql().unwrap_or_default();
    let binds = query
        .args
        .iter()
        .map(|arg| {
            let field_ident = ident(&arg.name);
            quote! { .bind(args.#field_ident.clone()) }
        })
        .collect::<Vec<_>>();
    let fetch = match query.result_type.arity {
        TypeArity::List => quote! { fetch_all },
        // `Optional` is rejected by the parser, so `Required` is the only
        // other arity that reaches here; `fetch_one` surfaces "no rows" as
        // `CratestackError::NotFound` through `cratestack_error_from_sqlx`.
        _ => quote! { fetch_one },
    };

    let execution = read_only_execution(element_type, &fetch, &binds);
    let query_error_fn = query_error_fn();

    quote! {
        /// The SQL body exactly as written in the schema. Executed
        /// verbatim; parameters are bound, never interpolated.
        pub const SQL: &str = #sql;

        /// Run this query.
        ///
        /// # What is checked before any SQL runs
        ///
        /// This query's `@allow`/`@deny` policy, against `args` and `ctx`.
        /// Returns `CratestackError::Forbidden` without touching the
        /// database if it does not pass. A query that declares no
        /// `@allow` at all denies everyone — deny-by-default, the same
        /// rule models and procedures follow.
        ///
        /// # Reads only, enforced by the database
        ///
        /// The statement runs inside a Postgres `READ ONLY` transaction,
        /// so `INSERT`/`UPDATE`/`DELETE`/`TRUNCATE` and DDL are refused by
        /// the engine (SQLSTATE `25006`) — including when they are hidden
        /// inside a data-modifying CTE such as
        /// `WITH ins AS (INSERT … RETURNING …) SELECT …`, which is an
        /// ordinary `SELECT` as far as the driver is concerned.
        ///
        /// This is not stylistic. A write reaching the database through a
        /// `query` would bypass `@@audit` rows, the `@@emit` outbox,
        /// `@version` optimistic locking, soft-delete, `@@internal`
        /// suppression and the target model's own write `@@allow` — none
        /// of which a policy expression on this query can compensate for.
        /// Use a `procedure` (or a model write builder) for anything that
        /// changes data.
        ///
        /// # The policy does not filter rows
        ///
        /// It gates *whether this call is permitted*. Nothing injects a
        /// `deleted_at IS NULL` predicate or a row-level `@allow` filter
        /// into a `query` body the way `push_scoped_conditions` does for
        /// every generated read. If this query reads a soft-delete-enabled
        /// model's table, deleted rows count toward its results unless the
        /// SQL says otherwise — you own every `WHERE`/`FILTER` predicate
        /// here. See `docs/design/declarative-custom-query.md` §6.
        ///
        /// # It runs on its own connection
        ///
        /// The transaction above is opened on a connection taken from the
        /// pool, which is **not** the connection an enclosing
        /// `Cratestack::transaction(...)` is using. A query called from
        /// inside that closure therefore cannot see writes the closure has
        /// made but not yet committed — it will observe the pre-transaction
        /// state and, for a `fetch_one` query, may return
        /// `CratestackError::NotFound`. Read after the transaction
        /// commits. Composing the two is not supported in v1 and would be
        /// contradictory anyway: this transaction is `READ ONLY` and the
        /// enclosing one is not.
        ///
        /// It also **takes a second connection** for the duration. On a
        /// pool with no free slot that is not a stale read but a stall:
        /// the acquire blocks for `acquire_timeout` and then fails with
        /// "pool timed out while waiting for an open connection". Calling
        /// a query from inside a transaction on a small pool is therefore
        /// a deadlock risk, not just an isolation surprise.
        pub async fn run(
            db: &super::super::Cratestack,
            args: &Args,
            ctx: &::cratestack::CratestackContext,
        ) -> Result<Output, ::cratestack::CratestackError> {
            use ::cratestack::sqlx::Acquire as _;

            let started = ::std::time::Instant::now();
            ::cratestack::authorize_query(ALLOW_POLICIES, DENY_POLICIES, args, ctx)
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

            let result = #execution.await;

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

        #query_error_fn
    }
}
