//! `include_client_schema!` gRPC client codegen (ticket #209) — the
//! `tonic`-client twin of `include::server::grpc` (ticket #171). Emits,
//! for a `transport grpc` schema:
//!
//! 1. The same `pb::{Model,TypeDecl,Create<M>Input,Update<M>Input}` mirror
//!    structs the server emits (`crate::include::grpc_pb::
//!    build_domain_pb_items` — role-agnostic, shared verbatim; see that
//!    module's doc), plus this crate's **own** encode-facing
//!    `<Model>RpcPkInput`/`<Model>RpcUpdateInput`/`<Model>RpcListInput`/
//!    `PageOf<Model>`/`StringList`/`RpcListPredicate`/`PageInfo` wrapper
//!    messages ([`rpc_inputs`]) — deliberately **not**
//!    `include::server::grpc`'s own versions of those, which bake in
//!    decode-only inherent methods a client never calls (dead-code hazard
//!    — see `rpc_inputs.rs`'s module doc).
//! 2. [`tonic_client`] — the hand-rolled `tonic::client::Grpc<T>`-based
//!    client, one struct + method per model CRUD verb, mirroring what
//!    `tonic-build` itself emits for a client stub — same "verified
//!    directly against the pinned tonic version's own source" precedent
//!    `include::server::grpc::service`'s module doc already established
//!    for the server half.
//!
//! Reached only when `super::super::reject_grpc::guard_client_grpc_transport`
//! has already let the schema through — i.e. `transport grpc` *and*
//! `cratestack-macros`'s own `grpc` Cargo feature is on — so everything
//! below can assume the feature is live, same precedent as
//! `include::server::grpc`'s module doc.
//!
//! **Scope note (ticket #209, matches #171's own):** model CRUD only.
//! `transport grpc` procedures are not wired into the generated tonic
//! *service* yet (`include::server::grpc`'s own scope note), so there is
//! nothing for this client generator to bind a method to either — a
//! schema with `procedure` declarations gets no gRPC client method for
//! them, tracked as the sibling out-of-scope item ticket #209 itself
//! calls out.

mod client_struct;
mod model_api;
mod rpc_inputs;
mod rpc_list;

use std::collections::BTreeSet;
use std::path::Path;

use cratestack_core::{Schema, TransportStyle};
use proc_macro::TokenStream;
use quote::quote;
use syn::LitStr;

use crate::include::grpc_pb::{build_domain_pb_items, models_with_pk, numbers_for};

fn compile_error(schema_path: &LitStr, error: String) -> TokenStream {
    syn::Error::new(schema_path.span(), error)
        .to_compile_error()
        .into()
}

pub(super) fn build_client_grpc_module(
    schema: &Schema,
    schema_resolved: &Path,
    schema_path: &LitStr,
) -> Result<proc_macro2::TokenStream, TokenStream> {
    if schema.transport != TransportStyle::Grpc {
        return Ok(quote! {});
    }

    let extra_messages = cratestack_proto::synthesize_messages(schema)
        .map_err(|error| compile_error(schema_path, error.to_string()))?;
    let pb_lock =
        crate::include::grpc_pb::lock::load_pb_lock(schema, schema_resolved, &extra_messages)
            .map_err(|error| compile_error(schema_path, error))?;

    let enum_names: BTreeSet<&str> = schema.enums.iter().map(|e| e.name.as_str()).collect();

    let mut pb_items = build_domain_pb_items(schema, &pb_lock, &enum_names)
        .map_err(|error| compile_error(schema_path, error))?;

    let models_with_pk = models_with_pk(schema);

    if !models_with_pk.is_empty() {
        let string_list_numbers = numbers_for(&pb_lock, "StringList")
            .map_err(|error| compile_error(schema_path, error))?;
        pb_items.push(
            rpc_inputs::render_string_list(string_list_numbers)
                .map_err(|error| compile_error(schema_path, error))?,
        );
        let predicate_numbers = numbers_for(&pb_lock, "RpcListPredicate")
            .map_err(|error| compile_error(schema_path, error))?;
        pb_items.push(
            rpc_inputs::render_rpc_list_predicate(predicate_numbers)
                .map_err(|error| compile_error(schema_path, error))?,
        );
        let page_info_numbers =
            numbers_for(&pb_lock, "PageInfo").map_err(|error| compile_error(schema_path, error))?;
        pb_items.push(
            rpc_inputs::render_page_info(page_info_numbers)
                .map_err(|error| compile_error(schema_path, error))?,
        );

        for (model, pk) in &models_with_pk {
            let pk_numbers = numbers_for(&pb_lock, &format!("{}RpcPkInput", model.name))
                .map_err(|error| compile_error(schema_path, error))?;
            pb_items.push(
                rpc_inputs::render_rpc_pk_input(&model.name, pk, pk_numbers)
                    .map_err(|error| compile_error(schema_path, error))?,
            );
            let update_numbers = numbers_for(&pb_lock, &format!("{}RpcUpdateInput", model.name))
                .map_err(|error| compile_error(schema_path, error))?;
            pb_items.push(
                rpc_inputs::render_rpc_update_input(&model.name, pk, update_numbers)
                    .map_err(|error| compile_error(schema_path, error))?,
            );
            let list_numbers = numbers_for(&pb_lock, &format!("{}RpcListInput", model.name))
                .map_err(|error| compile_error(schema_path, error))?;
            pb_items.push(
                rpc_list::render_rpc_list_input(&model.name, list_numbers)
                    .map_err(|error| compile_error(schema_path, error))?,
            );
            let page_numbers = numbers_for(&pb_lock, &format!("PageOf{}", model.name))
                .map_err(|error| compile_error(schema_path, error))?;
            pb_items.push(
                rpc_list::render_page_of(&model.name, page_numbers)
                    .map_err(|error| compile_error(schema_path, error))?,
            );
        }
    }

    let package = pb_lock.package.clone().unwrap_or_default();
    let client_struct_tokens = client_struct::build_client_struct(&package, &models_with_pk);
    let model_apis = models_with_pk
        .iter()
        .map(|(model, pk)| model_api::build_model_api(model, pk));

    Ok(quote! {
        pub mod grpc {
            pub mod pb {
                #(#pb_items)*
            }

            #client_struct_tokens

            #(#model_apis)*
        }
    })
}
