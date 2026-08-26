//! REST per-model client struct: `<Model>Client` with list / get /
//! create / update / delete (plus `*_view` projection variants on
//! list/get). Paged models return `Page<Model>`; non-paged return
//! `Vec<Model>`.
//!
//! cratestack#743 (`docs/design/route-suppression.md`): a suppressed
//! verb (`@@internal(...)`) gets no client method at all — calling it
//! becomes a compile error for the SDK consumer, not a runtime 403
//! (design doc §4/§5). `cratestack_core::model_internal_actions`, the
//! one shared source of truth, is consulted once below; each verb
//! group's own builder lives in [`groups_read`]/[`groups_write`]
//! (split the same way `transport::rpc::model_dispatch` splits into
//! `arms_read`/`arms_write`, for the 200-LoC file convention).

use std::collections::BTreeSet;

use cratestack_core::Model;
use quote::quote;

use crate::client::model_output_type_tokens;
use crate::shared::{
    ident, is_paged_model, is_primary_key, pluralize, rust_type_tokens, to_snake_case,
};

mod computed;
mod context;
mod groups_read;
mod groups_write;
mod with_response;
use context::ModelRestClientContext;

pub(super) fn generate_generated_model_client(
    model: &Model,
    bearing: &BTreeSet<String>,
    computed_params_ident: Option<&syn::Ident>,
) -> Result<proc_macro2::TokenStream, String> {
    let internal = cratestack_core::model_internal_actions(model);
    let client_ident = ident(&format!("{}Client", model.name));
    let route_path = format!("/{}", pluralize(&to_snake_case(&model.name)));
    let paged = is_paged_model(model);
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .ok_or_else(|| format!("model {} is missing a primary key", model.name))?;
    let primary_key_type = rust_type_tokens(&primary_key.ty);
    let model_output_type = model_output_type_tokens(&model.name, bearing);
    let list_output_type = if paged {
        quote! { ::cratestack::Page<#model_output_type> }
    } else {
        quote! { Vec<#model_output_type> }
    };
    let list_view_output_type = if paged {
        quote! { ::cratestack::Page<P::Output> }
    } else {
        quote! { Vec<P::Output> }
    };
    let list_view_call = if paged {
        quote! {
            self.runtime
                .list_view_paged(#route_path, projection, query, headers)
                .await
        }
    } else {
        quote! {
            self.runtime
                .list_view(#route_path, projection, query, headers)
                .await
        }
    };

    // `computed_params_ident` gates both `list` and `get` on whether this
    // model declares at least one parameterized `@computed` field
    // (`crate::client::computed_params::model_computed_params_ident`) —
    // an ungated model keeps the exact tokens this function emitted
    // before this feature existed (see `model/computed.rs`'s doc for
    // why).
    let ctx = ModelRestClientContext {
        route_path,
        primary_key_type,
        model_output_type,
        list_output_type,
        list_view_output_type,
        list_view_call,
        create_input_ident: ident(&format!("Create{}Input", model.name)),
        update_input_ident: ident(&format!("Update{}Input", model.name)),
        computed_params_ident: computed_params_ident.cloned(),
    };

    // Each verb group below is emitted only when `!internal.contains(...)`
    // — cratestack#743's REST client gate. `list`/`get` also cover their
    // `*_view` projection siblings, since both hit the same suppressed
    // route.
    let list_group = if !internal.contains("list") {
        groups_read::list_group(&ctx)
    } else {
        Default::default()
    };
    let get_group = if !internal.contains("get") {
        groups_read::get_group(&ctx)
    } else {
        Default::default()
    };
    let create_group = if !internal.contains("create") {
        groups_write::create_group(&ctx)
    } else {
        Default::default()
    };
    let update_group = if !internal.contains("update") {
        groups_write::update_group(&ctx)
    } else {
        Default::default()
    };
    let delete_group = if !internal.contains("delete") {
        groups_write::delete_group(&ctx)
    } else {
        Default::default()
    };

    Ok(quote! {
        #[derive(Clone)]
        pub struct #client_ident<C = ::cratestack::client_rust::CborCodec>
        where
            C: ::cratestack::client_rust::HttpClientCodec,
        {
            runtime: ::cratestack::client_rust::CratestackClient<C>,
        }

        impl<C> #client_ident<C>
        where
            C: ::cratestack::client_rust::HttpClientCodec,
        {
            fn new(runtime: ::cratestack::client_rust::CratestackClient<C>) -> Self {
                Self { runtime }
            }

            #list_group

            #get_group

            #create_group

            #update_group

            #delete_group
        }
    })
}
