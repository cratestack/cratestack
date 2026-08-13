//! Shared, role-agnostic gRPC pb-mirror-struct builders — reused by both
//! `include::server::grpc` (ticket #171, the tonic *service*) and
//! `include::client::grpc` (ticket #209, the tonic *client*). Everything
//! here builds the `Model`/`TypeDecl`/`Create<M>Input`/`Update<M>Input` pb
//! mirror structs and their `From`/`TryFrom` domain conversions — the part
//! of the pb surface that is identical regardless of which entry macro is
//! generating it, since both sides need the exact same wire shape for the
//! exact same `.pb.lock` field numbers to interoperate at all.
//!
//! **Deliberately does not cover** the gRPC-only CRUD-wrapper messages
//! (`<Model>RpcPkInput`, `<Model>RpcUpdateInput`, `<Model>RpcListInput`,
//! `PageOf<Model>`, `StringList`, `RpcListPredicate`): the server's own
//! versions of those (`include::server::grpc::{rpc_inputs,rpc_list}`) bake
//! in decode-only inherent methods (`into_pk`, `into_id_and_patch`,
//! `into_domain`) that only the hand-rolled server dispatch ever calls —
//! reusing them verbatim for the client would ship inherent methods the
//! client-generated code never calls, which `-D warnings`' `dead_code`
//! lint would flag in a client-only crate (no server code in the same
//! crate to call them). `include::client::grpc::rpc_inputs` builds the
//! same wire shapes independently, with client-appropriate (encode-facing)
//! helpers instead — see that module's doc for the full reasoning.

pub(crate) mod fields;
pub(crate) mod lock;
pub(crate) mod message;
mod patch_field;
pub(crate) mod scalar;
pub(crate) mod update_message;

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::Schema;

use crate::shared::{is_primary_key, is_readonly_field, is_server_only_field, is_version_field};
use fields::{model_allows_create, scalar_model_fields, visible_model_fields, visible_type_fields};

/// Builds the `Model`/`TypeDecl`/`Create<M>Input`/`Update<M>Input` pb
/// mirror-struct tokens shared by both the server and client gRPC
/// composers — everything each composer needs before branching into its
/// own CRUD-wrapper-message and service/client tail. Mirrors the loop that
/// used to live directly in `include::server::grpc::build_grpc_module`
/// before ticket #209 gave it a second caller.
pub(crate) fn build_domain_pb_items(
    schema: &Schema,
    pb_lock: &cratestack_proto::PbLock,
    enum_names: &BTreeSet<&str>,
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    let model_names: BTreeSet<&str> = schema.models.iter().map(|m| m.name.as_str()).collect();
    let mut pb_items = Vec::new();

    for model in &schema.models {
        let numbers = numbers_for(pb_lock, &model.name)?;
        let fields = visible_model_fields(model);
        let domain_path = {
            let ident = crate::shared::ident(&model.name);
            quote::quote! { super::super::#ident }
        };
        let rendered =
            message::render_message(&model.name, domain_path, &fields, numbers, enum_names)?;
        pb_items.push(rendered.tokens);

        if model_allows_create(model) {
            let create_name = format!("Create{}Input", model.name);
            let create_numbers = numbers_for(pb_lock, &create_name)?;
            let create_fields = scalar_model_fields(model, &model_names)
                .into_iter()
                .filter(|field| !crate::shared::is_generated_on_create(field))
                .collect::<Vec<_>>();
            let create_ident = crate::shared::ident(&create_name);
            let rendered = message::render_message(
                &create_name,
                quote::quote! { super::super::#create_ident },
                &create_fields,
                create_numbers,
                enum_names,
            )?;
            pb_items.push(rendered.tokens);
        }

        let update_name = format!("Update{}Input", model.name);
        let update_numbers = numbers_for(pb_lock, &update_name)?;
        let update_fields = update_input_fields(model, &model_names);
        let update_ident = crate::shared::ident(&update_name);
        let rendered = update_message::render_update_message(
            &update_name,
            quote::quote! { super::super::#update_ident },
            &update_fields,
            update_numbers,
            enum_names,
        )?;
        pb_items.push(rendered.tokens);
    }

    for ty in &schema.types {
        let numbers = numbers_for(pb_lock, &ty.name)?;
        let fields = visible_type_fields(ty);
        let ty_ident = crate::shared::ident(&ty.name);
        let rendered = message::render_message(
            &ty.name,
            quote::quote! { super::super::#ty_ident },
            &fields,
            numbers,
            enum_names,
        )?;
        pb_items.push(rendered.tokens);
    }

    Ok(pb_items)
}

/// Models with a primary key, in schema order — the only models either
/// composer emits gRPC CRUD-wrapper messages/methods for (no PK means no
/// `get`/`update`/`delete`, per the same gating `cratestack-proto::emit`
/// already applies to REST/RPC).
pub(crate) fn models_with_pk(
    schema: &Schema,
) -> Vec<(&cratestack_core::Model, &cratestack_core::Field)> {
    schema
        .models
        .iter()
        .filter_map(|model| {
            model
                .fields
                .iter()
                .find(|field| is_primary_key(field))
                .map(|pk| (model, pk))
        })
        .collect()
}

pub(crate) fn numbers_for<'a>(
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
pub(crate) fn update_input_fields<'a>(
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
