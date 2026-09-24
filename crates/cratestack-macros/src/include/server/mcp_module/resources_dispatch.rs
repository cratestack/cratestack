//! The code a `resources/read` runs (cratestack#1040): the `resources`,
//! `read_record` and `read_page` methods of the generated `McpTools` impl,
//! one `match` arm per resource segment.
//!
//! **No second query path.** Each arm is the REST handler's read, restated
//! without HTTP, calling the same generated functions:
//!
//! - a record is `db.<model>().find_unique(id).run(ctx)` — what `GET
//!   /<plural>/{id}` runs (`axum::model::handlers_crud::get`), with the
//!   `detail` policy slice;
//! - a page is `build_<model>_list_request(&db, &query, true)?.run(ctx)` —
//!   the builder `GET /<plural>` runs (`axum::model::serializers`), with the
//!   `list` slice, over a `ModelListQuery` carrying only `limit`, `offset`
//!   and a primary-key `sort` (a total order, so pages cannot overlap);
//! - every record is rendered by `serialize_<model>_model_value`, the REST
//!   serializer, with REST's defaults for a request with no query string
//!   (all fields, no includes, no `computedParams`). That function never
//!   emits a `@server_only` field and resolves `@computed` ones; the
//!   procedure-output `compose_*` helpers are not involved.
//!
//! The row policy is therefore in the SQL of both reads, applied in the
//! `WHERE` before `ORDER BY ... LIMIT ... OFFSET`, under the caller's
//! context. An id that does not parse as the primary key is a row that
//! does not exist: `None`, the same as a hidden one.

use quote::quote;

use crate::include::mcp_gate::ResourcePlan;
use crate::shared::{ident, rust_type_tokens, to_snake_case};

pub(super) fn resource_method_tokens(resources: &[ResourcePlan]) -> proc_macro2::TokenStream {
    if resources.is_empty() {
        // The trait's defaults: no resources, every read `None`/empty.
        return proc_macro2::TokenStream::new();
    }
    let record_arms = resources.iter().map(record_arm);
    let page_arms = resources.iter().map(page_arm);

    quote! {
        fn resources(&self) -> &'static [::cratestack::mcp::ResourceDescriptor] {
            &RESOURCES
        }

        async fn read_record(
            &self,
            segment: &str,
            id: &str,
            ctx: &::cratestack::CratestackContext,
        ) -> ::core::result::Result<
            ::core::option::Option<::cratestack::serde_json::Value>,
            CratestackError,
        > {
            match segment {
                #(#record_arms)*
                _ => ::core::result::Result::Ok(::core::option::Option::None),
            }
        }

        async fn read_page(
            &self,
            segment: &str,
            limit: u32,
            offset: u64,
            ctx: &::cratestack::CratestackContext,
        ) -> ::core::result::Result<::std::vec::Vec<::cratestack::serde_json::Value>, CratestackError>
        {
            let offset = i64::try_from(offset).map_err(|_| {
                CratestackError::BadRequest("the page offset is out of range".to_owned())
            })?;
            match segment {
                #(#page_arms)*
                _ => ::core::result::Result::Ok(::std::vec::Vec::new()),
            }
        }
    }
}

/// `resource_json`, the module-level helper both arms render through.
/// JSON because MCP resource contents are text; the `ProjectedValue` is
/// serialized exactly as REST's JSON codec serializes it.
pub(super) fn resource_support_tokens(resources: &[ResourcePlan]) -> proc_macro2::TokenStream {
    if resources.is_empty() {
        return proc_macro2::TokenStream::new();
    }
    quote! {
        fn resource_json(
            value: &::cratestack::ProjectedValue,
        ) -> ::core::result::Result<::cratestack::serde_json::Value, CratestackError> {
            ::cratestack::serde_json::to_value(value).map_err(|error| {
                CratestackError::Internal(::std::format!(
                    "mcp: could not serialize a resource record: {error}"
                ))
            })
        }
    }
}

struct Names {
    segment: String,
    accessor: syn::Ident,
    serialize: syn::Ident,
    list_builder: syn::Ident,
}

/// The per-model function names `axum::model::prep` gives the REST side.
/// Spelled again here rather than shared; a drift fails to compile, since
/// the call below names a function that would no longer exist.
fn names(resource: &ResourcePlan) -> Names {
    let snake = to_snake_case(&resource.model.name);
    Names {
        segment: resource.segment.clone(),
        accessor: ident(&snake),
        serialize: ident(&format!("serialize_{snake}_model_value")),
        list_builder: ident(&format!("build_{snake}_list_request")),
    }
}

fn record_arm(resource: &ResourcePlan) -> proc_macro2::TokenStream {
    let Names {
        segment,
        accessor,
        serialize,
        ..
    } = names(resource);
    let key_type = rust_type_tokens(&resource.primary_key.ty);
    quote! {
        #segment => {
            let ::core::result::Result::Ok(id) = id.parse::<#key_type>() else {
                return ::core::result::Result::Ok(::core::option::Option::None);
            };
            let selection = super::axum::ModelSelectionQuery::default();
            let computed_params = super::axum::ComputedParamsQuery::new();
            match self.db.#accessor().find_unique(id).run(ctx).await? {
                ::core::option::Option::Some(record) => {
                    let value = super::axum::#serialize(
                        &self.db, &self.resolvers, ctx, &record, &selection,
                        ::core::option::Option::Some(&computed_params),
                    )
                    .await?;
                    resource_json(&value).map(::core::option::Option::Some)
                }
                ::core::option::Option::None => {
                    ::core::result::Result::Ok(::core::option::Option::None)
                }
            }
        }
    }
}

fn page_arm(resource: &ResourcePlan) -> proc_macro2::TokenStream {
    let Names {
        segment,
        serialize,
        list_builder,
        ..
    } = names(resource);
    let key_name = &resource.primary_key.name;
    quote! {
        #segment => {
            let query = super::axum::ModelListQuery {
                limit: ::core::option::Option::Some(i64::from(limit)),
                offset: ::core::option::Option::Some(offset),
                sort: ::core::option::Option::Some(#key_name.to_owned()),
                ..::core::default::Default::default()
            };
            let computed_params = super::axum::ComputedParamsQuery::new();
            let records = super::axum::#list_builder(&self.db, &query, true)?
                .run(ctx)
                .await?;
            let mut values = ::std::vec::Vec::with_capacity(records.len());
            for record in &records {
                let value = super::axum::#serialize(
                    &self.db, &self.resolvers, ctx, record, &query.selection,
                    ::core::option::Option::Some(&computed_params),
                )
                .await?;
                values.push(resource_json(&value)?);
            }
            ::core::result::Result::Ok(values)
        }
    }
}
