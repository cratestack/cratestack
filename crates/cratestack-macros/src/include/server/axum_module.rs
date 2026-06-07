//! Body of the generated `pub mod axum { ... }` — shared types
//! (selection / list / fetch query DTOs), per-procedure + per-model
//! axum handlers, the `model_router`/`procedure_router`/`router` fns,
//! plus the RPC sub-module when `transport rpc`.

use quote::quote;

use crate::axum::generate_axum_shared_support;

use super::collect::ServerCollected;

pub(super) fn build_axum_module(c: &ServerCollected) -> proc_macro2::TokenStream {
    let procedure_transport_constants = &c.procedure_transport_constants;
    let model_transport_constants = &c.model_transport_constants;
    let route_transport_entries = &c.route_transport_entries;
    let op_descriptor_entries = &c.op_descriptor_entries;
    let procedure_axum_handler_defs = &c.procedure_axum_handler_defs;
    let model_axum_handler_defs = &c.model_axum_handler_defs;
    let procedure_axum_routes = &c.procedure_axum_routes;
    let model_axum_routes = &c.model_axum_routes;
    let axum_shared_support = generate_axum_shared_support();
    let rpc_module = super::rpc_module::build_rpc_module(c.is_rpc, &c.rpc_dispatch_arms);
    let dtos = super::axum_dtos::build_axum_dtos();

    quote! {
        pub mod axum {
            use ::cratestack::AuthProvider;
            use ::cratestack::CoolError;
            use ::cratestack::HttpTransport;
            use ::cratestack::axum;
            use ::cratestack::axum::body::Bytes;
            use ::cratestack::axum::extract::{Path, RawQuery, State};
            use ::cratestack::axum::http::HeaderMap;
            use ::cratestack::axum::response::Response;

            #[derive(Clone)]
            pub struct ProcedureRouterState<R, C, Auth> {
                pub db: super::Cratestack,
                pub registry: R,
                pub codec: C,
                pub auth_provider: Auth,
            }

            #[derive(Clone)]
            pub struct ModelRouterState<C, Auth> {
                pub db: super::Cratestack,
                pub codec: C,
                pub auth_provider: Auth,
            }

            /// The four request components that make up a canonical signed
            /// request. On `transport rest` these are the REST method/path/
            /// query/body; on `transport rpc` they are the ACTUAL rpc request
            /// (`POST /rpc/<op_id>`, no query, the raw frame bytes). A handler's
            /// `_dispatch` fn takes one of these so signature verification and
            /// tracing share a single source of truth that matches the client
            /// byte-for-byte.
            struct CanonicalRequest<'a> {
                method: &'a str,
                path: &'a str,
                query: Option<&'a str>,
                body: &'a [u8],
            }

            fn request_context<'a>(
                method: &'a str,
                path: &'a str,
                query: Option<&'a str>,
                headers: &'a HeaderMap,
                body: &'a [u8],
            ) -> ::cratestack::RequestContext<'a> {
                ::cratestack::RequestContext {
                    method,
                    path,
                    query,
                    headers,
                    body,
                }
            }

            #dtos

            #(#procedure_transport_constants)*
            #(#model_transport_constants)*
            #axum_shared_support

            pub const ROUTE_TRANSPORTS: &[::cratestack::RouteTransportDescriptor] = &[
                #(#route_transport_entries,)*
            ];

            /// RPC op descriptors. Populated only when the schema declares
            /// `transport rpc`; empty otherwise. The two slices
            /// (`ROUTE_TRANSPORTS` and `OPS`) are never both non-empty for a
            /// given schema — see `docs/design/rpc-transport.md`.
            pub const OPS: &[::cratestack::OpDescriptor] = &[
                #(#op_descriptor_entries,)*
            ];

            #(#procedure_axum_handler_defs)*
            #(#model_axum_handler_defs)*

            pub fn model_router<C, Auth>(
                db: super::Cratestack,
                codec: C,
                auth_provider: Auth,
            ) -> axum::Router
            where
                C: HttpTransport,
                Auth: AuthProvider,
            {
                let state = ModelRouterState {
                    db,
                    codec,
                    auth_provider,
                };

                axum::Router::new()
                    #(#model_axum_routes)*
                    .with_state(state)
            }

            pub fn procedure_router<R, C, Auth>(
                db: super::Cratestack,
                registry: R,
                codec: C,
                auth_provider: Auth,
            ) -> axum::Router
            where
                R: super::procedures::ProcedureRegistry,
                C: HttpTransport,
                Auth: AuthProvider,
            {
                let state = ProcedureRouterState {
                    db,
                    registry,
                    codec,
                    auth_provider,
                };

                axum::Router::new()
                    #(#procedure_axum_routes)*
                    .with_state(state)
            }

            pub fn router<R, C, Auth>(
                db: super::Cratestack,
                registry: R,
                codec: C,
                auth_provider: Auth,
            ) -> axum::Router
            where
                R: super::procedures::ProcedureRegistry,
                C: HttpTransport,
                Auth: AuthProvider,
            {
                model_router(db.clone(), codec.clone(), auth_provider.clone())
                    .merge(procedure_router(db, registry, codec, auth_provider))
            }

            #rpc_module
        }
    }
}
