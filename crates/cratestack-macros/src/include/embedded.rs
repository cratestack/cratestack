//! `include_embedded_schema!` composer — emits the embedded ORM
//! surface backed by rusqlite. No sqlx, no axum, no procedures.

use std::collections::BTreeSet;

use proc_macro::TokenStream;
use quote::quote;
use syn::LitStr;

use crate::model::{
    generate_create_input_struct, generate_field_module, generate_model_descriptor,
    generate_model_struct_only, generate_primary_key_accessor_impl,
    generate_rusqlite_from_row_impl, generate_update_input_struct, generate_upsert_input_struct,
};
use crate::shared::decimal_backend::{DecimalBackend, with_decimal_backend};
use crate::shared::schema_lit;
use crate::types::{generate_enum_type, generate_type_struct};

use super::decimal_arg::resolve_decimal_backend;
use super::parse::parse_schema_literal;

pub(super) fn compose_embedded_schema(
    schema_path: &LitStr,
    decimal: Option<DecimalBackend>,
) -> TokenStream {
    let (schema_relative, resolved, schema, schema_sha256) = match parse_schema_literal(schema_path)
    {
        Ok(parsed) => parsed,
        Err(error) => return error,
    };
    if let Err(error) =
        super::extension_gate::guard_embedded_declared_extensions(schema_path, &schema)
    {
        return error;
    }
    let decimal_backend = match resolve_decimal_backend(schema_path, &schema, decimal) {
        Ok(backend) => backend,
        Err(error) => return error,
    };

    // The rest of this function's body — every codegen call that might
    // touch a `Decimal` field — runs inside this scope so
    // `current_decimal_type_tokens()` (called deep inside
    // `crate::shared`/`crate::model` composers) resolves against the
    // `decimal = ...` argument this macro invocation was given, rather
    // than a shared Cargo feature (cratestack#505 Direction 2).
    with_decimal_backend(decimal_backend, move || {
        // `@@paged` gates a generated `list` **route**'s response envelope on
        // REST/RPC (`Page<Model>` vs. a bare `Vec<Model>` — see
        // docs/design/idempotency-rate-limit-declarative-surface.md). The
        // embedded composer generates no routes, so there is no per-model
        // wire contract to fix in advance the way there is for those
        // transports — but real pagination is still available: every model's
        // (and view's) `find_many()` builder carries an unconditional
        // `.paginate(PageInput) -> Page<M>` (`cratestack-rusqlite`'s
        // `FindMany::paginate`, backed by a real `COUNT(*)` alongside the
        // paginated `SELECT`), so `@@paged` needs nothing special wired here:
        // it's neither rejected nor required — the same "silently ignored,
        // no per-attribute wiring needed" treatment `@@audit`/`@@emit` below
        // already get.
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

        // View emission (ADR-0003) — embedded composer.
        //
        // Materialized views are a server-only feature (SQLite has no
        // `MATERIALIZED VIEW`). The ADR specifies this is a hard error
        // at expansion time: emit `compile_error!` pointing at the
        // attribute so the developer sees the gating in their IDE.
        for view in &schema.views {
            if view.is_materialized() {
                return syn::Error::new(
                schema_path.span(),
                format!(
                    "view `{}` is `@@materialized` which is not supported on the embedded backend \
                     (SQLite has no materialized views). Either gate this view with a feature \
                     flag or split it into a server-only schema. See ADR-0003 \
                     (cratestack-docs `internals/views-adr.md`) for the rationale.",
                    view.name
                ),
            )
            .to_compile_error()
            .into();
            }
        }

        // Views with only `@@server_sql` (no `@@embedded_sql` / `@@sql`)
        // are backend-specific to Postgres and are silently skipped on
        // the embedded composer — the same way procedures are skipped.
        let view_structs = schema
            .views
            .iter()
            .filter(|view| view.embedded_sql().is_some())
            .map(|view| crate::view::generate_view_struct_only(view, &enum_name_set))
            .collect::<Vec<_>>();
        let view_descriptors = match schema
            .views
            .iter()
            .filter(|view| view.embedded_sql().is_some())
            .map(|view| {
                crate::view::generate_view_descriptor(view, &schema.models, &schema.types, auth)
            })
            .collect::<Result<Vec<_>, String>>()
        {
            Ok(descriptors) => descriptors,
            Err(error) => {
                return syn::Error::new(schema_path.span(), error)
                    .to_compile_error()
                    .into();
            }
        };
        let view_rusqlite_from_row_impls = schema
            .views
            .iter()
            .filter(|view| view.embedded_sql().is_some())
            .map(|view| crate::view::generate_view_rusqlite_from_row_impl(view, &enum_name_set))
            .collect::<Vec<_>>();

        // Procedures are skipped on the embedded path — local apps don't have an
        // RPC surface to call. `@@audit` and `@@emit` directives are silently
        // ignored for v1; see CHANGELOG for the follow-up plan.

        let expanded = quote! {
            pub mod cratestack_schema {
                pub const SCHEMA_PATH: &str = #schema_relative;
                pub const SCHEMA_SOURCE: &str = include_str!(#resolved_literal);
                /// Hex-encoded SHA-256 of `SCHEMA_SOURCE`'s raw bytes, computed
                /// once at macro-expansion time. Not cryptographic-strength
                /// integrity — it's a drift-detection fingerprint: the server's
                /// schema-fingerprint middleware compares a client-sent copy of
                /// this value against its own and `tracing::warn!`s on mismatch,
                /// never rejects. See issue #178.
                pub const SCHEMA_SHA256: &str = #schema_sha256;
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

                    // Embedded view emission — same shape as the server
                    // composer (ADR-0003), minus `@@materialized` which
                    // is rejected upstream by `compile_error!` above.
                    #(#view_structs)*
                    #(#view_rusqlite_from_row_impls)*
                    #(#view_descriptors)*
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
    })
}
