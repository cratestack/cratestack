//! The four lifecycle helpers spliced into every generated
//! `pub mod <procedure>`: `authorize`, `authorize_with_db`, `invoke`,
//! `invoke_with_db`. Factored out of the entry to keep
//! [`super::generate_procedure_module`] readable.
//!
//! Each helper wraps the same shape: stamp `Instant::now()`, run the
//! policy check (and any `@authorize` model checks), then `tracing`
//! the result in the standard `cratestack_*` field set.

use quote::quote;

pub(super) fn authorize_fn_tokens() -> proc_macro2::TokenStream {
    quote! {
        pub fn authorize<A: ::cratestack::ProcedureArgs + ?Sized>(
            args: &A,
            ctx: &::cratestack::CoolContext,
        ) -> Result<(), ::cratestack::CoolError> {
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
            ctx: &::cratestack::CoolContext,
        ) -> Result<(), ::cratestack::CoolError> {
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
            Ok(())
        }
    }
}

pub(super) fn invoke_fn_tokens() -> proc_macro2::TokenStream {
    quote! {
        pub async fn invoke<A, F, Fut, T>(
            args: &A,
            ctx: &::cratestack::CoolContext,
            f: F,
        ) -> Result<T, ::cratestack::CoolError>
        where
            A: ::cratestack::ProcedureArgs + ?Sized,
            F: FnOnce() -> Fut,
            Fut: ::core::future::Future<Output = Result<T, ::cratestack::CoolError>>,
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
        pub async fn invoke_with_db<F, Fut, T>(
            db: &super::super::Cratestack,
            args: &Args,
            ctx: &::cratestack::CoolContext,
            f: F,
        ) -> Result<T, ::cratestack::CoolError>
        where
            F: FnOnce() -> Fut,
            Fut: ::core::future::Future<Output = Result<T, ::cratestack::CoolError>>,
        {
            let span = ::cratestack::tracing::info_span!(
                "cratestack_procedure_invoke_with_db",
                cratestack_procedure = NAME,
                cratestack_operation = "invoke_with_db",
                cratestack_authenticated = ctx.is_authenticated(),
            );
            let _guard = span.enter();
            let started = ::std::time::Instant::now();
            authorize_with_db(db, args, ctx).await?;
            let result = f().await;
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
