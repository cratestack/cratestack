//! `authorize_with_db` and the private `authorize_validated_with_db` it
//! shares with `invoke_with_db`, split out of the parent `instrument` module
//! to keep it under the crate's ~200-LoC file ceiling.
//!
//! Validation and authorization are two functions so that each public helper
//! validates exactly once, first, and the `@isolation` form of
//! `invoke_with_db` can validate before it takes a pooled connection and
//! then authorize inside the attempt's transaction without validating again
//! on every retry (`super::invoke_isolated`). Only the private function has
//! no validation, and nothing outside the generated module can call it.

use quote::quote;

/// `authorize_with_db` validates the arguments' fields, then authorizes:
/// `@allow`/`@deny` and any `@authorize` model checks. This is the one place
/// every transport and every non-HTTP caller passes through to obtain an
/// `Authorized` witness, so the validation is not a per-transport call that
/// one transport can forget (CLAUDE.md, "Transport parity"). It precedes
/// `@allow`, as a model input's `validate` precedes its create policy. Only
/// arguments are validated: a return value is produced by the server, so no
/// call site exists for it.
pub(in crate::procedure) fn authorize_with_db_fn_tokens(
    model_authorizers: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    quote! {
        pub async fn authorize_with_db(
            db: &super::super::Cratestack,
            args: &Args,
            ctx: &::cratestack::CratestackContext,
        ) -> Result<Authorized, ::cratestack::CratestackError> {
            ::cratestack::ProcedureArgs::validate_fields(args)?;
            authorize_validated_with_db(db, args, ctx).await
        }

        /// [`authorize_with_db`] for arguments that already passed
        /// `ProcedureArgs::validate_fields`: `@allow`/`@deny` and any
        /// `@authorize` model checks. Private, so that no caller can obtain
        /// an [`Authorized`] without validating.
        async fn authorize_validated_with_db(
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
