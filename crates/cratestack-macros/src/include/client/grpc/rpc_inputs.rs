//! gRPC CRUD-wrapper pb mirror structs for the **client** side —
//! `<Model>RpcPkInput`, `<Model>RpcUpdateInput`, `StringList`,
//! `RpcListPredicate`, `PageInfo`. `<Model>RpcListInput`/`PageOf<Model>`
//! are split out into [`super::rpc_list`] to stay under the repo's
//! 200-LoC file convention — see that module's doc for why it's a
//! sibling rather than folded back in here. Same wire shape (field names,
//! `#[prost(...)]` tags sourced from the same committed `.pb.lock`) as
//! `include::server::grpc::{rpc_inputs,rpc_list}` — required for interop,
//! since these two sides exchange the exact same bytes — but **not** the
//! same generated code.
//!
//! **Why this module exists instead of reusing the server's renderers
//! directly:** the server's versions attach decode-only inherent methods
//! (`into_pk`, `into_id_and_patch`, `into_domain`) that its hand-rolled
//! `tonic::server::Grpc` service arms call to turn a *received* wrapper
//! message into the plain arguments `handle_*_dispatch` wants. A gRPC
//! client never receives any of these three messages — it only ever
//! *sends* them (they are request-only wrappers; the server's responses
//! are always a plain `Model`/`PageOf<Model>`) — so it would never call
//! those methods. Reusing the server's renderers verbatim would ship
//! `pub(super) fn into_pk`/etc. into a client-only crate with no call
//! site, which `-D warnings`' `dead_code` lint flags for an unused
//! *inherent* method (unlike an unused *trait* impl, which rustc never
//! flags — see `grpc_pb::update_message`'s module doc for the same
//! distinction applied to `Update<M>Input`). Emitting bare structs here
//! and constructing them with plain struct-literal syntax (every field is
//! `pub`) at each call site in `tonic_client.rs` sidesteps the problem
//! entirely, at the cost of a small amount of duplicated struct-shape
//! code — the same "small pure shape gets reimplemented per module"
//! precedent already established by `cratestack-client-typescript::grpc::
//! wire`'s module doc and `include::grpc_pb::fields`'s module doc.

use std::collections::BTreeMap;

use cratestack_core::Field;
use quote::quote;

use crate::shared::{ident, rust_type_tokens};

use crate::include::grpc_pb::scalar::scalar_wire;

pub(super) fn lock_number(
    numbers: &BTreeMap<String, i32>,
    message: &str,
    field: &str,
) -> Result<proc_macro2::Literal, String> {
    numbers
        .get(field)
        .map(|n| proc_macro2::Literal::i32_unsuffixed(*n))
        .ok_or_else(|| format!("no `.pb.lock` entry for `{message}.{field}`"))
}

/// Schema-global, emitted once regardless of model count (matches the
/// server's own "emitted once per file" convention).
pub(crate) fn render_string_list(
    numbers: &BTreeMap<String, i32>,
) -> Result<proc_macro2::TokenStream, String> {
    let number = lock_number(numbers, "StringList", "values")?;
    Ok(quote! {
        #[derive(Clone, PartialEq, ::cratestack::grpc::prost::Message)]
        pub struct StringList {
            #[prost(string, repeated, tag = #number)]
            pub values: Vec<String>,
        }
    })
}

pub(crate) fn render_rpc_list_predicate(
    numbers: &BTreeMap<String, i32>,
) -> Result<proc_macro2::TokenStream, String> {
    let key = lock_number(numbers, "RpcListPredicate", "key")?;
    let value = lock_number(numbers, "RpcListPredicate", "value")?;
    Ok(quote! {
        #[derive(Clone, PartialEq, ::cratestack::grpc::prost::Message)]
        pub struct RpcListPredicate {
            #[prost(string, tag = #key)]
            pub key: String,
            #[prost(string, tag = #value)]
            pub value: String,
        }
    })
}

pub(crate) fn render_page_info(
    numbers: &BTreeMap<String, i32>,
) -> Result<proc_macro2::TokenStream, String> {
    let limit = lock_number(numbers, "PageInfo", "limit")?;
    let offset = lock_number(numbers, "PageInfo", "offset")?;
    let has_next = lock_number(numbers, "PageInfo", "has_next_page")?;
    let has_prev = lock_number(numbers, "PageInfo", "has_previous_page")?;
    Ok(quote! {
        #[derive(Clone, PartialEq, ::cratestack::grpc::prost::Message)]
        pub struct PageInfo {
            #[prost(int64, optional, tag = #limit)]
            pub limit: Option<i64>,
            #[prost(int64, optional, tag = #offset)]
            pub offset: Option<i64>,
            #[prost(bool, tag = #has_next)]
            pub has_next_page: bool,
            #[prost(bool, tag = #has_prev)]
            pub has_previous_page: bool,
        }
    })
}

struct PkWire {
    rust_type: proc_macro2::TokenStream,
    prost_kind: proc_macro2::TokenStream,
}

fn pk_wire(pk: &Field) -> Result<PkWire, String> {
    scalar_wire(pk.ty.name.as_str())
        .map(|wire| PkWire {
            rust_type: wire.rust_type,
            prost_kind: wire.prost_kind,
        })
        .ok_or_else(|| {
            format!(
                "primary key `{}` has a non-scalar type; unsupported for gRPC",
                pk.name
            )
        })
}

/// `<Model>RpcPkInput { optional <pk-wire> id = N; }` — bare struct, no
/// inherent methods (see module doc). The client constructs one directly:
/// `pb::WidgetRpcPkInput { id: Some(id) }`.
pub(crate) fn render_rpc_pk_input(
    model_name: &str,
    pk: &Field,
    numbers: &BTreeMap<String, i32>,
) -> Result<proc_macro2::TokenStream, String> {
    let message_name = format!("{model_name}RpcPkInput");
    let ident_tok = ident(&message_name);
    let number = lock_number(numbers, &message_name, "id")?;
    let wire = pk_wire(pk)?;
    let rust_type = &wire.rust_type;
    let kind = &wire.prost_kind;
    Ok(quote! {
        #[derive(Clone, PartialEq, ::cratestack::grpc::prost::Message)]
        pub struct #ident_tok {
            #[prost(#kind, optional, tag = #number)]
            pub id: Option<#rust_type>,
        }
    })
}

/// `<Model>RpcUpdateInput { optional <pk-wire> id = N; optional
/// Update<Model>Input patch = M; }` — bare struct, no inherent methods.
pub(crate) fn render_rpc_update_input(
    model_name: &str,
    pk: &Field,
    numbers: &BTreeMap<String, i32>,
) -> Result<proc_macro2::TokenStream, String> {
    let message_name = format!("{model_name}RpcUpdateInput");
    let ident_tok = ident(&message_name);
    let id_number = lock_number(numbers, &message_name, "id")?;
    let patch_number = lock_number(numbers, &message_name, "patch")?;
    let wire = pk_wire(pk)?;
    let rust_type = &wire.rust_type;
    let kind = &wire.prost_kind;
    let update_input_ident = ident(&format!("Update{model_name}Input"));
    Ok(quote! {
        #[derive(Clone, PartialEq, ::cratestack::grpc::prost::Message)]
        pub struct #ident_tok {
            #[prost(#kind, optional, tag = #id_number)]
            pub id: Option<#rust_type>,
            #[prost(message, optional, boxed, tag = #patch_number)]
            pub patch: Option<Box<#update_input_ident>>,
        }
    })
}

/// PK domain type tokens, reused by `tonic_client.rs` when building each
/// model's `get`/`update`/`delete` method signatures.
pub(crate) fn pk_domain_type(pk: &Field) -> proc_macro2::TokenStream {
    rust_type_tokens(&pk.ty)
}
