//! Body of the generated `pub mod axum { ... }` — shared types
//! (selection / list / fetch query DTOs), per-procedure + per-model
//! axum handlers, the `model_router`/`procedure_router`/`router` fns,
//! plus the RPC sub-module when `transport rpc`.

mod model_router;
mod router_fn;
#[cfg(test)]
mod tests;

use quote::quote;

use crate::axum::generate_axum_shared_support;

use super::super::parse::ServerDb;
use super::collect::ServerCollected;

pub(super) fn build_axum_module(c: &ServerCollected, db: ServerDb) -> proc_macro2::TokenStream {
    let procedure_transport_constants = &c.procedure_transport_constants;
    let model_transport_constants = &c.model_transport_constants;
    let route_transport_entries = &c.route_transport_entries;
    let op_descriptor_entries = &c.op_descriptor_entries;
    let procedure_axum_handler_defs = &c.procedure_axum_handler_defs;
    let model_axum_handler_defs = &c.model_axum_handler_defs;
    let procedure_axum_routes = &c.procedure_axum_routes;
    let axum_shared_support = generate_axum_shared_support();
    let rpc_module = super::rpc_module::build_rpc_module(
        c.is_rpc,
        &c.rpc_dispatch_arms,
        &c.rpc_subscribe_dispatch_arms,
    );
    let dtos = super::axum_dtos::build_axum_dtos();

    // `ModelRouterState`/`model_router` (cratestack#328): emitted only
    // under `db = Postgres`. `datasource { provider = "none" }` schemas
    // can never declare a `model` (cratestack#327's guard), so under
    // `db = None` these would be provably dead — a struct/fn nothing
    // could ever construct or call. `model_router::build_state` /
    // `model_router::build_fn` return empty token streams for
    // `ServerDb::None` so the items compile out entirely instead of
    // existing as unused generic code.
    let model_router_state = model_router::build_state(db);
    let model_router_fn = model_router::build_fn(db, &c.model_axum_routes);
    let router_fn = router_fn::build(db);

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

            #model_router_state

            /// The four request components that make up a canonical signed
            /// request. On `transport rest` these are the REST method/path/
            /// query/body; on `transport rpc` they are the ACTUAL rpc request
            /// (`POST /rpc/<op_id>`, no query, the raw frame bytes). A handler's
            /// `_dispatch` fn takes one of these so signature verification and
            /// tracing share a single source of truth that matches the client
            /// byte-for-byte.
            // `pub(super)` (not private): the gRPC service module
            // (`super::grpc`, a sibling of this `axum` module, emitted only
            // for `transport grpc` schemas under the `grpc` Cargo feature —
            // see `crates/cratestack-macros/src/include/server/grpc/`)
            // constructs `CanonicalRequest` and calls the `_dispatch` fns
            // below directly, so gRPC method bodies delegate to the exact
            // same dispatch functions REST/RPC already call — "no second
            // dispatch path" (ticket #171 AC). `pub(super)` keeps them
            // unreachable from outside the generated `cratestack_schema`
            // module entirely (schema authors never see these).
            pub(super) struct CanonicalRequest<'a> {
                pub(super) method: &'a str,
                pub(super) path: &'a str,
                pub(super) query: Option<&'a str>,
                pub(super) body: &'a [u8],
            }

            pub(super) fn request_context<'a>(
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

            #model_router_fn

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
                    .layer(::cratestack::axum::middleware::from_fn_with_state(
                        super::SCHEMA_SHA256,
                        ::cratestack::schema_fingerprint::warn_on_schema_mismatch,
                    ))
                    .with_state(state)
            }

            #router_fn

            #rpc_module
        }
    }
}
