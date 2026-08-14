//! Schema-include composers.
//!
//! Three top-level proc-macros target three deployment shapes (see the
//! 0.3.0 CHANGELOG for context):
//!
//! - [`include_server_schema`] — full server: sqlx Postgres backend,
//!   `Cratestack` runtime, axum router, procedure handlers, events. No
//!   rusqlite anywhere in the output. Its `db = Postgres` / `db = None`
//!   argument is cross-checked against the schema's own `datasource.provider`
//!   (cratestack#327) — a mismatch is a compile-time error. `db = None`'s
//!   codegen is otherwise unchanged for now (see the epic's later stories).
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
mod datasource_guard;
mod decimal_arg;
mod embedded;
mod extension_gate;
mod grpc_pb;
mod parse;
mod reject_grpc;
mod schema_args;
mod server;

use proc_macro::TokenStream;
use syn::parse_macro_input;

use parse::{SchemaPathArgs, ServerSchemaArgs};

pub(crate) fn include_server_schema(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as ServerSchemaArgs);
    server::compose_server_schema(&args.schema_path, args.db, args.decimal)
}

pub(crate) fn include_embedded_schema(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as SchemaPathArgs);
    embedded::compose_embedded_schema(&args.schema_path, args.decimal)
}

pub(crate) fn include_client_schema(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as SchemaPathArgs);
    client::compose_client_schema(&args.schema_path, args.decimal)
}
