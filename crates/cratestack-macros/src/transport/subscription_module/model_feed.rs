//! One `@@subscribe` model's open function inside the generated
//! `subscriptions` module.
//!
//! Every event passes the subscriber's read policy before it is sent: the
//! model's `detail` and `read` rules, the ones `get` applies. An event has
//! no query to push them into, so they are decided in-process against the
//! event's record (`cratestack_sqlx::record_readable`; a relation predicate
//! still probes the database), and an event the subscriber may not read is
//! dropped. The record lacks its `@server_only` columns, and carries only
//! the columns a policy can compare ([`policy_comparable`]); a policy that
//! reads any other column cannot be decided, so the subscription is refused
//! when it opens (500, the columns named in the server log) rather than
//! decided on a missing value.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, TypeArity};
use quote::quote;

use crate::event::model_emitted_events;
use crate::shared::{
    ident, is_server_only_field, scalar_model_fields, sql_value_tokens, to_snake_case,
};

pub(super) struct ModelFeed {
    pub(super) op_id: String,
    pub(super) open_ident: proc_macro2::Ident,
    pub(super) function: proc_macro2::TokenStream,
}

pub(super) fn generate(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> Result<Option<ModelFeed>, String> {
    if !cratestack_core::model_verbs(model).contains(&cratestack_core::ModelVerb::Subscribe) {
        return Ok(None);
    }
    let emitted = model_emitted_events(model).map_err(|error| {
        format!(
            "failed to parse emitted events for `{}`: {error}",
            model.name
        )
    })?;
    let model_name = model.name.as_str();
    let model_snake = to_snake_case(model_name);
    let event_alias = ident(&format!("{model_name}CreatedEvent"));
    let op_id = cratestack_core::ModelVerb::Subscribe.rpc_op_id(model_name);
    let open_ident = ident(&format!("open_{model_snake}"));
    let descriptor = ident(&format!("{}_MODEL", model_snake.to_uppercase()));
    let policy_fields: Vec<&Field> = scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !is_server_only_field(field) && policy_comparable(field, enum_names))
        .collect();
    let policy_columns = policy_fields.iter().map(|field| to_snake_case(&field.name));
    let policy_values = policy_fields.iter().map(|field| {
        let column = to_snake_case(&field.name);
        let field_ident = ident(&field.name);
        let value = sql_value_tokens(
            quote! { record.#field_ident.clone() },
            &field.ty,
            enum_names,
        );
        quote! { ::cratestack::SqlColumnValue { column: #column, value: #value } }
    });
    let registrations = emitted.iter().map(|operation| {
        let method_ident = ident(&format!("on_{model_snake}_{}", operation.as_str()));
        quote! {
            {
                let push = push.clone();
                let handle = events.#method_ident(move |event| {
                    push.push(event);
                    ::core::future::ready(Ok(()))
                });
                guard.track(handle);
            }
        }
    });

    let function = quote! {
        fn #open_ident(
            db: &super::Cratestack,
            input: ::cratestack::serde_json::Value,
            ctx: ::cratestack::CratestackContext,
        ) -> Result<
            ::cratestack::SubscriptionStream<::cratestack::serde_json::Value>,
            ::cratestack::CratestackError,
        > {
            no_input(#op_id, &input)?;
            let descriptor = &super::models::#descriptor;
            let (allow, deny) = (descriptor.detail_allow_policies, descriptor.detail_deny_policies);
            let missing = ::cratestack::__private::missing_policy_columns(allow, deny, &[#(#policy_columns),*]);
            if !missing.is_empty() {
                ::cratestack::tracing::error!(
                    target: "cratestack",
                    cratestack_op_id = #op_id,
                    columns = ?missing,
                    "subscription refused: the read policy reads columns a model event does not carry",
                );
                return Err(::cratestack::CratestackError::Internal(format!(
                    "the read policy of `{}` cannot be decided on a model event (columns {missing:?})",
                    #model_name,
                )));
            }
            let events = db.events();
            let mut guard = ::cratestack::SubscriptionGuard::new(events.__event_bus());
            let (push, rx) =
                ::cratestack::__private::subscription_channel::<super::events::#event_alias>();
            #(#registrations)*
            drop(push);
            let stream = ::cratestack::__private::guarded_receiver_stream(rx, guard);
            let pool = db.pool().clone();
            Ok(::cratestack::__private::model_feed(
                stream,
                move |event: &super::events::#event_alias| {
                    let record = &event.data;
                    let values = vec![#(#policy_values),*];
                    let (pool, ctx) = (pool.clone(), ctx.clone());
                    async move {
                        ::cratestack::__private::record_readable(&pool, allow, deny, &values, &ctx).await
                    }
                },
            ))
        }
    };
    Ok(Some(ModelFeed {
        op_id,
        open_ident,
        function,
    }))
}

/// The fields a read policy can compare: with a literal or an `auth()`
/// claim (`String`, `Int`, `BigInt`, `Boolean`, an enum), or across a
/// relation join (those and `Uuid`, `Cuid`). Only these go into the
/// record a policy is decided on; a policy over any other column is
/// refused at open by `missing_policy_columns`.
fn policy_comparable(field: &Field, enum_names: &BTreeSet<&str>) -> bool {
    !matches!(field.ty.arity, TypeArity::List)
        && (enum_names.contains(field.ty.name.as_str())
            || matches!(
                field.ty.name.as_str(),
                "String" | "Cuid" | "Int" | "BigInt" | "Boolean" | "Uuid"
            ))
}
