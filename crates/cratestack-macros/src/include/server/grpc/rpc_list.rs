//! `<Model>RpcListInput` (the `list` verb's gRPC request message — 9
//! fields mirroring `cratestack_axum::rpc::RpcListInput` 1:1, per
//! `cratestack-proto::emit::rpc_input_synth::rpc_list_input_fields`) and
//! `PageOf<Model>` (the `list` verb's response — every model's `list`
//! returns `Page<Model>` under gRPC regardless of the model's own
//! `@@paged` attribute, per `cratestack-proto::emit::synth_page`'s module
//! doc). Split out of `rpc_inputs.rs` to stay under the repo's 200-LoC
//! file convention.

use std::collections::BTreeMap;

use quote::quote;

use crate::shared::ident;

use super::rpc_inputs::lock_number;

/// `<Model>RpcListInput { optional int64 limit = 1; optional int64 offset
/// = 2; repeated string fields = 3; repeated string include = 4; map
/// <string, StringList> include_fields = 5; optional string sort = 6;
/// optional string where_expr = 7; optional string or = 8; repeated
/// RpcListPredicate filters = 9; }` plus an inherent `into_domain()`
/// building the existing `cratestack::rpc::RpcListInput` —
/// `::cratestack::rpc::synthesize_list_query` (already shipped, used by
/// the RPC binding's own `model.<M>.list` dispatch arm) turns that
/// straight into the `raw_query: Option<String>`
/// `handle_list_*_dispatch` wants, so gRPC's `list` method reuses the
/// exact same query-synthesis path RPC already does.
pub(super) fn render_rpc_list_input(
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

        impl #ident_tok {
            pub(super) fn into_domain(self) -> ::cratestack::rpc::RpcListInput {
                ::cratestack::rpc::RpcListInput {
                    limit: self.limit,
                    offset: self.offset,
                    fields: if self.fields.is_empty() { None } else { Some(self.fields) },
                    include: if self.include.is_empty() { None } else { Some(self.include) },
                    include_fields: self.include_fields
                        .into_iter()
                        .map(|(key, value)| (key, value.values))
                        .collect(),
                    sort: self.sort,
                    where_expr: self.where_expr,
                    or: self.or,
                    filters: self.filters
                        .into_iter()
                        .map(|predicate| ::cratestack::rpc::RpcListPredicate {
                            key: predicate.key,
                            value: predicate.value,
                        })
                        .collect(),
                }
            }
        }
    })
}

/// `PageOf<Model> { repeated Model items = 1; optional int64 total_count =
/// 2; optional PageInfo page_info = 3; }` plus `From<&Page<Model>>`
/// (response-encode direction only — `list` never decodes a `PageOf<M>`).
pub(super) fn render_page_of(
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

        impl ::core::convert::From<&::cratestack::Page<super::super::#model_ident>> for #ident_tok {
            fn from(value: &::cratestack::Page<super::super::#model_ident>) -> Self {
                Self {
                    items: value.items.iter().map(#model_ident::from).collect(),
                    total_count: value.total_count,
                    page_info: Some(PageInfo::from(&value.page_info)),
                }
            }
        }
    })
}
