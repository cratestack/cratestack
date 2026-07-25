//! `transport grpc` server codegen (ticket #171,
//! `docs/design/protobuf.md` §7). Orchestrates:
//!
//! 1. [`lock`] — read + validate the committed `<schema>.pb.lock`.
//! 2. [`message`]/[`update_message`] — `pb::{Model,TypeDecl,Create<M>Input,
//!    Update<M>Input}` mirror structs + `From`/`TryFrom` domain
//!    conversions.
//! 3. [`rpc_inputs`]/[`rpc_list`] — the gRPC-only request/response wrapper
//!    messages (`<Model>RpcPkInput`, `<Model>RpcUpdateInput`,
//!    `<Model>RpcListInput`, `StringList`, `RpcListPredicate`,
//!    `PageOf<Model>`, `PageInfo`).
//! 4. [`service`] — the hand-rolled tonic service (`tower::Service` +
//!    `NamedService`) whose method bodies decode a pb request, call the
//!    same `super::axum::handle_*_dispatch` fn REST/RPC already call (via
//!    `cratestack_axum::rpc::bridge_grpc_response` — see `service.rs`),
//!    and encode the pb response. "No second dispatch path" (ticket #171
//!    AC): every gRPC method delegates to the identical dispatch fn, so
//!    policy/audit/idempotency behavior is unchanged from REST/RPC.
//!
//! Emitted only for `transport grpc` schemas, and only spliced into
//! `compose_server_schema`'s output when `cfg!(feature = "grpc")` is on —
//! `super::super::reject_grpc::guard_server_grpc_transport` already
//! rejected the schema with a `compile_error!` before this module runs
//! otherwise, so everything below can assume the feature is live.
//!
//! **Scope note (ticket #171, judgment call):** this pass covers model
//! CRUD (`list`/`get`/`create`/`update`/`delete`) end to end, including
//! server streaming plumbing shared with procedures. `transport grpc`
//! *procedures* are not yet wired into the generated service — a model
//! with no primary key or with `create` policy-gated off already narrows
//! correctly (mirroring `cratestack-proto::emit::service`'s own gating),
//! but a schema with `procedure` declarations gets no gRPC method for
//! them yet. Tracked as follow-up rather than attempted here given the
//! ticket's own risk register ("may need splitting once ticket 3 lands
//! and the shape is concrete").

mod fields;
mod lock;
mod message;
mod rpc_inputs;
mod rpc_list;
mod scalar;
mod service;
mod update_message;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use cratestack_core::{Schema, TransportStyle};
use proc_macro::TokenStream;
use quote::quote;
use syn::LitStr;

use crate::shared::{
    is_generated_on_create, is_primary_key, is_readonly_field, is_server_only_field,
    is_version_field,
};
use fields::{model_allows_create, scalar_model_fields, visible_model_fields, visible_type_fields};

pub(super) fn build_grpc_module(
    schema: &Schema,
    schema_resolved: &Path,
    schema_path: &LitStr,
) -> Result<proc_macro2::TokenStream, TokenStream> {
    if schema.transport != TransportStyle::Grpc {
        return Ok(quote! {});
    }

    let extra_messages = cratestack_proto::synthesize_messages(schema)
        .map_err(|error| super::collect::compile_error(schema_path, error.to_string()))?;
    let pb_lock = lock::load_pb_lock(schema, schema_resolved, &extra_messages)
        .map_err(|error| super::collect::compile_error(schema_path, error))?;

    let model_names: BTreeSet<&str> = schema.models.iter().map(|m| m.name.as_str()).collect();
    let enum_names: BTreeSet<&str> = schema.enums.iter().map(|e| e.name.as_str()).collect();

    let mut pb_items = Vec::new();

    for model in &schema.models {
        let numbers = numbers_for(&pb_lock, &model.name)
            .map_err(|error| super::collect::compile_error(schema_path, error))?;
        let fields = visible_model_fields(model);
        let domain_path = {
            let ident = crate::shared::ident(&model.name);
            quote! { super::super::#ident }
        };
        let rendered =
            message::render_message(&model.name, domain_path, &fields, numbers, &enum_names)
                .map_err(|error| super::collect::compile_error(schema_path, error))?;
        pb_items.push(rendered.tokens);

        if model_allows_create(model) {
            let create_name = format!("Create{}Input", model.name);
            let create_numbers = numbers_for(&pb_lock, &create_name)
                .map_err(|error| super::collect::compile_error(schema_path, error))?;
            let create_fields = scalar_model_fields(model, &model_names)
                .into_iter()
                .filter(|field| !is_generated_on_create(field))
                .collect::<Vec<_>>();
            let create_ident = crate::shared::ident(&create_name);
            let rendered = message::render_message(
                &create_name,
                quote! { super::super::#create_ident },
                &create_fields,
                create_numbers,
                &enum_names,
            )
            .map_err(|error| super::collect::compile_error(schema_path, error))?;
            pb_items.push(rendered.tokens);
        }

        let update_name = format!("Update{}Input", model.name);
        let update_numbers = numbers_for(&pb_lock, &update_name)
            .map_err(|error| super::collect::compile_error(schema_path, error))?;
        let update_fields = update_input_fields(model, &model_names);
        let update_ident = crate::shared::ident(&update_name);
        let rendered = update_message::render_update_message(
            &update_name,
            quote! { super::super::#update_ident },
            &update_fields,
            update_numbers,
            &enum_names,
        )
        .map_err(|error| super::collect::compile_error(schema_path, error))?;
        pb_items.push(rendered.tokens);
    }

    for ty in &schema.types {
        let numbers = numbers_for(&pb_lock, &ty.name)
            .map_err(|error| super::collect::compile_error(schema_path, error))?;
        let fields = visible_type_fields(ty);
        let ty_ident = crate::shared::ident(&ty.name);
        let rendered = message::render_message(
            &ty.name,
            quote! { super::super::#ty_ident },
            &fields,
            numbers,
            &enum_names,
        )
        .map_err(|error| super::collect::compile_error(schema_path, error))?;
        pb_items.push(rendered.tokens);
    }

    let models_with_pk: Vec<(&cratestack_core::Model, &cratestack_core::Field)> = schema
        .models
        .iter()
        .filter_map(|model| {
            model
                .fields
                .iter()
                .find(|field| is_primary_key(field))
                .map(|pk| (model, pk))
        })
        .collect();

    if !models_with_pk.is_empty() {
        let string_list_numbers = numbers_for(&pb_lock, "StringList")
            .map_err(|error| super::collect::compile_error(schema_path, error))?;
        pb_items.push(
            rpc_inputs::render_string_list(string_list_numbers)
                .map_err(|error| super::collect::compile_error(schema_path, error))?,
        );
        let predicate_numbers = numbers_for(&pb_lock, "RpcListPredicate")
            .map_err(|error| super::collect::compile_error(schema_path, error))?;
        pb_items.push(
            rpc_inputs::render_rpc_list_predicate(predicate_numbers)
                .map_err(|error| super::collect::compile_error(schema_path, error))?,
        );
        let page_info_numbers = numbers_for(&pb_lock, "PageInfo")
            .map_err(|error| super::collect::compile_error(schema_path, error))?;
        pb_items.push(
            rpc_inputs::render_page_info(page_info_numbers)
                .map_err(|error| super::collect::compile_error(schema_path, error))?,
        );

        for (model, pk) in &models_with_pk {
            let pk_numbers = numbers_for(&pb_lock, &format!("{}RpcPkInput", model.name))
                .map_err(|error| super::collect::compile_error(schema_path, error))?;
            pb_items.push(
                rpc_inputs::render_rpc_pk_input(&model.name, pk, pk_numbers)
                    .map_err(|error| super::collect::compile_error(schema_path, error))?,
            );
            let update_numbers = numbers_for(&pb_lock, &format!("{}RpcUpdateInput", model.name))
                .map_err(|error| super::collect::compile_error(schema_path, error))?;
            pb_items.push(
                rpc_inputs::render_rpc_update_input(&model.name, pk, update_numbers)
                    .map_err(|error| super::collect::compile_error(schema_path, error))?,
            );
            let list_numbers = numbers_for(&pb_lock, &format!("{}RpcListInput", model.name))
                .map_err(|error| super::collect::compile_error(schema_path, error))?;
            pb_items.push(
                rpc_list::render_rpc_list_input(&model.name, list_numbers)
                    .map_err(|error| super::collect::compile_error(schema_path, error))?,
            );
            let page_numbers = numbers_for(&pb_lock, &format!("PageOf{}", model.name))
                .map_err(|error| super::collect::compile_error(schema_path, error))?;
            pb_items.push(
                rpc_list::render_page_of(&model.name, page_numbers)
                    .map_err(|error| super::collect::compile_error(schema_path, error))?,
            );
        }
    }

    let package = pb_lock.package.clone().unwrap_or_default();
    let service_tokens = service::build_service(schema, &package, &models_with_pk);

    Ok(quote! {
        pub mod grpc {
            pub mod pb {
                #(#pb_items)*
            }
            #service_tokens
        }
    })
}

fn numbers_for<'a>(
    lock: &'a cratestack_proto::PbLock,
    message: &str,
) -> Result<&'a BTreeMap<String, i32>, String> {
    lock.messages
        .get(message)
        .map(|entry| &entry.fields)
        .ok_or_else(|| format!("no `.pb.lock` entry for message `{message}`"))
}

/// Mirrors `crate::model::inputs::update_input_fields` (PK/`@readonly`/
/// `@server_only`/`@version` excluded) — see `update_message.rs`'s module
/// doc for why this must match the *domain* `Update<M>Input` struct's
/// field set exactly, not `cratestack-proto`'s own (slightly broader)
/// `Update<M>Input` lock entry.
fn update_input_fields<'a>(
    model: &'a cratestack_core::Model,
    model_names: &BTreeSet<&str>,
) -> Vec<&'a cratestack_core::Field> {
    scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !is_primary_key(field))
        .filter(|field| !is_readonly_field(field))
        .filter(|field| !is_server_only_field(field))
        .filter(|field| !is_version_field(field))
        .collect()
}
