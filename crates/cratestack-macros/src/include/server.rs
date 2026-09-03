//! `include_server_schema!` composer — emits the full server surface:
//! sqlx Postgres backend, `Cratestack` runtime, axum router, procedure
//! handlers, events. No rusqlite anywhere in the output.

mod axum_dtos;
mod axum_module;
mod collect;
mod query_guard;
mod rpc_module;
mod runtime;

use proc_macro::TokenStream;
use quote::quote;
use syn::LitStr;

use crate::shared::decimal_backend::{DecimalBackend, with_decimal_backend};

use super::decimal_arg::resolve_decimal_backend;
use super::parse::{ServerDb, parse_schema_literal};

use collect::collect_server_schema;

pub(super) fn compose_server_schema(
    schema_path: &LitStr,
    db: ServerDb,
    decimal: Option<DecimalBackend>,
) -> TokenStream {
    let (schema_relative, resolved, schema, schema_sha256) = match parse_schema_literal(schema_path)
    {
        Ok(parsed) => parsed,
        Err(error) => return error,
    };
    if let Err(error) =
        super::datasource_guard::guard_server_datasource_provider(schema_path, &schema, db)
    {
        return error;
    }
    if let Err(error) = super::datasource_guard::guard_server_postgres_backend(schema_path, db) {
        return error;
    }
    if let Err(error) =
        super::extension_gate::guard_server_declared_extensions(schema_path, &schema)
    {
        return error;
    }
    if let Err(error) = query_guard::guard_no_queries_without_a_database(schema_path, &schema, db) {
        return error;
    }
    let decimal_backend = match resolve_decimal_backend(schema_path, &schema, decimal) {
        Ok(backend) => backend,
        Err(error) => return error,
    };

    // Wraps the rest of composition — `collect_server_schema` (which
    // reaches every one of the six `Decimal`-emitting codegen sites) needs
    // the schema's chosen decimal backend in scope (cratestack#505
    // Direction 2; see `include::embedded`'s matching comment).
    with_decimal_backend(decimal_backend, move || {
        let resolved_literal = resolved.display().to_string();

        let collected = match collect_server_schema(&schema, schema_path) {
            Ok(collected) => collected,
            Err(error) => return error,
        };

        let axum_module = axum_module::build_axum_module(&collected, db);
        let runtime_block = runtime::build_runtime_block(
            db,
            &collected.model_accessors,
            &collected.bound_model_accessors,
            &collected.view_accessors,
            &collected.query_accessors,
        );

        // Destructure here for `quote!` interpolation — quoting through the
        // struct adds a `c.` prefix per field, which `quote!` doesn't accept.
        let collect::ServerCollected {
            transport_style_str,
            mixin_names,
            model_names,
            type_names,
            enum_names,
            procedure_names,
            view_names,
            type_structs,
            enum_types,
            computed_field_descriptors,
            computed_field_resolver_methods,
            wire_structs,
            model_structs,
            pg_from_row_impls,
            primary_key_accessor_impls,
            model_descriptors,
            field_modules,
            create_input_structs,
            update_input_structs,
            upsert_input_impls,
            find_many_input_structs,
            view_structs,
            view_descriptors,
            view_pg_from_row_impls,
            query_modules,
            query_from_row_impls,
            query_accessors,
            procedure_modules,
            procedure_registry_methods,
            generated_client_module,
            generated_event_module,
            ..
        } = collected;

        // `datasource { provider = "none" }` schemas can never declare a
        // `model` (cratestack#327's guard), so `generated_event_module` is
        // always an empty-bodied `pub mod events { ... }` shell under
        // `db = None` — a `Subscriptions::new` that nothing in the crate ever
        // calls (there is no `Cratestack::events()` accessor for `db = None`;
        // see `runtime::none`'s module doc), which would trip `dead_code`
        // under this workspace's `-D warnings` gate. Drop it entirely instead
        // of keeping it as unreachable API surface.
        let generated_event_module = match db {
            ServerDb::Postgres => generated_event_module,
            ServerDb::None => proc_macro2::TokenStream::new(),
        };

        // `datasource { provider = "none" }` schemas can never declare a
        // `model` either, so `pg_from_row_impls`/`model_structs`/etc. below
        // are always empty under `db = None` — but the `use ::cratestack::sqlx;`
        // import itself would still fail to resolve once `sqlx`/`cratestack-sqlx`
        // is Cargo-feature-gated behind the (default-on) `postgres` feature
        // (cratestack#329) and a `db = None`-only consumer disables it. Only
        // pull the import in for `db = Postgres`, where it's actually needed
        // for the sqlx `FromRow` impls in this same module.
        let models_sqlx_import = match db {
            ServerDb::Postgres => quote! { use ::cratestack::sqlx; },
            ServerDb::None => proc_macro2::TokenStream::new(),
        };

        // A schema with no `@computed` fields at all gets a generated
        // `impl ComputedFieldResolver for ()` so callers can pass `()` as
        // `router()`'s resolver argument instead of hand-writing an empty
        // impl of their own (docs/design/computed-fields.md decision 4).
        // Only emitted when the trait truly has no methods — implementing
        // an empty trait for `()` when the trait *does* have methods would
        // be a silent "every computed field always resolves to nothing"
        // trap disguised as ergonomics.
        let unit_computed_resolver_impl = if computed_field_descriptors.is_empty() {
            quote! { impl ComputedFieldResolver for () {} }
        } else {
            proc_macro2::TokenStream::new()
        };

        // The wire-shape mirror of every computed-bearing owner
        // (`crate::computed::wire`), consumed by the embedded self/peer-
        // calling client's decode targets (`crate::client`) so a resolved
        // computed field survives the round trip instead of being
        // silently dropped into the server-side struct shape
        // (`docs/design/computed-fields.md`'s "Exclusions" section).
        // Skipped entirely for a schema with no `@computed` fields at
        // all — `wire_structs` is empty exactly when `bearing` (computed
        // in `collect_server_schema`) is, so this mirrors the
        // `unit_computed_resolver_impl` "zero generated code when there's
        // nothing computed" convention above.
        let wire_module = if wire_structs.is_empty() {
            proc_macro2::TokenStream::new()
        } else {
            quote! {
                pub mod wire {
                    use ::cratestack::serde;

                    #(#wire_structs)*
                }
            }
        };

        // `query` blocks (cratestack#867). Emitted only when the schema
        // declares at least one — the same "zero generated code when
        // there's nothing" convention `wire_module` above follows, and
        // here it also keeps an unused `use ::cratestack::sqlx;` from
        // tripping the workspace's `-D warnings` gate in every schema that
        // declares no queries.
        //
        // The `Queries` accessor struct lives inside this module rather
        // than beside `Views` in the runtime block because its methods
        // reference sibling query modules by bare path; keeping them in one
        // module is what makes `loyalty_fee_summary::Args` resolve without
        // a `super::` prefix that would have to change if the nesting ever
        // moved.
        let queries_module = if query_modules.is_empty() {
            proc_macro2::TokenStream::new()
        } else {
            quote! {
                pub mod queries {
                    //! Declarative custom-SQL reads (`query` blocks).
                    //!
                    //! Server-internal by design: no route, no RPC op id and
                    //! no generated client stub exists for any of these —
                    //! they are reachable only as Rust calls from code
                    //! already running inside this process. See
                    //! `docs/design/declarative-custom-query.md` §5.
                    use ::cratestack::sqlx;

                    #(#query_from_row_impls)*
                    #(#query_modules)*

                    /// Sub-accessor returned by `Cratestack::queries()`.
                    pub struct Queries<'a> {
                        pub(super) db: &'a super::Cratestack,
                    }

                    impl<'a> Queries<'a> {
                        pub(super) fn new(db: &'a super::Cratestack) -> Self {
                            Self { db }
                        }

                        #(#query_accessors)*
                    }
                }
            }
        };

        let expanded = quote! {
            pub mod cratestack_schema {
                pub const SCHEMA_PATH: &str = #schema_relative;
                pub const SCHEMA_SOURCE: &str = include_str!(#resolved_literal);
                /// Hex-encoded SHA-256 of `SCHEMA_SOURCE`'s raw bytes, computed
                /// once at macro-expansion time. Not cryptographic-strength
                /// integrity — it's a drift-detection fingerprint: `axum::router()`
                /// below layers on middleware that compares a client-sent copy of
                /// this value against its own and `tracing::warn!`s on mismatch,
                /// never rejects. See issue #178.
                pub const SCHEMA_SHA256: &str = #schema_sha256;
                pub const MIXINS: &[&str] = &[#(#mixin_names),*];
                pub const MODELS: &[&str] = &[#(#model_names),*];
                pub const TYPES: &[&str] = &[#(#type_names),*];
                pub const ENUMS: &[&str] = &[#(#enum_names),*];
                pub const PROCEDURES: &[&str] = &[#(#procedure_names),*];
                pub const VIEWS: &[&str] = &[#(#view_names),*];

                pub const MIXIN_COUNT: usize = MIXINS.len();
                pub const MODEL_COUNT: usize = MODELS.len();
                pub const TYPE_COUNT: usize = TYPES.len();
                pub const ENUM_COUNT: usize = ENUMS.len();
                pub const PROCEDURE_COUNT: usize = PROCEDURES.len();
                pub const VIEW_COUNT: usize = VIEWS.len();

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
                    #models_sqlx_import

                    #(#model_structs)*
                    #(#pg_from_row_impls)*
                    #(#primary_key_accessor_impls)*
                    #(#model_descriptors)*

                    // View emission (ADR-0003) lives alongside models in
                    // the same `models` module so the view structs share
                    // the same scope as the source models they were
                    // declared `from`. The `runtime.views().<view>()`
                    // accessor reaches into `super::models::<View>` to
                    // construct the `ViewDelegate`.
                    #(#view_structs)*
                    #(#view_pg_from_row_impls)*
                    #(#view_descriptors)*
                }

                pub use models::*;

                #(#field_modules)*

                pub mod inputs {
                    use ::cratestack::serde;

                    #(#create_input_structs)*
                    #(#update_input_structs)*
                    #(#upsert_input_impls)*
                    #(#find_many_input_structs)*
                }

                pub use inputs::*;

                #wire_module

                #generated_client_module
                #generated_event_module
                #queries_module

                pub mod procedures {
                    #(#procedure_modules)*

                    /// The schema's `procedure` declarations, one method each.
                    /// Implement every method as a plain `async fn` — the
                    /// `impl Future<Output = …> + Send` return type below is
                    /// only how the trait spells "your future must be `Send`",
                    /// and an `async fn` in the impl satisfies it directly (the
                    /// compiler checks the `Send` bound on your body's future).
                    pub trait ProcedureRegistry: Clone + Send + Sync + 'static {
                        #(#procedure_registry_methods)*
                    }
                }

                pub mod computed {
                    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
                    pub struct ComputedFieldDescriptor {
                        pub owner: &'static str,
                        pub field: &'static str,
                        pub resolver_method: &'static str,
                        pub params_type: Option<&'static str>,
                    }

                    pub const FIELDS: &[ComputedFieldDescriptor] = &[
                        #(#computed_field_descriptors),*
                    ];

                    pub const FIELD_COUNT: usize = FIELDS.len();

                    pub trait ComputedFieldResolver: Clone + Send + Sync + 'static {
                        #(#computed_field_resolver_methods)*
                    }

                    #unit_computed_resolver_impl
                }

                pub use computed::ComputedFieldResolver;

                pub const COMPUTED_FIELDS: &[computed::ComputedFieldDescriptor] = computed::FIELDS;
                pub const COMPUTED_FIELD_COUNT: usize = computed::FIELD_COUNT;

                #axum_module

                #runtime_block
            }
        };

        expanded.into()
    })
}
