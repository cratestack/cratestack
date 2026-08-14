//! List-handler logging + paging blocks (total_count gate,
//! success-value wrapper, and the per-paging variant of the
//! `tracing` log).

use quote::quote;

/// `total_count` for a `@@paged` model's list route.
///
/// Reuses `#list_builder_ident` — the same builder function that
/// assembles the page query's `WHERE`/policy scoping — with paging
/// disabled, then converts the resulting `FindMany` into an
/// `AggregateCount` via `From` (cratestack-sqlx's
/// `query/read/aggregate_count.rs`) so the count issues `SELECT
/// COUNT(*) ...` instead of materialising every matching row just to
/// call `.len()` on it (cratestack#570). The `From` conversion moves
/// the already-built `filters` vector over verbatim rather than
/// re-deriving it, which is what keeps the count's `WHERE` clause and
/// policy scope byte-identical to the page query's — a divergence
/// there would let a caller learn the size of a result set policy
/// doesn't let them read.
pub(super) fn total_count_tokens(
    paged: bool,
    list_builder_ident: &syn::Ident,
    list_response_type: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if !paged {
        return quote! {};
    }
    quote! {
        let total_count = {
            let count_request = match #list_builder_ident(&state.db, &query, false) {
                Ok(request) => request,
                Err(error) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, #list_response_type>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
                }
            };
            match ::cratestack::AggregateCount::from(count_request).run(&ctx).await {
                Ok(count) => count,
                Err(error) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, #list_response_type>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
                }
            }
        };
    }
}

pub(super) fn list_success_tokens(paged: bool) -> proc_macro2::TokenStream {
    if !paged {
        return quote! { Ok(values) };
    }
    quote! {{
        let limit = query.limit;
        let offset = query.offset.unwrap_or(0);
        Ok(::cratestack::Page::new(
            values,
            ::cratestack::PageInfo {
                limit,
                offset: query.offset,
                has_next_page: limit.is_some_and(|limit| offset + limit < total_count),
                has_previous_page: offset > 0,
            },
        ).with_total_count(Some(total_count)))
    }}
}

pub(super) fn list_result_log_tokens(paged: bool, model_name: &str) -> proc_macro2::TokenStream {
    if paged {
        quote! {
            match &result {
                Ok(page) => ::cratestack::tracing::info!(
                    target: "cratestack",
                    cratestack_route = canonical_route,
                    cratestack_model = #model_name,
                    cratestack_operation = "list",
                    cratestack_paged = true,
                    cratestack_limit = ?query.limit,
                    cratestack_offset = ?query.offset,
                    cratestack_count = page.items.len(),
                    cratestack_total_count = ?page.total_count,
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    cratestack_request_id = ctx.request_id().unwrap_or(""),
                    "cratestack model list completed",
                ),
                Err(error) => ::cratestack::tracing::warn!(
                    target: "cratestack",
                    cratestack_route = canonical_route,
                    cratestack_model = #model_name,
                    cratestack_operation = "list",
                    cratestack_paged = true,
                    cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    cratestack_request_id = ctx.request_id().unwrap_or(""),
                    "cratestack model list failed",
                ),
            }
        }
    } else {
        quote! {
            match &result {
                Ok(values) => ::cratestack::tracing::info!(
                    target: "cratestack",
                    cratestack_route = canonical_route,
                    cratestack_model = #model_name,
                    cratestack_operation = "list",
                    cratestack_paged = false,
                    cratestack_limit = ?query.limit,
                    cratestack_offset = ?query.offset,
                    cratestack_count = values.len(),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    cratestack_request_id = ctx.request_id().unwrap_or(""),
                    "cratestack model list completed",
                ),
                Err(error) => ::cratestack::tracing::warn!(
                    target: "cratestack",
                    cratestack_route = canonical_route,
                    cratestack_model = #model_name,
                    cratestack_operation = "list",
                    cratestack_paged = false,
                    cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    cratestack_request_id = ctx.request_id().unwrap_or(""),
                    "cratestack model list failed",
                ),
            }
        }
    }
}
