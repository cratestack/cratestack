//! The REST mount (`GET /<plural>`) of the list route: a thin axum handler
//! that forwards to the shared `#list_dispatch_ident` body in
//! `handlers_list`, passing the REST route path as the canonical identity.

use quote::quote;

use super::prep::ModelHandlerPrep;

pub(super) fn build_list_rest_mount(p: &ModelHandlerPrep) -> proc_macro2::TokenStream {
    let list_handler_ident = &p.list_handler_ident;
    let list_dispatch_ident = &p.list_dispatch_ident;
    let list_route_path = &p.list_route_path;

    quote! {
        // REST mount (`GET /<plural>`): canonical request identity is the REST route path.
        async fn #list_handler_ident<CR, C, Auth>(
            State(state): State<ModelRouterState<CR, C, Auth>>,
            headers: HeaderMap,
            RawQuery(raw_query): RawQuery,
            client_ip_ctx: ClientIpContext,
        ) -> Response
        where
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            let canonical_query = raw_query.clone();
            #list_dispatch_ident(
                state,
                CanonicalRequest {
                    method: "GET",
                    path: #list_route_path,
                    query: canonical_query.as_deref(),
                    body: &[],
                },
                headers,
                client_ip_ctx,
                raw_query,
            ).await
        }
    }
}
