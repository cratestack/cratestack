//! The gRPC-only request/response wrapper messages `cratestack-proto`
//! already synthesizes into the `.pb.lock` and `.proto` (ticket #170,
//! `cratestack-proto::emit::rpc_input_synth`/`synth_page`):
//! `<Model>RpcPkInput`, `<Model>RpcUpdateInput`, `<Model>RpcListInput`,
//! the shared `StringList`/`RpcListPredicate`, and `PageOf<Model>` (+ the
//! shared `PageInfo`). This module renders their Rust mirror side and the
//! glue that turns a decoded mirror into exactly the arguments the
//! existing `_dispatch` fns (`super::axum::handle_*_dispatch`) already
//! take — reusing `super::scalar`'s field-level wire<->domain conversion
//! primitives (the same ones `message.rs` uses for ordinary model/type
//! fields) rather than re-deriving PK type handling here.
//!
//! Unlike `message.rs`, none of these wrapper messages get a `TryFrom` to
//! a *struct*: the dispatch fns want a bare PK value, a `Bytes`-encoded
//! patch, or a `raw_query: Option<String>` — not a wrapper struct — so
//! each helper here returns exactly what its call site needs directly,
//! via an inherent method on the mirror struct rather than a trait impl.

use std::collections::BTreeMap;

use cratestack_core::Field;
use quote::quote;

use crate::shared::{ident, rust_type_tokens};

use crate::include::grpc_pb::scalar::{domain_from_wire_expr, scalar_wire};

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

/// `StringList { repeated string values = 1; }` — shared across every
/// model's `RpcListInput.include_fields` map values. Schema-global (one
/// per schema, not one per model), matching
/// `rpc_input_synth::synthesize_rpc_inputs`'s "emitted once per file"
/// comment.
pub(super) fn render_string_list(
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

/// `RpcListPredicate { string key = 1; string value = 2; }` — shared,
/// schema-global (see [`render_string_list`]).
pub(super) fn render_rpc_list_predicate(
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

/// `PageInfo { optional int64 limit = 1; optional int64 offset = 2; bool
/// has_next_page = 3; bool has_previous_page = 4; }` — shared, schema-global.
pub(super) fn render_page_info(
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

        impl ::core::convert::From<&::cratestack::PageInfo> for PageInfo {
            fn from(value: &::cratestack::PageInfo) -> Self {
                Self {
                    limit: value.limit,
                    offset: value.offset,
                    has_next_page: value.has_next_page,
                    has_previous_page: value.has_previous_page,
                }
            }
        }
    })
}

struct PkWire {
    rust_type: proc_macro2::TokenStream,
    prost_kind: proc_macro2::TokenStream,
}

/// The PK field's wire type, reused by `RpcPkInput` and `RpcUpdateInput`.
/// Errors (rather than panics) if the PK's `.cstack` type isn't one of
/// `scalar.rs`'s known scalars — parser-enforced today (a primary key is
/// always a scalar), kept as a `Result` rather than an `unreachable!`
/// since this crate's convention (`lock.rs`, `message.rs`) prefers a
/// diagnostic `compile_error!` over a panic for any state that isn't a
/// purely internal invariant.
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

/// `<Model>RpcPkInput { optional <pk-wire> id = N; }` plus an inherent
/// `into_pk()` that extracts the bare domain PK value —
/// `handle_get_*_dispatch`/`handle_delete_*_dispatch` want the PK
/// directly, not a wrapper struct.
pub(super) fn render_rpc_pk_input(
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
    let pk_domain_type = rust_type_tokens(&pk.ty);
    let to_domain = domain_from_wire_expr(pk.ty.name.as_str(), quote! { raw }, &message_name, "id");
    Ok(quote! {
        #[derive(Clone, PartialEq, ::cratestack::grpc::prost::Message)]
        pub struct #ident_tok {
            #[prost(#kind, optional, tag = #number)]
            pub id: Option<#rust_type>,
        }

        impl #ident_tok {
            pub(super) fn into_pk(self) -> ::core::result::Result<#pk_domain_type, ::cratestack::CoolError> {
                let raw = self.id.ok_or_else(|| {
                    ::cratestack::CoolError::BadRequest("missing `id`".to_owned())
                })?;
                #to_domain
            }
        }
    })
}

/// `<Model>RpcUpdateInput { optional <pk-wire> id = N; optional
/// Update<Model>Input patch = M; }` plus an inherent `into_id_and_patch()`
/// returning `(Pk, Update<Model>Input)` — exactly the two arguments
/// `handle_update_*_dispatch` needs (the patch still has to be re-encoded
/// through the codec by the caller, same as the RPC binding's own update
/// dispatch arm — see `transport::rpc::generate_model_rpc_dispatch_arms`).
pub(super) fn render_rpc_update_input(
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
    let pk_domain_type = rust_type_tokens(&pk.ty);
    let to_domain = domain_from_wire_expr(pk.ty.name.as_str(), quote! { raw }, &message_name, "id");
    let update_input_ident = ident(&format!("Update{model_name}Input"));
    Ok(quote! {
        #[derive(Clone, PartialEq, ::cratestack::grpc::prost::Message)]
        pub struct #ident_tok {
            #[prost(#kind, optional, tag = #id_number)]
            pub id: Option<#rust_type>,
            #[prost(message, optional, boxed, tag = #patch_number)]
            pub patch: Option<Box<#update_input_ident>>,
        }

        impl #ident_tok {
            pub(super) fn into_id_and_patch(
                self,
            ) -> ::core::result::Result<(#pk_domain_type, super::super::#update_input_ident), ::cratestack::CoolError> {
                let raw = self.id.ok_or_else(|| {
                    ::cratestack::CoolError::BadRequest("missing `id`".to_owned())
                })?;
                let id = #to_domain?;
                let patch_pb = self.patch.ok_or_else(|| {
                    ::cratestack::CoolError::BadRequest("missing `patch`".to_owned())
                })?;
                let patch = super::super::#update_input_ident::try_from(*patch_pb)?;
                Ok((id, patch))
            }
        }
    })
}
