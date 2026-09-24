mod axum;
mod builder;
mod client;
mod computed;
mod event;
mod include;
mod json_schema;
mod model;
mod policy;
mod procedure;
mod query;
mod relation;
mod shared;
mod transport;
mod types;
mod validators;
mod view;

use proc_macro::TokenStream;

/// Full server schema: sqlx Postgres backend, `Cratestack` runtime, axum
/// router, procedures, events. Pass `db = Postgres` (only value currently
/// supported; MySQL / SQLite-via-sqlx will land in a future release).
#[proc_macro]
pub fn include_server_schema(input: TokenStream) -> TokenStream {
    include::include_server_schema(input)
}

/// Embedded ORM schema: rusqlite backend only. Compiles to native and to
/// `wasm32-unknown-unknown` (via `sqlite-wasm-rs`). No sqlx, no axum, no
/// procedures. Local apps that don't need an RPC surface use this.
#[proc_macro]
pub fn include_embedded_schema(input: TokenStream) -> TokenStream {
    include::include_embedded_schema(input)
}

/// HTTP client schema: model/input/procedure stubs for talking to a server
/// over the wire. No DB, no router, no FromRow impls. Renamed from
/// `include_client_macro!` in 0.3.0.
#[proc_macro]
pub fn include_client_schema(input: TokenStream) -> TokenStream {
    include::include_client_schema(input)
}

/// Not public API: no stability guarantee, and no facade re-exports it.
/// Expands to the JSON Schemas phase 2 of the MCP operator generates for
/// each procedure of a `.cstack` file (cratestack#1037), so the
/// round-trip suites can check them against the real generated types.
/// Takes the same arguments as `include_client_schema!`. See
/// `src/include/json_schema_probe.rs`.
#[doc(hidden)]
#[proc_macro]
pub fn __procedure_json_schemas(input: TokenStream) -> TokenStream {
    include::procedure_json_schemas(input)
}
