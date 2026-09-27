//! `invoke_with_db` for a procedure that declares `@isolation(...)`
//! (docs/design/procedure-isolation.md §2, §5; GHSA-r67q-4qqq-g9gm).
//!
//! Same name and the same role as [`super::invoke_with_db_fn_tokens`] —
//! the one function every transport's dispatch (REST, RPC, RPC batch, MCP)
//! and every non-HTTP caller goes through — but a different shape: instead
//! of handing the caller's closure an `Authorized` witness and letting it
//! reach the registry with whatever `db` it closed over, it runs
//! authorization *and* the closure inside
//! `SqlxRuntime::run_isolated`, and hands the closure the
//! `IsolatedCratestack` bound to that transaction. The closure is cloned
//! and called once per attempt, because a serialization failure re-runs
//! it; it takes the handle by value so the future it returns can own it
//! (a closure over a *borrowed* handle would need a higher-ranked `Send`
//! bound the compiler cannot prove for an axum handler's future).

use quote::quote;

use crate::shared::{isolation_tokens, procedure_isolation};

/// A procedure module's `ISOLATION` const and its `invoke_with_db`: for an
/// `@isolation` procedure the const and the transaction-running function
/// below; otherwise no const and exactly the tokens every procedure had
/// before GHSA-r67q-4qqq-g9gm.
pub(in crate::procedure) fn isolation_and_invoke_with_db_tokens(
    procedure: &cratestack_core::Procedure,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let Some(level) = procedure_isolation(procedure) else {
        return (quote! {}, super::invoke_with_db_fn_tokens());
    };
    let level = isolation_tokens(level);
    let isolation_const = quote! {
        /// The `@isolation(...)` level this procedure runs at.
        pub const ISOLATION: ::cratestack::TransactionIsolation = #level;
    };
    (isolation_const, invoke_with_db_isolated_fn_tokens())
}

fn invoke_with_db_isolated_fn_tokens() -> proc_macro2::TokenStream {
    quote! {
        /// Runs this procedure's authorization (`@allow`/`@deny` and any
        /// `@authorize` model check) and then `f` inside one database
        /// transaction at [`ISOLATION`], retrying both on a serialization
        /// failure or deadlock (SQLSTATE `40001`/`40P01`), and commits when
        /// `f` returns `Ok`. `f` receives the
        /// [`super::super::IsolatedCratestack`] bound to that transaction —
        /// the only database handle an `@isolation` procedure's
        /// [`super::ProcedureRegistry`] method accepts — and the
        /// [`Authorized`] witness for this attempt.
        ///
        /// `f` may run more than once. Everything it does through the handle
        /// is rolled back with a failed attempt; anything else it does is
        /// not. Retries exhausted: `409 TRANSACTION_ABORTED`. See
        /// docs/design/procedure-isolation.md.
        ///
        /// ```text
        /// let args = procedures::withdraw::Args { .. };
        /// let (call_args, call_ctx) = (args.clone(), ctx.clone());
        /// let result = procedures::withdraw::invoke_with_db(
        ///     &db,
        ///     &args,
        ///     &ctx,
        ///     move |tx_db, authorized| async move {
        ///         registry.withdraw(&tx_db, &call_ctx, call_args, authorized).await
        ///     },
        /// )
        /// .await;
        /// ```
        pub async fn invoke_with_db<F, Fut, T>(
            db: &super::super::Cratestack,
            args: &Args,
            ctx: &::cratestack::CratestackContext,
            f: F,
        ) -> Result<T, ::cratestack::CratestackError>
        where
            F: FnOnce(super::super::IsolatedCratestack, Authorized) -> Fut + Clone,
            Fut: ::core::future::Future<Output = Result<T, ::cratestack::CratestackError>>,
        {
            let span = ::cratestack::tracing::info_span!(
                "cratestack_procedure_invoke_with_db",
                cratestack_procedure = NAME,
                cratestack_operation = "invoke_with_db",
                cratestack_authenticated = ctx.is_authenticated(),
                cratestack_isolation = ISOLATION.as_sql(),
            );
            let _guard = span.enter();
            let started = ::std::time::Instant::now();
            let result = db
                .runtime
                .run_isolated(ISOLATION, |runtime| {
                    let f = f.clone();
                    async move {
                        let tx_db = super::super::IsolatedCratestack {
                            inner: super::super::Cratestack { runtime },
                        };
                        let authorized = authorize_with_db(&tx_db.inner, args, ctx).await?;
                        f(tx_db, authorized).await
                    }
                })
                .await;
            match &result {
                Ok(_) => ::cratestack::tracing::info!(
                    target: "cratestack",
                    cratestack_procedure = NAME,
                    cratestack_operation = "invoke_with_db",
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    "cratestack procedure completed",
                ),
                Err(error) => ::cratestack::tracing::warn!(
                    target: "cratestack",
                    cratestack_procedure = NAME,
                    cratestack_operation = "invoke_with_db",
                    cratestack_error = error.code(),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    "cratestack procedure failed",
                ),
            }
            result
        }
    }
}
