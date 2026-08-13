//! `<Model>RpcListInput` and `PageOf<Model>` — split out of
//! [`super::rpc_inputs`] to stay under the repo's 200-LoC file convention
//! (same split the server side already has, `include::server::grpc::
//! {rpc_inputs,rpc_list}`). Bare structs, no inherent methods or trait
//! impls — see `rpc_inputs.rs`'s module doc for why.

use std::collections::BTreeMap;

use quote::quote;

use crate::shared::ident;

use super::rpc_inputs::lock_number;

/// `<Model>RpcListInput` — 9 fields mirroring
/// `cratestack_axum::rpc::RpcListInput` 1:1 (same field set the
/// server-side renderer targets), bare struct, no inherent methods. The
/// client builds one from `&::cratestack::rpc::RpcListInput` inline at the
/// call site (`tonic_client.rs`) rather than via a generated `From` impl
/// here — the field mapping is a straight 1:1 copy, not worth a
/// generated-code indirection.
pub(crate) fn render_rpc_list_input(
    model_name: &str,
    numbers: &BTreeMap<String, i32>,
) -> Result<proc_macro2::TokenStream, String> {
    let message_name = format!("{model_name}RpcListInput");
    let ident_tok = ident(&message_name);
    let limit = lock_number(numbers, &message_name, "limit")?;
    let offset = lock_number(numbers, &message_name, "offset")?;
    let fields = lock_number(numbers, &message_name, "fields")?;
    let include = lock_number(numbers, &message_name, "include")?;
    let include_fields = lock_number(numbers, &message_name, "include_fields")?;
    let sort = lock_number(numbers, &message_name, "sort")?;
    let where_expr = lock_number(numbers, &message_name, "where_expr")?;
    let or_field = lock_number(numbers, &message_name, "or")?;
    let filters = lock_number(numbers, &message_name, "filters")?;

    Ok(quote! {
        #[derive(Clone, PartialEq, ::cratestack::grpc::prost::Message)]
        pub struct #ident_tok {
            #[prost(int64, optional, tag = #limit)]
            pub limit: Option<i64>,
            #[prost(int64, optional, tag = #offset)]
            pub offset: Option<i64>,
            #[prost(string, repeated, tag = #fields)]
            pub fields: Vec<String>,
            #[prost(string, repeated, tag = #include)]
            pub include: Vec<String>,
            #[prost(map = "string, message", tag = #include_fields)]
            pub include_fields: ::std::collections::HashMap<String, StringList>,
            #[prost(string, optional, tag = #sort)]
            pub sort: Option<String>,
            #[prost(string, optional, tag = #where_expr)]
            pub where_expr: Option<String>,
            #[prost(string, optional, tag = #or_field)]
            pub or: Option<String>,
            #[prost(message, repeated, tag = #filters)]
            pub filters: Vec<RpcListPredicate>,
        }
    })
}

/// `PageOf<Model> { repeated Model items = 1; optional int64 total_count =
/// 2; optional PageInfo page_info = 3; }` — bare struct, no trait impls.
/// The client decodes one into `::cratestack::Page<Model>` inline at the
/// call site (each item via the already-generated `TryFrom<pb::Model> for
/// Model`, from `grpc_pb::message::render_message`).
pub(crate) fn render_page_of(
    model_name: &str,
    numbers: &BTreeMap<String, i32>,
) -> Result<proc_macro2::TokenStream, String> {
    let message_name = format!("PageOf{model_name}");
    let ident_tok = ident(&message_name);
    let items = lock_number(numbers, &message_name, "items")?;
    let total_count = lock_number(numbers, &message_name, "total_count")?;
    let page_info = lock_number(numbers, &message_name, "page_info")?;
    let model_ident = ident(model_name);

    Ok(quote! {
        #[derive(Clone, PartialEq, ::cratestack::grpc::prost::Message)]
        pub struct #ident_tok {
            #[prost(message, repeated, tag = #items)]
            pub items: Vec<#model_ident>,
            #[prost(int64, optional, tag = #total_count)]
            pub total_count: Option<i64>,
            #[prost(message, optional, tag = #page_info)]
            pub page_info: Option<PageInfo>,
        }
    })
}
