//! Guards `transport grpc` schemas out of the Rust codegen path — split out
//! of `parse.rs` (which stays focused on entry-macro argument parsing + the
//! shared schema loader) per the repo's 200-LoC file convention.
//!
//! `transport grpc` parses (ticket #170, `cratestack-parser`) and
//! `cratestack-proto` can already emit its `.proto` `service` block for one
//! (ticket #170, `cratestack-proto`) — but no Rust codegen for it exists
//! yet: no `cratestack-grpc` server runtime (#171), no gRPC-Web client
//! (#172). Without this guard, a `Grpc` schema would silently fall through
//! `collect.rs`'s `is_rpc` boolean (`matches!(schema.transport,
//! TransportStyle::Rpc)` is `false` for `Grpc` too) into the REST codegen
//! branch, emitting REST routes for a schema that explicitly opted out of
//! REST — wrong-not-broken, and exactly the failure mode this ticket's
//! acceptance criteria calls out. Closing it here, called from
//! `parse::parse_schema_literal` (the one shared schema loader every entry
//! macro calls), means every downstream `if is_rpc { .. } else { .. }`
//! site is unreachable for a `Grpc` schema without having to be
//! individually patched — same precedent as `parse::reject_composite_primary_keys`,
//! and deliberately not specialized to any one of the three entry macros,
//! matching that precedent too.

use proc_macro::TokenStream;
use syn::LitStr;

pub(super) fn reject_grpc_transport_without_runtime(
    schema_path: &LitStr,
    schema: &cratestack_core::Schema,
) -> Result<(), TokenStream> {
    if !schema_declares_grpc_transport(schema) {
        return Ok(());
    }
    Err(TokenStream::from(
        syn::Error::new(
            schema_path.span(),
            "schema declares `transport grpc`, but gRPC server/client codegen is not \
             implemented yet (tracking: https://github.com/cratestack/cratestack/issues/171 \
             for the server runtime, https://github.com/cratestack/cratestack/issues/172 for \
             the browser client) — `cratestack generate-proto` can still emit this schema's \
             `.proto` contract including its `service` block today; only the Rust codegen path \
             (`include_server_schema!`/`include_client_schema!`/`include_embedded_schema!`) is \
             gated.",
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

    // `reject_grpc_transport_without_runtime` itself is deliberately not
    // exercised directly here: it returns `proc_macro::TokenStream`, whose
    // conversion from `proc_macro2::TokenStream` (via `syn::Error::
    // to_compile_error()`) requires an active proc-macro invocation context
    // and panics ("procedural macro API is used outside of a procedural
    // macro") in a plain `cargo test` run. `parse::reject_composite_primary_keys`
    // hits the same constraint and, for the same reason, is only tested
    // through its pure `find_composite_id_model` predicate — this mirrors
    // that precedent via `schema_declares_grpc_transport`, the pure
    // condition the guard branches on.
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
