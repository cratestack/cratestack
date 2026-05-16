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

mod client;
mod embedded;
mod parse;
mod server;

use proc_macro::TokenStream;
use syn::{LitStr, parse_macro_input};

use parse::ServerSchemaArgs;

pub(crate) fn include_server_schema(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as ServerSchemaArgs);
    let _ = args.db; // Postgres-only today; reserved for future backends.
    server::compose_server_schema(&args.schema_path)
}

pub(crate) fn include_embedded_schema(input: TokenStream) -> TokenStream {
    let schema_path = parse_macro_input!(input as LitStr);
    embedded::compose_embedded_schema(&schema_path)
}

pub(crate) fn include_client_schema(input: TokenStream) -> TokenStream {
    let schema_path = parse_macro_input!(input as LitStr);
    client::compose_client_schema(&schema_path)
}
