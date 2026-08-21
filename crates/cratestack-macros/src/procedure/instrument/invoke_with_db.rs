//! [`invoke_with_db_fn_tokens`], split out of the parent `instrument`
//! module purely to keep it under the crate's ~200-LoC file ceiling —
//! its generated doc comment (see cratestack#611 below) makes it the
//! single largest of the four lifecycle helpers.

use quote::quote;

pub(in crate::procedure) fn invoke_with_db_fn_tokens() -> proc_macro2::TokenStream {
    quote! {
        /// Runs `@allow`/`@deny` (and any `@authorize` model checks) via
        /// [`authorize_with_db`], then calls `f` with the resulting
        /// [`Authorized`] witness. `f` is the only place that witness can
        /// go: into the [`super::procedures::ProcedureRegistry`] method
        /// call this procedure's generated dispatch handler makes — see
        /// cratestack#512.
        ///
        /// This is also the sanctioned way to invoke a procedure from
        /// non-HTTP code (a cron job, background worker, or admin tool):
        /// the generated axum/RPC handlers call this same function,
        /// nothing more privileged is available to them.
        ///
        /// cratestack#611: this example is illustrative pseudocode — it
        /// references a procedure (`reconcile_accounts`) and free
        /// variables (`db`, `registry`, `ctx`) that don't resolve at a
        /// real crate root, so it was never meant to compile. It's
        /// fenced ```` ```text ```` rather than left unfenced-and-indented
        /// (the shape actually proposed in cratestack#611) or fenced
        /// ```` ```ignore ````: an *unfenced* indented block is still a
        /// CommonMark code block, so rustdoc still schedules it as a
        /// doctest and it still fails to compile under a plain `cargo
        /// test` — strictly worse, not fixed. ```` ```ignore ```` compiles
        /// correctly under plain `cargo test` (shown "ignored"), but
        /// `cargo test -- --ignored` reuses that exact bucket to force-run
        /// every `#[ignore]`d test *and* every ```` ```ignore ```` doctest,
        /// with no way to opt out — one failure per procedure the moment a
        /// downstream crate's own CI convention includes `-- --ignored`
        /// anywhere. `text` is a language rustdoc doesn't recognize as
        /// Rust, so it's never scheduled as a doctest candidate under any
        /// flag combination, while still rendering as a fenced, monospaced
        /// example in `cargo doc` output.
        ///
        /// ```text
        /// // Internal caller shape — a background worker running as the
        /// // system principal (`auth().isSystem()`, cratestack#486).
        /// let ctx = SystemContext::for_service("nightly-reconciler").into_context();
        /// let args = procedures::reconcile_accounts::Args { .. };
        /// let result = procedures::reconcile_accounts::invoke_with_db(
        ///     &db,
        ///     &args,
        ///     &ctx,
        ///     |authorized| async move {
        ///         registry.reconcile_accounts(&db, &ctx, args, authorized).await
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
            F: FnOnce(Authorized) -> Fut,
            Fut: ::core::future::Future<Output = Result<T, ::cratestack::CratestackError>>,
        {
            let span = ::cratestack::tracing::info_span!(
                "cratestack_procedure_invoke_with_db",
                cratestack_procedure = NAME,
                cratestack_operation = "invoke_with_db",
                cratestack_authenticated = ctx.is_authenticated(),
            );
            let _guard = span.enter();
            let started = ::std::time::Instant::now();
            let authorized = authorize_with_db(db, args, ctx).await?;
            let result = f(authorized).await;
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
