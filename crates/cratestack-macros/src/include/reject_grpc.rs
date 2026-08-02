//! Guards `transport grpc` schemas in and out of the Rust codegen path —
//! split out of `parse.rs` (which stays focused on entry-macro argument
//! parsing + the shared schema loader) per the repo's 200-LoC file
//! convention.
//!
//! Three independent guards, called from the three places that actually
//! differ (previously this was one guard shared unconditionally by all
//! three entry macros via `parse::parse_schema_literal`; ticket #171 split
//! server out first, because "no runtime yet" stopped being true for
//! exactly one of the three; ticket #209 splits client out from embedded
//! for the same reason — a second one stopped being true):
//!
//! - [`guard_server_grpc_transport`] — `include_server_schema!` only. A
//!   `Grpc` schema now compiles for real *if* this crate's own `grpc`
//!   Cargo feature is on (forwarded from `cratestack-pg`); if it's off,
//!   this still rejects with `compile_error!`, but the message says
//!   "enable the feature", not "not implemented at all".
//! - [`guard_client_grpc_transport`] — `include_client_schema!` only.
//!   Ticket #209: same shape as the server guard above — a `Grpc` schema
//!   compiles for real when the `grpc` feature is on (the native `tonic`
//!   client codegen lives in `include::client::grpc`), and still rejects
//!   with an "enable the feature" message when it's off. Deliberately
//!   **not** merged back with `guard_server_grpc_transport` into one
//!   feature-gated guard shared by both macros: the two entry points emit
//!   structurally different things behind the same feature flag (a tonic
//!   *service* vs. a tonic *client*), and a future guard for one path
//!   changing shape (e.g. a narrower feature, a deprecation) shouldn't have
//!   to thread a `macro_name` branch back in here to un-merge them — same
//!   reasoning `guard_client_or_embedded_grpc_transport` documented for why
//!   it didn't merge with the server guard in the first place, now applied
//!   one level deeper.
//! - [`guard_embedded_grpc_transport`] — `include_embedded_schema!` only.
//!   Unconditional reject, feature or no feature: the embedded role has no
//!   transport at all (rusqlite only, no HTTP surface of any kind), so
//!   there is no "enable the feature" story for it the way there is for
//!   server/client — a `grpc` Cargo feature toggle can never make this
//!   macro emit anything for a `Grpc` schema.
//!
//! Without any of these guards, a `Grpc` schema would silently fall
//! through `collect.rs`'s `is_rpc` boolean (`matches!(schema.transport,
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

/// `include_client_schema!` only. See the module doc — mirrors
/// [`guard_server_grpc_transport`]'s feature-gated shape: a `Grpc` schema
/// compiles for real when this crate's own `grpc` Cargo feature is on
/// (forwarded from `cratestack-pg`'s `grpc` feature, same as the server
/// path — ticket #209), and rejects with an "enable the feature" message
/// when it's off.
pub(super) fn guard_client_grpc_transport(
    schema_path: &LitStr,
    schema: &cratestack_core::Schema,
) -> Result<(), TokenStream> {
    if !schema_declares_grpc_transport(schema) {
        return Ok(());
    }
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
             https://github.com/cratestack/cratestack/issues/209).",
        )
        .to_compile_error(),
    ))
}

/// `include_embedded_schema!` only. See the module doc — always rejects a
/// `Grpc` schema, feature or no feature: the embedded role has no
/// transport at all.
pub(super) fn guard_embedded_grpc_transport(
    schema_path: &LitStr,
    schema: &cratestack_core::Schema,
) -> Result<(), TokenStream> {
    if !schema_declares_grpc_transport(schema) {
        return Ok(());
    }
    Err(TokenStream::from(
        syn::Error::new(
            schema_path.span(),
            "schema declares `transport grpc`, but `include_embedded_schema!` has no gRPC \
             codegen and never will — the embedded role (rusqlite, no HTTP surface) has no \
             transport at all, REST/RPC/RPC-batch included. `include_server_schema!` (behind \
             its `grpc` Cargo feature, ticket #171) and `include_client_schema!` (behind the \
             same feature, ticket #209) both support `transport grpc`; \
             `include_embedded_schema!` does not and is not a place to add it. \
             `cratestack generate-proto` can still emit this schema's `.proto` contract today \
             for use with a non-CrateStack gRPC client.",
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
