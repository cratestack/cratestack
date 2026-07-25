//! Guards `transport grpc` schemas in and out of the Rust codegen path —
//! split out of `parse.rs` (which stays focused on entry-macro argument
//! parsing + the shared schema loader) per the repo's 200-LoC file
//! convention.
//!
//! Two independent guards, called from the two places that actually differ
//! (previously this was one guard shared unconditionally by all three
//! entry macros via `parse::parse_schema_literal`; ticket #171 split it,
//! because "no runtime yet" stopped being true for exactly one of the
//! three):
//!
//! - [`guard_server_grpc_transport`] — `include_server_schema!` only. A
//!   `Grpc` schema now compiles for real *if* this crate's own `grpc`
//!   Cargo feature is on (forwarded from `cratestack-pg`); if it's off,
//!   this still rejects with `compile_error!`, but the message says
//!   "enable the feature", not "not implemented at all".
//! - [`guard_client_or_embedded_grpc_transport`] — `include_client_schema!`
//!   and `include_embedded_schema!`. Unconditional reject, feature or no
//!   feature: no Rust gRPC client codegen exists yet (a separate future
//!   ticket, not #172 either — #172 is gRPC-Web/TypeScript), and the
//!   embedded role has no transport at all.
//!
//! Without either guard, a `Grpc` schema would silently fall through
//! `collect.rs`'s `is_rpc` boolean (`matches!(schema.transport,
//! TransportStyle::Rpc)` is `false` for `Grpc` too) into the REST codegen
//! branch, emitting REST routes for a schema that explicitly opted out of
//! REST — wrong-not-broken, and exactly the failure mode these guards exist
//! to close. Same precedent as `parse::reject_composite_primary_keys`.

use proc_macro::TokenStream;
use syn::LitStr;

/// `include_server_schema!` only. See the module doc.
pub(super) fn guard_server_grpc_transport(
    schema_path: &LitStr,
    schema: &cratestack_core::Schema,
) -> Result<(), TokenStream> {
    if !schema_declares_grpc_transport(schema) {
        return Ok(());
    }
    // `cfg!(feature = "grpc")` reads *this crate's* (`cratestack-macros`)
    // own compiled feature set, forwarded from `cratestack-pg`'s `grpc`
    // feature (`grpc = ["cratestack-macros/grpc", "dep:cratestack-grpc"]`)
    // — not the consumer crate's `CARGO_FEATURE_*` env vars, which a
    // proc-macro cannot see (those describe the crate being expanded
    // *into*, not this one). See `docs/design/extensions.md` §2.
    if cfg!(feature = "grpc") {
        return Ok(());
    }
    Err(TokenStream::from(
        syn::Error::new(
            schema_path.span(),
            "schema declares `transport grpc`, but `cratestack-macros` was compiled without \
             its `grpc` Cargo feature — enable it via \
             `cratestack = { package = \"cratestack-pg\", features = [\"grpc\"] }` in your \
             Cargo.toml. Without the feature, `cratestack generate-proto` can still emit this \
             schema's `.proto` contract including its `service` block (tracking: \
             https://github.com/cratestack/cratestack/issues/171).",
        )
        .to_compile_error(),
    ))
}

/// `include_client_schema!` and `include_embedded_schema!`. See the module
/// doc — always rejects a `Grpc` schema, feature or no feature, because no
/// Rust codegen exists for either path.
pub(super) fn guard_client_or_embedded_grpc_transport(
    schema_path: &LitStr,
    schema: &cratestack_core::Schema,
    macro_name: &str,
) -> Result<(), TokenStream> {
    if !schema_declares_grpc_transport(schema) {
        return Ok(());
    }
    Err(TokenStream::from(
        syn::Error::new(
            schema_path.span(),
            format!(
                "schema declares `transport grpc`, but `{macro_name}!` has no gRPC codegen — \
                 only `include_server_schema!` does (behind its `grpc` Cargo feature). A Rust \
                 gRPC client generator and embedded-role gRPC support are not implemented and \
                 not currently tracked as a specific ticket; \
                 https://github.com/cratestack/cratestack/issues/172 covers the browser \
                 (gRPC-Web/TypeScript) client only, not a native Rust one. \
                 `cratestack generate-proto` can still emit this schema's `.proto` contract \
                 today for use with a non-CrateStack gRPC client.",
            ),
        )
        .to_compile_error(),
    ))
}

fn schema_declares_grpc_transport(schema: &cratestack_core::Schema) -> bool {
    schema.transport == cratestack_core::TransportStyle::Grpc
}

#[cfg(test)]
mod tests {
    use super::schema_declares_grpc_transport;

    // The guards themselves are deliberately not exercised directly here:
    // they return `proc_macro::TokenStream`, whose conversion from
    // `proc_macro2::TokenStream` (via `syn::Error::to_compile_error()`)
    // requires an active proc-macro invocation context and panics
    // ("procedural macro API is used outside of a procedural macro") in a
    // plain `cargo test` run. `parse::reject_composite_primary_keys` hits
    // the same constraint and, for the same reason, is only tested through
    // its pure `find_composite_id_model` predicate — this mirrors that
    // precedent via `schema_declares_grpc_transport`, the pure condition
    // both guards branch on. The `cfg!(feature = "grpc")` branch inside
    // `guard_server_grpc_transport` is exercised by CI running this crate's
    // test suite twice — with and without `--features grpc` — per the
    // ticket's verification checklist; the boolean itself has no logic
    // worth unit-testing beyond "reads the feature flag", which `cfg!` is
    // trusted to do correctly.
    #[test]
    fn flags_grpc_transport_schema() {
        let schema = cratestack_parser::parse_schema(
            r#"
transport grpc

model Widget {
  id Int @id
}
"#,
        )
        .expect("schema should parse");

        assert!(schema_declares_grpc_transport(&schema));
    }

    #[test]
    fn does_not_flag_rest_transport_schema() {
        let schema = cratestack_parser::parse_schema(
            r#"
model Widget {
  id Int @id
}
"#,
        )
        .expect("schema should parse");

        assert!(!schema_declares_grpc_transport(&schema));
    }

    #[test]
    fn does_not_flag_rpc_transport_schema() {
        let schema = cratestack_parser::parse_schema(
            r#"
transport rpc

model Widget {
  id Int @id
}
"#,
        )
        .expect("schema should parse");

        assert!(!schema_declares_grpc_transport(&schema));
    }
}
