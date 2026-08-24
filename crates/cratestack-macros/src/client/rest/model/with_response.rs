//! `*_with_response` method builders for the REST per-model client, split
//! out of `client/rest/model.rs` per the repo's 200-LoC file convention.
//! Each mirrors its plain counterpart (`get`/`update`/`delete`) but
//! returns a `TypedResponse<T>` so callers can read response headers —
//! `ETag`/`If-Match` for `@version` optimistic locking (issue #493,
//! cratestack#519).

use proc_macro2::TokenStream;
use quote::quote;

/// Same call as `get`, but returns the status and response headers
/// alongside the record (issue #493) — read `TypedResponse::header("etag")`
/// off the result to get the value `update_with_response` needs as
/// `If-Match` on an `@version` model. `delete_with_response` needs it too,
/// since cratestack#519: the server enforces `If-Match` on `DELETE` exactly
/// like `PATCH`.
pub(super) fn build_get_with_response_method(
    route_path: &str,
    primary_key_type: &TokenStream,
    model_output_type: &TokenStream,
) -> TokenStream {
    quote! {
        pub async fn get_with_response(
            &self,
            id: &#primary_key_type,
            headers: &[::cratestack::client_rust::HeaderPair<'_>],
        ) -> Result<
            ::cratestack::client_rust::TypedResponse<#model_output_type>,
            ::cratestack::client_rust::ClientError,
        > {
            self.runtime.get_with_response(&format!("{}/{}", #route_path, id), &[], headers).await
        }
    }
}

/// Same call as `update`, but returns the status and response headers
/// alongside the record (issue #493) — on an `@version` model, `headers`
/// must carry `If-Match` (from a prior `get_with_response`), and the
/// response's `ETag` is the value a chained update needs next.
pub(super) fn build_update_with_response_method(
    route_path: &str,
    primary_key_type: &TokenStream,
    update_input_ident: &syn::Ident,
    model_output_type: &TokenStream,
) -> TokenStream {
    quote! {
        pub async fn update_with_response(
            &self,
            id: &#primary_key_type,
            input: &super::inputs::#update_input_ident,
            headers: &[::cratestack::client_rust::HeaderPair<'_>],
        ) -> Result<
            ::cratestack::client_rust::TypedResponse<#model_output_type>,
            ::cratestack::client_rust::ClientError,
        > {
            self.runtime.patch_with_response(&format!("{}/{}", #route_path, id), input, headers).await
        }
    }
}

/// Same call as `delete`, but returns the status and response headers
/// alongside the record (issue #493) — for reading e.g. a `Retry-After` on
/// a `429`, or any other out-of-band signal a server sends on a delete
/// response.
///
/// Part of the `@version` optimistic-locking round trip since
/// cratestack#519: like `update_with_response`, the server requires
/// `If-Match` in `headers` on an `@version` model and returns `412` on a
/// stale or missing value.
pub(super) fn build_delete_with_response_method(
    route_path: &str,
    primary_key_type: &TokenStream,
    model_output_type: &TokenStream,
) -> TokenStream {
    quote! {
        pub async fn delete_with_response(
            &self,
            id: &#primary_key_type,
            headers: &[::cratestack::client_rust::HeaderPair<'_>],
        ) -> Result<
            ::cratestack::client_rust::TypedResponse<#model_output_type>,
            ::cratestack::client_rust::ClientError,
        > {
            self.runtime.delete_with_response(&format!("{}/{}", #route_path, id), headers).await
        }
    }
}
