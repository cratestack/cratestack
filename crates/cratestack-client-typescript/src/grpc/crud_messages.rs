//! Seeds [`super::messages`]'s recursive collector with every message a
//! CRUD-only gRPC-Web client touches: each model's response shape
//! (recursing into relations/`type` blocks via the collector),
//! `Create<M>Input`/`Update<M>Input`, the per-model RPC wrapper inputs
//! (`<M>RpcPkInput`/`<M>RpcUpdateInput`/a reduced `<M>RpcListInput` — see
//! [`super::list_input_wire_fields`] for which fields), and the shared
//! `PageInfo`/`PageOf<M>` pair. Split out of `messages.rs` to stay under
//! the repo's 200-LoC convention — this file knows about CRUD verbs, the
//! collector it calls into doesn't.

use std::collections::BTreeSet;

use cratestack_core::{Model, Schema, TypeArity};
use cratestack_proto::PbLock;

use super::messages::{GrpcMessageView, Seen, collect_from_fields, collect_message};
use super::synth_fields::{
    page_info_wire_fields, page_of_wire_fields, scalar_fields_for_create, scalar_fields_for_update,
    scalar_ty, synthetic_field,
};
use crate::error::TypeScriptGeneratorError;

pub(crate) fn build_grpc_messages(
    schema: &Schema,
    pb_lock: &PbLock,
    models_with_pk: &[&Model],
    enum_names: &BTreeSet<&str>,
) -> Result<Vec<GrpcMessageView>, TypeScriptGeneratorError> {
    let mut seen: Seen = Seen::new();
    let all_model_names: BTreeSet<&str> = schema.models.iter().map(|m| m.name.as_str()).collect();

    collect_from_fields(
        "PageInfo",
        &page_info_wire_fields(),
        pb_lock,
        enum_names,
        schema,
        true,
        &mut seen,
    )?;
    // `PageInfo.hasNextPage`/`hasPreviousPage` are the one place this
    // generator emits implicit-presence fields (mirrors
    // `cratestack-proto::emit::message::render_page_info` — the same two
    // fields bypass the universal-optional rule on the Rust/`.proto` side
    // too) — see `GrpcFieldDescriptor::defaults_when_absent`'s doc.
    if let Some(page_info) = seen.get_mut("PageInfo") {
        for field in &mut page_info.fields {
            if field.property == "hasNextPage" || field.property == "hasPreviousPage" {
                field.defaults_when_absent = true;
            }
        }
    }

    for model in models_with_pk {
        collect_message(&model.name, schema, pb_lock, enum_names, &mut seen)?;

        // `Create<M>Input` only exists — on the wire, in the lock, and in
        // this generator's client method — when the model allows create
        // (`cratestack-proto::emit::synth`'s own gate, mirrored by
        // `methods::build_grpc_model_view`'s `create_method: Option<_>`).
        // Collecting it unconditionally here would look up a lock entry
        // that was never assigned for a create-disabled model.
        if crate::types::model_allows_create(model) {
            collect_from_fields(
                &format!("Create{}Input", model.name),
                &scalar_fields_for_create(model, &all_model_names),
                pb_lock,
                enum_names,
                schema,
                false,
                &mut seen,
            )?;
        }
        collect_from_fields(
            &format!("Update{}Input", model.name),
            &scalar_fields_for_update(model, &all_model_names),
            pb_lock,
            enum_names,
            schema,
            false,
            &mut seen,
        )?;

        let pk = crate::types::primary_key_field(model)
            .expect("models_with_pk only contains models with a primary key");
        collect_from_fields(
            &format!("{}RpcPkInput", model.name),
            &[synthetic_field("id", pk.ty.clone())],
            pb_lock,
            enum_names,
            schema,
            false,
            &mut seen,
        )?;
        collect_from_fields(
            &format!("{}RpcUpdateInput", model.name),
            &[
                synthetic_field("id", pk.ty.clone()),
                synthetic_field(
                    "patch",
                    scalar_ty(&format!("Update{}Input", model.name), TypeArity::Required),
                ),
            ],
            pb_lock,
            enum_names,
            schema,
            false,
            &mut seen,
        )?;
        collect_from_fields(
            &format!("{}RpcListInput", model.name),
            &super::list_input_wire_fields(),
            pb_lock,
            enum_names,
            schema,
            false,
            &mut seen,
        )?;

        collect_from_fields(
            &format!("PageOf{}", model.name),
            &page_of_wire_fields(&model.name),
            pb_lock,
            enum_names,
            schema,
            true,
            &mut seen,
        )?;
    }

    Ok(seen.into_values().collect())
}
