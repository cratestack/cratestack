//! List-handler logging + paging blocks (total_count gate,
//! success-value wrapper, and the per-paging variant of the
//! `tracing` log).

use quote::quote;

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
            match count_request.run(&ctx).await {
                Ok(records) => records.len() as i64,
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

pub(super) fn list_result_log_tokens(
    paged: bool,
    list_route_path: &str,
    model_name: &str,
) -> proc_macro2::TokenStream {
    if paged {
        quote! {
            match &result {
                Ok(page) => ::cratestack::tracing::info!(
                    target: "cratestack",
                    cratestack_route = #list_route_path,
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
                    cratestack_route = #list_route_path,
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
                    cratestack_route = #list_route_path,
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
                    cratestack_route = #list_route_path,
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
