//! RPC sub-module emitted inside `pub mod axum { ... }` when the
//! schema declares `transport rpc`. Mounts `POST /rpc/{op_id}` (unary)
//! and `POST /rpc/batch` (sequence of frames). For `transport rest`
//! schemas the returned TokenStream is empty.

mod batch;

use quote::quote;

pub(super) fn build_rpc_module(
    is_rpc: bool,
    rpc_dispatch_arms: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    if !is_rpc {
        return quote! {};
    }

    let dispatch_block = build_dispatch_block(rpc_dispatch_arms);
    let batch_block = batch::build_batch_block();

    quote! {
        #[derive(Clone)]
        pub struct RpcRouterState<R, C, Auth> {
            pub db: super::Cratestack,
            pub registry: R,
            pub codec: C,
            pub auth_provider: Auth,
        }

        /// Encode a `CoolError` raised inside an RPC dispatch arm using
        /// the request's codec.
        fn rpc_dispatch_error<R, C, Auth>(
            state: &RpcRouterState<R, C, Auth>,
            headers: &::cratestack::axum::http::HeaderMap,
            error: ::cratestack::CoolError,
        ) -> ::cratestack::axum::response::Response
        where
            C: HttpTransport,
        {
            ::cratestack::rpc::encode_rpc_error(&state.codec, headers, &error)
        }

        #dispatch_block
        #batch_block

        /// Build the RPC router for `transport rpc` schemas. Mounts
        /// `POST /rpc/{op_id}` (unary) and `POST /rpc/batch` (frames).
        pub fn rpc_router<R, C, Auth>(
            db: super::Cratestack,
            registry: R,
            codec: C,
            auth_provider: Auth,
        ) -> axum::Router
        where
            R: super::procedures::ProcedureRegistry,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            let state = RpcRouterState { db, registry, codec, auth_provider };
            axum::Router::new()
                .route(
                    ::cratestack::rpc::RPC_BATCH_PATH,
                    axum::routing::post(rpc_batch_dispatch),
                )
                .route(
                    ::cratestack::rpc::RPC_UNARY_PATH,
                    axum::routing::post(rpc_dispatch),
                )
                .with_state(state)
        }
    }
}

fn build_dispatch_block(arms: &[proc_macro2::TokenStream]) -> proc_macro2::TokenStream {
    quote! {
        /// Per-op dispatch — shared by unary and batch routes.
        /// Handler-emitted error responses (any non-2xx that bubbles
        /// out of the underlying axum handler in `CoolErrorResponse`
        /// REST shape) are post-processed into `RpcErrorBody` shape
        /// before returning, so callers always see one error
        /// vocabulary on the wire.
        async fn rpc_dispatch_inner<R, C, Auth>(
            state: RpcRouterState<R, C, Auth>,
            headers: ::cratestack::axum::http::HeaderMap,
            op_id: &str,
            body: ::cratestack::axum::body::Bytes,
        ) -> ::cratestack::axum::response::Response
        where
            R: super::procedures::ProcedureRegistry,
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
                        &::cratestack::CoolError::NotFound(format!(
                            "unknown RPC op `{other}`",
                        )),
                    );
                }
            };

            ::cratestack::rpc::convert_handler_error_response(
                response, &post_codec, &post_headers,
            ).await
        }

        async fn rpc_dispatch<R, C, Auth>(
            ::cratestack::axum::extract::State(state):
                ::cratestack::axum::extract::State<RpcRouterState<R, C, Auth>>,
            ::cratestack::axum::extract::Path(op_id):
                ::cratestack::axum::extract::Path<String>,
            headers: ::cratestack::axum::http::HeaderMap,
            body: ::cratestack::axum::body::Bytes,
        ) -> ::cratestack::axum::response::Response
        where
            R: super::procedures::ProcedureRegistry,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            rpc_dispatch_inner(state, headers, &op_id, body).await
        }
    }
}
