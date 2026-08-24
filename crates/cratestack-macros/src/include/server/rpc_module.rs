//! RPC sub-module emitted inside `pub mod axum { ... }` when the
//! schema declares `transport rpc`. Mounts `POST /rpc/{op_id}` (unary),
//! `POST /rpc/batch` (sequence of frames), and `GET /rpc/subscribe/
//! {op_id}` (SSE subscriptions, §3.4a). For `transport rest` schemas
//! the returned TokenStream is empty.

mod batch;
mod subscribe;

use quote::quote;

pub(super) fn build_rpc_module(
    is_rpc: bool,
    rpc_dispatch_arms: &[proc_macro2::TokenStream],
    rpc_subscribe_dispatch_arms: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    if !is_rpc {
        return quote! {};
    }

    let dispatch_block = build_dispatch_block(rpc_dispatch_arms);
    let batch_block = batch::build_batch_block();
    let subscribe_block = subscribe::build_subscribe_block(rpc_subscribe_dispatch_arms);

    quote! {
        #[derive(Clone)]
        pub struct RpcRouterState<R, CR, C, Auth> {
            pub db: super::Cratestack,
            pub registry: R,
            pub resolvers: CR,
            pub codec: C,
            pub auth_provider: Auth,
        }

        /// Encode a `CratestackError` raised inside an RPC dispatch arm using
        /// the request's codec.
        fn rpc_dispatch_error<R, CR, C, Auth>(
            state: &RpcRouterState<R, CR, C, Auth>,
            headers: &::cratestack::axum::http::HeaderMap,
            error: ::cratestack::CratestackError,
        ) -> ::cratestack::axum::response::Response
        where
            C: HttpTransport,
        {
            ::cratestack::rpc::encode_rpc_error(&state.codec, headers, &error)
        }

        #dispatch_block
        #batch_block
        #subscribe_block

        /// Build the RPC router for `transport rpc` schemas. Mounts
        /// `POST /rpc/{op_id}` (unary), `POST /rpc/batch` (frames), and
        /// `GET /rpc/subscribe/{op_id}` (SSE subscriptions, §3.4a).
        ///
        /// `body_limit_bytes` (cratestack#413) is applied once as the
        /// outermost `DefaultBodyLimit` layer — see
        /// `axum_module/router_fn.rs`'s module doc for why this has to be
        /// a real parameter rather than a default a consumer re-layers on
        /// top of.
        pub fn rpc_router<R, CR, C, Auth>(
            db: super::Cratestack,
            registry: R,
            resolvers: CR,
            codec: C,
            auth_provider: Auth,
            body_limit_bytes: usize,
        ) -> axum::Router
        where
            R: super::procedures::ProcedureRegistry,
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            let state = RpcRouterState { db, registry, resolvers, codec, auth_provider };
            axum::Router::new()
                .route(
                    ::cratestack::rpc::RPC_BATCH_PATH,
                    axum::routing::post(rpc_batch_dispatch),
                )
                .route(
                    ::cratestack::rpc::RPC_SUBSCRIBE_PATH,
                    axum::routing::get(rpc_subscribe_dispatch),
                )
                .route(
                    ::cratestack::rpc::RPC_UNARY_PATH,
                    axum::routing::post(rpc_dispatch),
                )
                .layer(::cratestack::axum::extract::DefaultBodyLimit::max(body_limit_bytes))
                .with_state(state)
        }
    }
}

fn build_dispatch_block(arms: &[proc_macro2::TokenStream]) -> proc_macro2::TokenStream {
    quote! {
        /// Per-op dispatch — shared by unary and batch routes.
        /// Handler-emitted error responses (any non-2xx that bubbles
        /// out of the underlying axum handler in `CratestackErrorResponse`
        /// REST shape) are post-processed into `RpcErrorBody` shape
        /// before returning, so callers always see one error
        /// vocabulary on the wire.
        async fn rpc_dispatch_inner<R, CR, C, Auth>(
            state: RpcRouterState<R, CR, C, Auth>,
            headers: ::cratestack::axum::http::HeaderMap,
            op_id: &str,
            body: ::cratestack::axum::body::Bytes,
            client_ip_ctx: ClientIpContext,
        ) -> ::cratestack::axum::response::Response
        where
            R: super::procedures::ProcedureRegistry,
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            // Hold a codec + headers reference for post-processing.
            let post_codec = state.codec.clone();
            let post_headers = headers.clone();

            let response = match op_id {
                #(#arms)*
                other => {
                    ::cratestack::tracing::warn!(
                        target: "cratestack",
                        cratestack_operation = "rpc_dispatch",
                        cratestack_op_id = other,
                        "unknown RPC op id",
                    );
                    return ::cratestack::rpc::encode_rpc_error(
                        &post_codec,
                        &post_headers,
                        &::cratestack::CratestackError::NotFound(format!(
                            "unknown RPC op `{other}`",
                        )),
                    );
                }
            };

            ::cratestack::rpc::convert_handler_error_response(
                response, &post_codec, &post_headers,
            ).await
        }

        async fn rpc_dispatch<R, CR, C, Auth>(
            ::cratestack::axum::extract::State(state):
                ::cratestack::axum::extract::State<RpcRouterState<R, CR, C, Auth>>,
            ::cratestack::axum::extract::Path(op_id):
                ::cratestack::axum::extract::Path<String>,
            headers: ::cratestack::axum::http::HeaderMap,
            client_ip_ctx: ClientIpContext,
            body: ::cratestack::axum::body::Bytes,
        ) -> ::cratestack::axum::response::Response
        where
            R: super::procedures::ProcedureRegistry,
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            rpc_dispatch_inner(state, headers, &op_id, body, client_ip_ctx).await
        }
    }
}
