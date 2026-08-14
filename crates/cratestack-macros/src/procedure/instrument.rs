//! The four lifecycle helpers spliced into every generated
//! `pub mod <procedure>`: `authorize`, `authorize_with_db`, `invoke`,
//! `invoke_with_db`. Factored out of the entry to keep
//! [`super::generate_procedure_module`] readable.
//!
//! Each helper wraps the same shape: stamp `Instant::now()`, run the
//! policy check (and any `@authorize` model checks), then `tracing`
//! the result in the standard `cratestack_*` field set.
//!
//! cratestack#512: `authorize_with_db`/`invoke_with_db` are also the
//! only source of an [`authorized_type_tokens`]-generated `Authorized`
//! witness — see that function's doc comment for why that is what
//! makes `registry.<method>(&db, &ctx, args)` (skipping the
//! `ProcedureRegistry` trait method's now-required witness parameter)
//! fail to compile instead of silently skipping every `@allow`. The
//! plain (non-`db`) `authorize`/`invoke` pair below is untouched:
//! `ProcedureRegistry` methods always take `db` (even under `db =
//! None`, where `Cratestack` is just a unit struct — see
//! `include/server/runtime/none.rs`), so nothing ever needs a witness
//! from the `db`-less pair to call one.

use quote::quote;

/// See the module doc's cratestack#512 note. Spliced into every
/// generated `pub mod <procedure>` alongside the four lifecycle
/// helpers, so it shares their module scope — that shared scope, not
/// any `pub` constructor, is the entire enforcement mechanism.
pub(super) fn authorized_type_tokens() -> proc_macro2::TokenStream {
    quote! {
        /// Proof that this procedure's `@allow`/`@deny` policy — and, for
        /// [`authorize_with_db`], any `@authorize` model checks — ran and
        /// passed for a call. [`super::procedures::ProcedureRegistry`]'s
        /// generated method for this procedure takes one of these as its
        /// last argument, which is what makes
        /// `registry.<method>(&db, &ctx, args)` — the shape that used to
        /// skip every `@allow` (cratestack#512) — fail to compile instead
        /// of compiling and silently bypassing policy.
        ///
        /// Enforced by construction, not convention: the tuple field below
        /// has no `pub`, so a value can only be built from code inside
        /// this module — exactly where `authorize_with_db`/`invoke_with_db`
        /// live, and nowhere else in the generated crate (not the `axum`/
        /// `rpc`/`grpc` dispatch modules, not a `ProcedureRegistry`
        /// implementor's own code, not any other procedure's module).
        ///
        /// Two honest limits on what this proves, so it isn't mistaken for
        /// more than it is:
        /// - It proves *a* call to `authorize_with_db` passed, not that
        ///   *this exact* `(ctx, args)` pair did — a caller who
        ///   deliberately stashes a witness and replays it for unrelated
        ///   arguments has gone out of their way to defeat the mechanism,
        ///   the same way holding a raw database connection bypasses
        ///   policy entirely (out of scope for the same reason).
        /// - Like every safe-Rust sealed/witness token, this does not
        ///   defend against `unsafe` code forging a zero-sized value —
        ///   this crate's own `unsafe_code = "forbid"` closes that door
        ///   for the framework's generated code, not for a consumer
        ///   crate that chooses to write `unsafe` itself.
        ///
        /// `Debug` is derived (not `Clone`/`PartialEq`/...) purely so
        /// `Result<Authorized, CratestackError>` satisfies `expect`/`expect_err`
        /// in tests that assert on `authorize_with_db`'s result without
        /// caring about the success value — it does not add any way to
        /// construct one.
        #[derive(Debug)]
        pub struct Authorized(());
    }
}

pub(super) fn authorize_fn_tokens() -> proc_macro2::TokenStream {
    quote! {
        pub fn authorize<A: ::cratestack::ProcedureArgs + ?Sized>(
            args: &A,
            ctx: &::cratestack::CratestackContext,
        ) -> Result<(), ::cratestack::CratestackError> {
            let started = ::std::time::Instant::now();
            let result = ::cratestack::authorize_procedure(ALLOW_POLICIES, DENY_POLICIES, args, ctx);
            match &result {
                Ok(()) => ::cratestack::tracing::debug!(
                    target: "cratestack",
                    cratestack_procedure = NAME,
                    cratestack_operation = "authorize",
                    cratestack_authenticated = ctx.is_authenticated(),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    "cratestack procedure authorized",
                ),
                Err(error) => ::cratestack::tracing::warn!(
                    target: "cratestack",
                    cratestack_procedure = NAME,
                    cratestack_operation = "authorize",
                    cratestack_authenticated = ctx.is_authenticated(),
                    cratestack_error = error.code(),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    "cratestack procedure authorization failed",
                ),
            }
            result
        }
    }
}

pub(super) fn authorize_with_db_fn_tokens(
    model_authorizers: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    quote! {
        pub async fn authorize_with_db(
            db: &super::super::Cratestack,
            args: &Args,
            ctx: &::cratestack::CratestackContext,
        ) -> Result<Authorized, ::cratestack::CratestackError> {
            let started = ::std::time::Instant::now();
            ::cratestack::authorize_procedure(ALLOW_POLICIES, DENY_POLICIES, args, ctx)?;
            #(#model_authorizers)*
            ::cratestack::tracing::debug!(
                target: "cratestack",
                cratestack_procedure = NAME,
                cratestack_operation = "authorize_with_db",
                cratestack_authenticated = ctx.is_authenticated(),
                cratestack_duration_ms = started.elapsed().as_millis() as u64,
                "cratestack procedure db authorization completed",
            );
            Ok(Authorized(()))
        }
    }
}

pub(super) fn invoke_fn_tokens() -> proc_macro2::TokenStream {
    quote! {
        pub async fn invoke<A, F, Fut, T>(
            args: &A,
            ctx: &::cratestack::CratestackContext,
            f: F,
        ) -> Result<T, ::cratestack::CratestackError>
        where
            A: ::cratestack::ProcedureArgs + ?Sized,
            F: FnOnce() -> Fut,
            Fut: ::core::future::Future<Output = Result<T, ::cratestack::CratestackError>>,
        {
            let span = ::cratestack::tracing::info_span!(
                "cratestack_procedure_invoke",
                cratestack_procedure = NAME,
                cratestack_operation = "invoke",
                cratestack_authenticated = ctx.is_authenticated(),
            );
            let _guard = span.enter();
            let started = ::std::time::Instant::now();
            ::cratestack::authorize_procedure(ALLOW_POLICIES, DENY_POLICIES, args, ctx)?;
            let result = f().await;
            match &result {
                Ok(_) => ::cratestack::tracing::info!(
                    target: "cratestack",
                    cratestack_procedure = NAME,
                    cratestack_operation = "invoke",
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    "cratestack procedure completed",
                ),
                Err(error) => ::cratestack::tracing::warn!(
                    target: "cratestack",
                    cratestack_procedure = NAME,
                    cratestack_operation = "invoke",
                    cratestack_error = error.code(),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    "cratestack procedure failed",
                ),
            }
            result
        }
    }
}

pub(super) fn invoke_with_db_fn_tokens() -> proc_macro2::TokenStream {
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
        /// the generated axum/RPC/gRPC handlers call this same function,
        /// nothing more privileged is available to them.
        ///
        /// ```ignore
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
