//! `transport grpc` server codegen (ticket #171,
//! `docs/design/protobuf.md` §7). Orchestrates:
//!
//! 1. [`crate::include::grpc_pb::lock`] — read + validate the committed
//!    `<schema>.pb.lock`.
//! 2. [`crate::include::grpc_pb::message`]/[`crate::include::grpc_pb::
//!    update_message`] — `pb::{Model,TypeDecl,Create<M>Input,
//!    Update<M>Input}` mirror structs + `From`/`TryFrom` domain
//!    conversions. Shared with `include::client::grpc` (ticket #209) via
//!    `crate::include::grpc_pb::build_domain_pb_items` — see that module's
//!    doc for why this part of the pb surface is role-agnostic.
//! 3. [`rpc_inputs`]/[`rpc_list`] — the gRPC-only request/response wrapper
//!    messages (`<Model>RpcPkInput`, `<Model>RpcUpdateInput`,
//!    `<Model>RpcListInput`, `StringList`, `RpcListPredicate`,
//!    `PageOf<Model>`, `PageInfo`). **Server-only** — these bake in
//!    decode-only inherent methods only the hand-rolled service below
//!    calls; `include::client::grpc::rpc_inputs` builds its own
//!    encode-facing equivalents rather than reusing these (see that
//!    module's doc).
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
//! **Scope (ticket #171 model CRUD, ticket #208 procedures):** this pass
//! covers model CRUD (`list`/`get`/`create`/`update`/`delete`) end to
//! end — a model with no primary key or with `create` policy-gated off
//! already narrows correctly (mirroring `cratestack-proto::emit::
//! service`'s own gating) — *and* `procedure` declarations, unary or
//! server-streaming per [`cratestack_core::TypeArity::List`] (see
//! [`service::build_service`]'s doc for exactly what "server-streaming"
//! means here). [`procedures`] builds the per-procedure `pb::<Base>
//! {Input,Output}` mirror messages; any `Page<T>` a procedure returns
//! that isn't already covered by a model-with-pk's own `PageOf<Model>`
//! (below) gets its `PageOf<Item>` synthesized here too, deduplicated
//! against both sources the same way `cratestack-proto::emit::
//! synth_page::synthesize_pages` already deduplicates them in the
//! `.pb.lock`/`.proto`.

mod api_server;
mod arm_support;
mod crud_arm_list;
mod crud_arm_spec;
mod crud_arms;
mod procedure_arms;
mod procedures;
mod rpc_inputs;
mod rpc_list;
mod service;

use std::collections::BTreeSet;
use std::path::Path;

use cratestack_core::{Schema, TransportStyle};
use proc_macro::TokenStream;
use quote::quote;
use syn::LitStr;

use crate::include::grpc_pb::{build_domain_pb_items, models_with_pk, numbers_for};

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
    let pb_lock =
        crate::include::grpc_pb::lock::load_pb_lock(schema, schema_resolved, &extra_messages)
            .map_err(|error| super::collect::compile_error(schema_path, error))?;

    let enum_names: BTreeSet<&str> = schema.enums.iter().map(|e| e.name.as_str()).collect();

    let mut pb_items = build_domain_pb_items(schema, &pb_lock, &enum_names)
        .map_err(|error| super::collect::compile_error(schema_path, error))?;

    let models_with_pk = models_with_pk(schema);

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

    // Ticket #208: per-procedure `<Base>Input`/`<Base>Output` mirrors,
    // plus any `PageOf<Item>` a `Page<T>`-returning procedure needs that
    // the models-with-pk loop above didn't already emit — deduplicated
    // the same way `cratestack-proto::emit::synth_page::synthesize_pages`
    // deduplicates the two sources in the `.pb.lock`/`.proto` itself, so
    // a schema where a model's own `list` and a procedure both return
    // `Page<SameModel>` gets exactly one `PageOfSameModel` mirror.
    let covered_pages: BTreeSet<String> = models_with_pk
        .iter()
        .map(|(model, _)| format!("PageOf{}", model.name))
        .collect();
    let mut extra_pages_emitted: BTreeSet<String> = BTreeSet::new();
    for procedure in &schema.procedures {
        let base = cratestack_proto::to_pascal_case(&procedure.name);
        let input_numbers = numbers_for(&pb_lock, &format!("{base}Input"))
            .map_err(|error| super::collect::compile_error(schema_path, error))?;
        pb_items.push(
            procedures::render_procedure_input(procedure, input_numbers, &enum_names)
                .map_err(|error| super::collect::compile_error(schema_path, error))?,
        );
        let output_numbers = numbers_for(&pb_lock, &format!("{base}Output"))
            .map_err(|error| super::collect::compile_error(schema_path, error))?;
        pb_items.push(
            procedures::render_procedure_output(procedure, output_numbers, &enum_names)
                .map_err(|error| super::collect::compile_error(schema_path, error))?,
        );

        let Some(item) = procedure.return_type.page_item() else {
            continue;
        };
        let message_name = format!("PageOf{}", item.name);
        if covered_pages.contains(&message_name)
            || !extra_pages_emitted.insert(message_name.clone())
        {
            continue;
        }
        let page_numbers = numbers_for(&pb_lock, &message_name)
            .map_err(|error| super::collect::compile_error(schema_path, error))?;
        pb_items.push(
            rpc_list::render_page_of(&item.name, page_numbers)
                .map_err(|error| super::collect::compile_error(schema_path, error))?,
        );
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
