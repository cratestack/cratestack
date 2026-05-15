//! Schema-include composers.
//!
//! Three top-level proc-macros target three deployment shapes (see the
//! 0.3.0 CHANGELOG for context):
//!
//! - [`include_server_schema`] — full server: sqlx Postgres backend,
//!   `Cratestack` runtime, axum router, procedure handlers, events. No
//!   rusqlite anywhere in the output.
//! - [`include_embedded_schema`] — embedded ORM only: rusqlite backend
//!   (works on mobile/desktop and on `wasm32-unknown-unknown` via
//!   `sqlite-wasm-rs`). No sqlx, no axum, no procedures.
//! - [`include_client_schema`] — HTTP client surface: model/input/procedure
//!   stubs for talking to a server over the wire. No DB at all.
//!
//! All three emit a `cratestack_schema` module — the schemas are
//! mutually-exclusive within a single crate. Pick one per crate based on its
//! role.

use std::collections::BTreeSet;
use std::path::PathBuf;

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{LitStr, Token, parse_macro_input};

use crate::axum::{
    generate_axum_shared_support, generate_model_axum_handlers, generate_model_axum_routes,
    generate_procedure_axum_handler, generate_procedure_axum_route,
};
use crate::client::generate_generated_client_module;
use crate::event::generate_event_module;
use crate::model::{
    generate_bound_model_accessor, generate_client_create_input_struct,
    generate_client_model_struct, generate_client_update_input_struct,
    generate_create_input_struct, generate_field_module, generate_model_accessor,
    generate_model_descriptor, generate_model_struct_only, generate_pg_from_row_impl,
    generate_primary_key_accessor_impl, generate_rusqlite_from_row_impl,
    generate_update_input_struct, generate_upsert_input_struct,
};
use crate::procedure::{
    generate_client_procedure_module, generate_procedure_module, generate_procedure_registry_method,
};
use crate::shared::schema_lit;
use crate::transport::{
    generate_model_op_descriptors, generate_model_rpc_dispatch_arms,
    generate_model_transport_constants, generate_model_transport_entries,
    generate_procedure_op_descriptor, generate_procedure_rpc_dispatch_arm,
    generate_procedure_transport_constants, generate_procedure_transport_entries,
};
use crate::types::{
    generate_client_enum_type, generate_client_type_struct, generate_custom_field_descriptors,
    generate_custom_field_resolver_methods, generate_enum_type, generate_type_struct,
};

/// Supported sqlx database backends for [`include_server_schema`].
///
/// Today only `Postgres` is accepted; the parser is wired so adding `MySql` /
/// `Sqlite`-via-sqlx (when we want them) is a non-breaking change at call sites
/// that already pass `db = Postgres`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerDb {
    Postgres,
}

/// Parsed arguments for `include_server_schema!("schema.cstack", db = Postgres)`.
struct ServerSchemaArgs {
    schema_path: LitStr,
    db: ServerDb,
}

impl Parse for ServerSchemaArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let schema_path: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;
        let key: syn::Ident = input.parse()?;
        if key != "db" {
            return Err(syn::Error::new(
                key.span(),
                "expected `db = Postgres` (only the `db` argument is recognised)",
            ));
        }
        input.parse::<Token![=]>()?;
        let value: syn::Ident = input.parse()?;
        let db = match value.to_string().as_str() {
            "Postgres" => ServerDb::Postgres,
            other => {
                return Err(syn::Error::new(
                    value.span(),
                    format!(
                        "unsupported db backend `{other}`. supported: Postgres. (MySql / sqlite-via-sqlx will land in a future release.)"
                    ),
                ));
            }
        };
        Ok(Self { schema_path, db })
    }
}

pub(crate) fn include_server_schema(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as ServerSchemaArgs);
    let _ = args.db; // Postgres-only today; reserved for future backends.
    compose_server_schema(&args.schema_path)
}

pub(crate) fn include_embedded_schema(input: TokenStream) -> TokenStream {
    let schema_path = parse_macro_input!(input as LitStr);
    compose_embedded_schema(&schema_path)
}

pub(crate) fn include_client_schema(input: TokenStream) -> TokenStream {
    let schema_path = parse_macro_input!(input as LitStr);
    compose_client_schema(&schema_path)
}

fn compose_server_schema(schema_path: &LitStr) -> TokenStream {
    let (schema_relative, resolved, schema) = match parse_schema_literal(schema_path) {
        Ok(parsed) => parsed,
        Err(error) => return error,
    };
    let resolved_literal = resolved.display().to_string();

    let mixin_names = schema.mixins.iter().map(|mixin| schema_lit(&mixin.name));
    let model_names = schema.models.iter().map(|model| schema_lit(&model.name));
    let model_name_set = schema
        .models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<BTreeSet<_>>();
    let type_names = schema.types.iter().map(|ty| schema_lit(&ty.name));
    let enum_names = schema
        .enums
        .iter()
        .map(|enum_decl| schema_lit(&enum_decl.name));
    let enum_name_set = crate::shared::enum_name_set(&schema.enums);
    let procedure_names = schema
        .procedures
        .iter()
        .map(|procedure| schema_lit(&procedure.name));
    let type_structs = schema
        .types
        .iter()
        .map(|ty| generate_type_struct(ty, &enum_name_set));
    let enum_types = schema.enums.iter().map(generate_enum_type);
    let custom_field_descriptors = schema
        .types
        .iter()
        .flat_map(|ty| generate_custom_field_descriptors(ty).into_iter());
    let custom_field_resolver_methods = schema
        .types
        .iter()
        .flat_map(|ty| generate_custom_field_resolver_methods(ty).into_iter());
    let model_structs = schema
        .models
        .iter()
        .map(|model| generate_model_struct_only(model, &model_name_set, &enum_name_set));
    let pg_from_row_impls = schema
        .models
        .iter()
        .map(|model| generate_pg_from_row_impl(model, &model_name_set, &enum_name_set));
    let primary_key_accessor_impls = schema
        .models
        .iter()
        .map(generate_primary_key_accessor_impl)
        .collect::<Vec<_>>();
    let auth = schema.auth.as_ref();
    let model_descriptors = match schema
        .models
        .iter()
        .map(|model| generate_model_descriptor(model, &schema.models, &schema.types, auth))
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(descriptors) => descriptors,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let field_modules = match schema
        .models
        .iter()
        .map(|model| generate_field_module(model, &model_name_set, &schema.models))
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(field_modules) => field_modules,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let create_input_structs = schema
        .models
        .iter()
        .map(|model| generate_create_input_struct(model, &model_name_set, &enum_name_set));
    let update_input_structs = schema
        .models
        .iter()
        .map(|model| generate_update_input_struct(model, &model_name_set, &enum_name_set));
    let upsert_input_impls = schema
        .models
        .iter()
        .map(|model| generate_upsert_input_struct(model, &model_name_set, &enum_name_set))
        .collect::<Vec<_>>();
    let model_accessors = schema.models.iter().map(generate_model_accessor);
    let bound_model_accessors = schema.models.iter().map(generate_bound_model_accessor);
    let procedure_modules = match schema
        .procedures
        .iter()
        .map(|procedure| {
            generate_procedure_module(
                procedure,
                &schema.models,
                &schema.types,
                &enum_name_set,
                auth,
            )
        })
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(modules) => modules,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let procedure_registry_methods = match schema
        .procedures
        .iter()
        .map(generate_procedure_registry_method)
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(methods) => methods,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let procedure_axum_handler_defs = match schema
        .procedures
        .iter()
        .map(generate_procedure_axum_handler)
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(handlers) => handlers,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let procedure_axum_routes = schema.procedures.iter().map(generate_procedure_axum_route);
    let procedure_transport_constants = match schema
        .procedures
        .iter()
        .map(generate_procedure_transport_constants)
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(constants) => constants,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let procedure_transport_entries = schema
        .procedures
        .iter()
        .map(generate_procedure_transport_entries)
        .collect::<Vec<_>>();
    let axum_shared_support = generate_axum_shared_support();
    let model_axum_handler_defs = match schema
        .models
        .iter()
        .map(|model| generate_model_axum_handlers(model, &schema.models))
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(handlers) => handlers,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let model_axum_routes = schema.models.iter().map(generate_model_axum_routes);
    let model_transport_constants = schema
        .models
        .iter()
        .map(generate_model_transport_constants)
        .collect::<Vec<_>>();
    let model_transport_entries = schema
        .models
        .iter()
        .flat_map(generate_model_transport_entries)
        .collect::<Vec<_>>();

    // RPC op descriptors — see docs/design/rpc-transport.md.
    //
    // The schema's `transport` directive picks which slice is populated at
    // emission time. Both consts are always emitted (so downstream code can
    // introspect uniformly), but exactly one is non-empty per schema.
    let is_rpc = matches!(
        schema.transport,
        ::cratestack_core::TransportStyle::Rpc,
    );
    let auth_required_default = schema.auth.is_some();
    let transport_style_str = schema.transport.as_str();
    let (op_descriptor_entries, route_transport_entries): (
        Vec<proc_macro2::TokenStream>,
        Vec<proc_macro2::TokenStream>,
    ) = if is_rpc {
        let mut ops: Vec<proc_macro2::TokenStream> = Vec::new();
        for procedure in &schema.procedures {
            ops.push(generate_procedure_op_descriptor(
                procedure,
                auth_required_default,
            ));
        }
        for model in &schema.models {
            ops.extend(generate_model_op_descriptors(model, auth_required_default));
        }
        (ops, Vec::new())
    } else {
        let mut routes: Vec<proc_macro2::TokenStream> = Vec::new();
        routes.extend(procedure_transport_entries.iter().cloned());
        routes.extend(model_transport_entries.iter().cloned());
        (Vec::new(), routes)
    };

    // RPC dispatch arms — emitted only when `transport rpc`, otherwise an
    // empty vec collapses the `match` to a single fallback arm. The
    // `rpc_router` / `rpc_dispatch` fns are also gated on `is_rpc` below.
    let rpc_dispatch_arms: Vec<proc_macro2::TokenStream> = if is_rpc {
        let mut arms: Vec<proc_macro2::TokenStream> = Vec::new();
        for procedure in &schema.procedures {
            arms.push(generate_procedure_rpc_dispatch_arm(procedure));
        }
        for model in &schema.models {
            arms.extend(generate_model_rpc_dispatch_arms(model));
        }
        arms
    } else {
        Vec::new()
    };
    let rpc_module = if is_rpc {
        quote! {
            #[derive(Clone)]
            pub struct RpcRouterState<R, C, Auth> {
                pub db: super::Cratestack,
                pub registry: R,
                pub codec: C,
                pub auth_provider: Auth,
            }

            /// Encode a `CoolError` raised inside an RPC dispatch arm using
            /// the request's codec. The status code comes from the error;
            /// the body shape is the existing `CoolErrorResponse` REST
            /// form (uppercase codes). Switching to `RpcErrorBody`
            /// (lowercase gRPC-style codes) lands in a follow-up.
            fn rpc_dispatch_error<R, C, Auth>(
                state: &RpcRouterState<R, C, Auth>,
                headers: &::cratestack::axum::http::HeaderMap,
                error: ::cratestack::CoolError,
            ) -> ::cratestack::axum::response::Response
            where
                C: HttpTransport,
            {
                // `success_status` is unused on the error path — pick an
                // arbitrary 2xx; the function uses `error.status_code()`.
                ::cratestack::encode_transport_result_with_status_for::<
                    _,
                    ::cratestack::serde_json::Value,
                >(
                    &state.codec,
                    headers,
                    &::cratestack::rpc::RPC_BINDING_CAPABILITIES,
                    ::cratestack::axum::http::StatusCode::OK,
                    Err(error),
                )
            }

            /// Per-op dispatch — shared by unary and batch routes. Returns
            /// an `axum::Response`; the unary route returns it as-is, the
            /// batch route buffers + decodes it back into a frame.
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
                match op_id {
                    #(#rpc_dispatch_arms)*
                    other => {
                        use ::cratestack::axum::response::IntoResponse;
                        ::cratestack::tracing::warn!(
                            target: "cratestack",
                            cratestack_operation = "rpc_dispatch",
                            cratestack_op_id = other,
                            "unknown RPC op id",
                        );
                        (
                            ::cratestack::axum::http::StatusCode::NOT_FOUND,
                            format!("unknown RPC op `{other}`"),
                        ).into_response()
                    }
                }
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

            /// Batch route — `POST /rpc/batch`. Decodes a sequence of
            /// `RpcRequest` frames, dispatches each through the same
            /// per-op routing as unary, and emits a sequence of
            /// `RpcResponseFrame`s in the same order. Per-frame errors
            /// don't poison the batch; a malformed batch envelope
            /// returns 400. See `docs/design/rpc-transport.md` §3.2.
            async fn rpc_batch_dispatch<R, C, Auth>(
                ::cratestack::axum::extract::State(state):
                    ::cratestack::axum::extract::State<RpcRouterState<R, C, Auth>>,
                headers: ::cratestack::axum::http::HeaderMap,
                body: ::cratestack::axum::body::Bytes,
            ) -> ::cratestack::axum::response::Response
            where
                R: super::procedures::ProcedureRegistry,
                C: HttpTransport,
                Auth: ::cratestack::AuthProvider,
            {
                if headers.get(::cratestack::axum::http::header::CONTENT_TYPE).is_some()
                    && headers
                        .get("idempotency-key")
                        .is_some()
                {
                    return rpc_dispatch_error(
                        &state,
                        &headers,
                        ::cratestack::CoolError::BadRequest(
                            "Idempotency-Key header is not supported on /rpc/batch; \
                             use the per-frame `idem` field instead".to_owned(),
                        ),
                    );
                }

                let frames: Vec<::cratestack::rpc::RpcRequest> =
                    match ::cratestack::__private::decode_rpc_body(&state.codec, &headers, &body) {
                        Ok(frames) => frames,
                        Err(error) => return rpc_dispatch_error(&state, &headers, error),
                    };

                let mut responses: Vec<::cratestack::rpc::RpcResponseFrame> =
                    Vec::with_capacity(frames.len());
                for frame in frames {
                    // Re-encode the frame's opaque `input` value back to
                    // codec bytes so we can route it through the same
                    // dispatcher as unary.
                    let input_bytes = match ::cratestack::__private::encode_rpc_value(
                        &state.codec,
                        &headers,
                        &frame.input,
                    ).await {
                        Ok(bytes) => bytes,
                        Err(error) => {
                            responses.push(::cratestack::rpc::RpcResponseFrame::err(
                                frame.id,
                                &error,
                            ));
                            continue;
                        }
                    };

                    // Per-frame state clone — we can't `move` the original
                    // because the loop owns it.
                    let frame_state = state.clone();
                    let frame_headers = headers.clone();
                    let response = rpc_dispatch_inner(
                        frame_state,
                        frame_headers,
                        &frame.op,
                        ::cratestack::axum::body::Bytes::from(input_bytes),
                    ).await;

                    let frame_result = ::cratestack::rpc::response_to_frame(
                        frame.id,
                        response,
                        &state.codec,
                        &headers,
                    ).await;
                    responses.push(frame_result);
                }

                ::cratestack::encode_transport_result_with_status_for::<
                    _,
                    Vec<::cratestack::rpc::RpcResponseFrame>,
                >(
                    &state.codec,
                    &headers,
                    &::cratestack::rpc::RPC_BINDING_CAPABILITIES,
                    ::cratestack::axum::http::StatusCode::OK,
                    Ok(responses),
                )
            }

            /// Build the RPC router for `transport rpc` schemas. Mounts:
            /// - `POST /rpc/{op_id}` — unary
            /// - `POST /rpc/batch` — sequence of frames
            ///
            /// WS / streaming bindings follow in subsequent patches.
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
                let state = RpcRouterState {
                    db,
                    registry,
                    codec,
                    auth_provider,
                };
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
    } else {
        quote! {}
    };

    let generated_client_module =
        match generate_generated_client_module(&schema.models, &schema.procedures) {
            Ok(module) => module,
            Err(error) => {
                return syn::Error::new(schema_path.span(), error)
                    .to_compile_error()
                    .into();
            }
        };
    let generated_event_module = match generate_event_module(&schema.models) {
        Ok(module) => module,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };

    let expanded = quote! {
        pub mod cratestack_schema {
            pub const SCHEMA_PATH: &str = #schema_relative;
            pub const SCHEMA_SOURCE: &str = include_str!(#resolved_literal);
            pub const MIXINS: &[&str] = &[#(#mixin_names),*];
            pub const MODELS: &[&str] = &[#(#model_names),*];
            pub const TYPES: &[&str] = &[#(#type_names),*];
            pub const ENUMS: &[&str] = &[#(#enum_names),*];
            pub const PROCEDURES: &[&str] = &[#(#procedure_names),*];

            pub const MIXIN_COUNT: usize = MIXINS.len();
            pub const MODEL_COUNT: usize = MODELS.len();
            pub const TYPE_COUNT: usize = TYPES.len();
            pub const ENUM_COUNT: usize = ENUMS.len();
            pub const PROCEDURE_COUNT: usize = PROCEDURES.len();

            /// Generation style the schema declared via the `transport`
            /// directive. Either `"rest"` (the default) or `"rpc"`. See
            /// `docs/design/rpc-transport.md`.
            pub const TRANSPORT_STYLE: &str = #transport_style_str;

            pub mod types {
                use ::cratestack::serde;

                #(#enum_types)*
                #(#type_structs)*
            }

            pub use types::*;

            pub mod models {
                use ::cratestack::serde;
                use ::cratestack::sqlx;

                #(#model_structs)*
                #(#pg_from_row_impls)*
                #(#primary_key_accessor_impls)*
                #(#model_descriptors)*
            }

            pub use models::*;

            #(#field_modules)*

            pub mod inputs {
                use ::cratestack::serde;

                #(#create_input_structs)*
                #(#update_input_structs)*
                #(#upsert_input_impls)*
            }

            pub use inputs::*;

            #generated_client_module
            #generated_event_module

            pub mod procedures {
                #(#procedure_modules)*

                pub trait ProcedureRegistry: Clone + Send + Sync + 'static {
                    #(#procedure_registry_methods)*
                }
            }

            pub mod custom {
                #[derive(Debug, Clone, Copy, PartialEq, Eq)]
                pub struct CustomFieldDescriptor {
                    pub owner: &'static str,
                    pub field: &'static str,
                    pub resolver_method: &'static str,
                }

                pub const FIELDS: &[CustomFieldDescriptor] = &[
                    #(#custom_field_descriptors),*
                ];

                pub const FIELD_COUNT: usize = FIELDS.len();

                pub trait CustomFieldResolver: Clone + Send + Sync + 'static {
                    #(#custom_field_resolver_methods)*
                }
            }

            pub use custom::CustomFieldResolver;

            pub const CUSTOM_FIELDS: &[custom::CustomFieldDescriptor] = custom::FIELDS;
            pub const CUSTOM_FIELD_COUNT: usize = custom::FIELD_COUNT;

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

                #[derive(Debug, Clone, Default)]
                pub struct ModelSelectionQuery {
                    pub fields: Option<Vec<String>>,
                    pub includes: Vec<String>,
                    pub include_fields: ::std::collections::BTreeMap<String, Vec<String>>,
                }

                impl ModelSelectionQuery {
                    fn fields_for_include(&self, include: &str) -> Option<&[String]> {
                        self.include_fields.get(include).map(Vec::as_slice)
                    }

                    fn direct_includes(&self) -> Vec<String> {
                        let mut includes = Vec::new();
                        for include in &self.includes {
                            let direct = include.split('.').next().unwrap_or(include).to_owned();
                            if !includes.iter().any(|selected| selected == &direct) {
                                includes.push(direct);
                            }
                        }
                        includes
                    }

                    fn selection_for_include(&self, include: &str) -> Option<Self> {
                        let mut selection = Self::default();
                        if let Some(fields) = self.include_fields.get(include) {
                            selection.fields = Some(fields.clone());
                        }

                        let prefix = format!("{include}.");
                        for selected in &self.includes {
                            if let Some(rest) = selected.strip_prefix(&prefix) {
                                selection.includes.push(rest.to_owned());
                            }
                        }
                        for (path, fields) in &self.include_fields {
                            if let Some(rest) = path.strip_prefix(&prefix) {
                                selection.include_fields.insert(rest.to_owned(), fields.clone());
                            }
                        }

                        if self.includes.iter().any(|selected| selected == include)
                            || selection.fields.is_some()
                            || !selection.includes.is_empty()
                        {
                            Some(selection)
                        } else {
                            None
                        }
                    }
                }

                #[derive(Debug, Clone, Default)]
                pub struct ModelListQuery {
                    pub selection: ModelSelectionQuery,
                    pub limit: Option<i64>,
                    pub offset: Option<i64>,
                    pub sort: Option<String>,
                    pub filters: Vec<::cratestack::QueryExpr>,
                }

                #[derive(Debug, Clone, Default)]
                pub struct ModelFetchQuery {
                    pub selection: ModelSelectionQuery,
                }

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

            #[derive(Clone)]
            pub struct Cratestack {
                runtime: ::cratestack::__private::SqlxRuntime,
            }

            #[derive(Clone)]
            pub struct BoundCratestack<'a> {
                inner: &'a Cratestack,
                ctx: ::cratestack::CoolContext,
            }

            pub struct CratestackBuilder {
                runtime: ::cratestack::__private::SqlxRuntime,
            }

            impl Cratestack {
                pub fn builder(pool: ::cratestack::sqlx::PgPool) -> CratestackBuilder {
                    CratestackBuilder {
                        runtime: ::cratestack::__private::SqlxRuntime::new(pool),
                    }
                }

                pub fn bind_context(&self, ctx: ::cratestack::CoolContext) -> BoundCratestack<'_> {
                    BoundCratestack { inner: self, ctx }
                }

                pub fn pool(&self) -> &::cratestack::sqlx::PgPool {
                    self.runtime.pool()
                }

                pub fn bind_auth<P: ::cratestack::serde::Serialize>(
                    &self,
                    principal: Option<P>,
                ) -> Result<BoundCratestack<'_>, ::cratestack::CoolError> {
                    let ctx = ::cratestack::CoolContext::from_principal(principal)?;
                    Ok(self.bind_context(ctx))
                }

                #(#model_accessors)*

                pub fn events(&self) -> events::Subscriptions<'_> {
                    events::Subscriptions::new(&self.runtime)
                }
            }

            impl<'a> BoundCratestack<'a> {
                pub fn context(&self) -> &::cratestack::CoolContext {
                    &self.ctx
                }

                #(#bound_model_accessors)*
            }

            impl CratestackBuilder {
                pub fn build(self) -> Cratestack {
                    Cratestack {
                        runtime: self.runtime,
                    }
                }
            }

            pub fn schema_summary() -> ::cratestack::SchemaSummary {
                ::cratestack::SchemaSummary {
                    mixins: MIXINS,
                    models: MODELS,
                    types: TYPES,
                    enums: ENUMS,
                    procedures: PROCEDURES,
                }
            }
        }
    };

    expanded.into()
}

fn compose_embedded_schema(schema_path: &LitStr) -> TokenStream {
    let (schema_relative, resolved, schema) = match parse_schema_literal(schema_path) {
        Ok(parsed) => parsed,
        Err(error) => return error,
    };
    let resolved_literal = resolved.display().to_string();

    let mixin_names = schema.mixins.iter().map(|mixin| schema_lit(&mixin.name));
    let model_names = schema.models.iter().map(|model| schema_lit(&model.name));
    let model_name_set = schema
        .models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<BTreeSet<_>>();
    let type_names = schema.types.iter().map(|ty| schema_lit(&ty.name));
    let enum_names = schema
        .enums
        .iter()
        .map(|enum_decl| schema_lit(&enum_decl.name));
    let enum_name_set = crate::shared::enum_name_set(&schema.enums);
    let type_structs = schema
        .types
        .iter()
        .map(|ty| generate_type_struct(ty, &enum_name_set));
    let enum_types = schema.enums.iter().map(generate_enum_type);
    let model_structs = schema
        .models
        .iter()
        .map(|model| generate_model_struct_only(model, &model_name_set, &enum_name_set));
    let rusqlite_from_row_impls = schema
        .models
        .iter()
        .map(|model| generate_rusqlite_from_row_impl(model, &model_name_set, &enum_name_set));
    let primary_key_accessor_impls = schema
        .models
        .iter()
        .map(generate_primary_key_accessor_impl)
        .collect::<Vec<_>>();
    let auth = schema.auth.as_ref();
    let model_descriptors = match schema
        .models
        .iter()
        .map(|model| generate_model_descriptor(model, &schema.models, &schema.types, auth))
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(descriptors) => descriptors,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let field_modules = match schema
        .models
        .iter()
        .map(|model| generate_field_module(model, &model_name_set, &schema.models))
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(field_modules) => field_modules,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let create_input_structs = schema
        .models
        .iter()
        .map(|model| generate_create_input_struct(model, &model_name_set, &enum_name_set));
    let update_input_structs = schema
        .models
        .iter()
        .map(|model| generate_update_input_struct(model, &model_name_set, &enum_name_set));
    let upsert_input_impls = schema
        .models
        .iter()
        .map(|model| generate_upsert_input_struct(model, &model_name_set, &enum_name_set))
        .collect::<Vec<_>>();

    // Procedures are skipped on the embedded path — local apps don't have an
    // RPC surface to call. `@@audit` and `@@emit` directives are silently
    // ignored for v1; see CHANGELOG for the follow-up plan.

    let expanded = quote! {
        pub mod cratestack_schema {
            pub const SCHEMA_PATH: &str = #schema_relative;
            pub const SCHEMA_SOURCE: &str = include_str!(#resolved_literal);
            pub const MIXINS: &[&str] = &[#(#mixin_names),*];
            pub const MODELS: &[&str] = &[#(#model_names),*];
            pub const TYPES: &[&str] = &[#(#type_names),*];
            pub const ENUMS: &[&str] = &[#(#enum_names),*];

            pub const MIXIN_COUNT: usize = MIXINS.len();
            pub const MODEL_COUNT: usize = MODELS.len();
            pub const TYPE_COUNT: usize = TYPES.len();
            pub const ENUM_COUNT: usize = ENUMS.len();

            pub mod types {
                #(#enum_types)*
                #(#type_structs)*
            }

            pub use types::*;

            pub mod models {
                #(#model_structs)*
                #(#rusqlite_from_row_impls)*
                #(#primary_key_accessor_impls)*
                #(#model_descriptors)*
            }

            pub use models::*;

            #(#field_modules)*

            pub mod inputs {
                #(#create_input_structs)*
                #(#update_input_structs)*
                #(#upsert_input_impls)*
            }

            pub use inputs::*;
        }
    };

    expanded.into()
}

fn compose_client_schema(schema_path: &LitStr) -> TokenStream {
    let (schema_relative, resolved, schema) = match parse_schema_literal(schema_path) {
        Ok(parsed) => parsed,
        Err(error) => return error,
    };
    let resolved_literal = resolved.display().to_string();

    let model_names = schema.models.iter().map(|model| schema_lit(&model.name));
    let model_name_set = schema
        .models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<BTreeSet<_>>();
    let type_names = schema.types.iter().map(|ty| schema_lit(&ty.name));
    let enum_names = schema
        .enums
        .iter()
        .map(|enum_decl| schema_lit(&enum_decl.name));
    let enum_name_set = crate::shared::enum_name_set(&schema.enums);
    let procedure_names = schema
        .procedures
        .iter()
        .map(|procedure| schema_lit(&procedure.name));
    let type_structs = schema.types.iter().map(generate_client_type_struct);
    let enum_types = schema.enums.iter().map(generate_client_enum_type);
    let model_structs = schema
        .models
        .iter()
        .map(|model| generate_client_model_struct(model, &model_name_set, &enum_name_set));
    let create_input_structs = schema
        .models
        .iter()
        .map(|model| generate_client_create_input_struct(model, &model_name_set, &enum_name_set));
    let update_input_structs = schema
        .models
        .iter()
        .map(|model| generate_client_update_input_struct(model, &model_name_set, &enum_name_set));
    let field_modules = match schema
        .models
        .iter()
        .map(|model| generate_field_module(model, &model_name_set, &schema.models))
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(field_modules) => field_modules,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let procedure_modules = match schema
        .procedures
        .iter()
        .map(|procedure| generate_client_procedure_module(procedure, &schema.types, &enum_name_set))
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(modules) => modules,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let generated_client_module =
        match generate_generated_client_module(&schema.models, &schema.procedures) {
            Ok(module) => module,
            Err(error) => {
                return syn::Error::new(schema_path.span(), error)
                    .to_compile_error()
                    .into();
            }
        };

    let expanded = quote! {
        pub mod cratestack_schema {
            pub const SCHEMA_PATH: &str = #schema_relative;
            pub const SCHEMA_SOURCE: &str = include_str!(#resolved_literal);
            pub const MODELS: &[&str] = &[#(#model_names),*];
            pub const TYPES: &[&str] = &[#(#type_names),*];
            pub const ENUMS: &[&str] = &[#(#enum_names),*];
            pub const PROCEDURES: &[&str] = &[#(#procedure_names),*];

            pub const MODEL_COUNT: usize = MODELS.len();
            pub const TYPE_COUNT: usize = TYPES.len();
            pub const ENUM_COUNT: usize = ENUMS.len();
            pub const PROCEDURE_COUNT: usize = PROCEDURES.len();

            pub mod types {
                use ::cratestack::serde;

                #(#enum_types)*
                #(#type_structs)*
            }

            pub use types::*;

            pub mod models {
                use ::cratestack::serde;

                #(#model_structs)*
            }

            pub use models::*;

            #(#field_modules)*

            pub mod inputs {
                use ::cratestack::serde;

                #(#create_input_structs)*
                #(#update_input_structs)*
            }

            pub use inputs::*;

            #generated_client_module

            pub mod procedures {
                use ::cratestack::serde;

                #(#procedure_modules)*
            }
        }
    };

    expanded.into()
}

fn parse_schema_literal(
    schema_path: &LitStr,
) -> Result<(String, PathBuf, cratestack_core::Schema), TokenStream> {
    let schema_relative = schema_path.value();
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let resolved = PathBuf::from(&manifest_dir).join(&schema_relative);
    let source = std::fs::read_to_string(&resolved).map_err(|error| {
        TokenStream::from(
            syn::Error::new(
                schema_path.span(),
                format!("failed to read schema file {}: {error}", resolved.display()),
            )
            .to_compile_error(),
        )
    })?;

    let schema = cratestack_parser::parse_schema_named(&resolved.display().to_string(), &source)
        .map_err(|error| {
            TokenStream::from(
                syn::Error::new(
                    schema_path.span(),
                    error.render(&resolved.display().to_string(), &source),
                )
                .to_compile_error(),
            )
        })?;

    Ok((schema_relative, resolved, schema))
}
