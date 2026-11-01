//! One `subscription procedure`'s open function inside the generated
//! `subscriptions` module (`docs/design/subscriptions-websocket.md` §3.2,
//! §3.3, slice S4).
//!
//! The arguments come from the query string (`procedure_query_args`); the
//! procedure's `authorize_with_db` runs its field validators, `@allow`/
//! `@deny`, `@authn` and `@authorize` checks and returns the `Authorized`
//! witness the registry method requires (cratestack#512); then the handler's
//! setup runs, and fails before `Ready` as an ordinary error. Its stream is
//! the subscription, ended when the opening credential expires.

use cratestack_core::Procedure;
use quote::quote;

use crate::shared::{ident, to_snake_case};

pub(super) struct ProcedureFeed {
    pub(super) op_id: String,
    pub(super) open_ident: proc_macro2::Ident,
    pub(super) function: proc_macro2::TokenStream,
}

pub(super) fn generate(procedure: &Procedure) -> ProcedureFeed {
    let snake = to_snake_case(&procedure.name);
    let module = ident(&snake);
    let method = ident(&snake);
    let open_ident = ident(&format!("open_procedure_{snake}"));
    let function = quote! {
        pub(super) async fn #open_ident<R: super::procedures::ProcedureRegistry>(
            db: &super::Cratestack,
            registry: &R,
            query: Option<String>,
            ctx: ::cratestack::CratestackContext,
        ) -> Result<
            ::cratestack::SubscriptionStream<::cratestack::serde_json::Value>,
            ::cratestack::CratestackError,
        > {
            let args: super::procedures::#module::Args =
                ::cratestack::__private::procedure_query_args(query.as_deref())?;
            let authorized = super::procedures::#module::authorize_with_db(db, &args, &ctx).await?;
            let expires_at = ctx.expires_at();
            let items = super::procedures::ProcedureRegistry::#method(
                registry, db, &ctx, args, authorized,
            )
            .await?;
            Ok(::cratestack::__private::until_credential_expires(
                ::cratestack::__private::procedure_feed(items),
                expires_at,
            ))
        }
    };
    ProcedureFeed {
        op_id: cratestack_core::subscription_procedure_op_key(procedure, true),
        open_ident,
        function,
    }
}
