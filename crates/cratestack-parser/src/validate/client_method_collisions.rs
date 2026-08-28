//! Collision detection between a schema-derived method on the generated
//! Rust client and one of that client's own built-in methods.
//!
//! Sibling of [`super::procedure_handler_collisions`] (cratestack#784) on
//! a different generated surface. That one guards two *schema-derived*
//! idents colliding with each other; this one guards a schema-derived
//! ident colliding with a method `cratestack-macros/src/client/{rest,rpc}.rs`
//! hard-codes into the same `impl` block:
//!
//! - `Client` gets one accessor per model, named
//!   `pluralize(to_snake_case(model.name))`, alongside its own `new` /
//!   `runtime` / `procedures` (REST) plus `rpc` / `batch` (RPC).
//! - `ProceduresClient` gets one method per procedure, named
//!   `to_snake_case(procedure.name)`, alongside its own `new`.
//!
//! So `model Procedure` and `procedure new` each put two methods of the
//! same name on one type. Both were verified against a real `cargo check`
//! of `examples/client-stub-rust`, which reports `error[E0592]: duplicate
//! definitions with name `procedures`` and `... with name `new``
//! respectively — a rustc error naming neither the model nor the
//! procedure that caused it, the same opacity #784 closed for `handle_*`.
//!
//! Two neighbouring cases this deliberately does **not** cover, because
//! they are not collisions:
//!
//! - A model accessor against a *procedure* method. They live on
//!   different types (`Client` vs `ProceduresClient`), so `model Post`
//!   alongside `procedure posts` compiles — confirmed by `cargo check`,
//!   not by reading the templates.
//! - Model-vs-model accessor collisions, already rejected upstream by
//!   [`super::route_collisions`]: an accessor name *is*
//!   `cratestack_core::route_naming::model_route_segment`, so two such
//!   models collide on their REST route first and are reported there.
//!
//! The generated client module is emitted by **both**
//! `include_client_schema!` and `include_server_schema!`
//! (`include/server/collect.rs` builds the server's own peer-calling
//! client from the same function), so this is not a client-role-only
//! concern.

use cratestack_core::Schema;
use cratestack_core::route_naming::{pluralize, to_snake_case};

use crate::diagnostics::{SchemaError, span_error};

/// Methods `client/rest.rs` and `client/rpc.rs` define on `Client`
/// themselves. The union of both transports, not a per-transport set:
/// `rpc`/`batch` are RPC-only, but `pluralize` only ever *appends*
/// (`s`/`es`/`ies`), so no model accessor can reach either name on any
/// transport — splitting the list by `schema.transport` would add a
/// branch that changes no outcome. The whole list is still checked, so
/// a built-in added to `Client` later is guarded without anyone having
/// to reason about reachability again.
const CLIENT_METHODS: &[&str] = &["new", "runtime", "procedures", "rpc", "batch"];

/// Methods `ProceduresClient` defines itself, on both transports.
const PROCEDURES_CLIENT_METHODS: &[&str] = &["new"];

pub(super) fn validate_client_method_collisions(schema: &Schema) -> Result<(), SchemaError> {
    for model in &schema.models {
        let accessor = pluralize(&to_snake_case(&model.name));
        if CLIENT_METHODS.contains(&accessor.as_str()) {
            return Err(span_error(
                format!(
                    "model `{model}` collides with the generated Rust client's own \
                     `{accessor}()` method — the per-model accessor is \
                     `pluralize(to_snake_case(\"{model}\"))`, which is `{accessor}`, and \
                     `cratestack_schema::client::Client` already defines a method of that \
                     name, so the generated `impl` block fails to compile as \
                     `error[E0592]: duplicate definitions with name `{accessor}``; rename \
                     the model",
                    model = model.name,
                ),
                model.name_span,
            ));
        }
    }

    for procedure in &schema.procedures {
        let method = to_snake_case(&procedure.name);
        if PROCEDURES_CLIENT_METHODS.contains(&method.as_str()) {
            return Err(span_error(
                format!(
                    "procedure `{procedure}` collides with the generated Rust client's own \
                     `{method}()` method — the per-procedure method is \
                     `to_snake_case(\"{procedure}\")`, which is `{method}`, and \
                     `cratestack_schema::client::ProceduresClient` already defines a method \
                     of that name, so the generated `impl` block fails to compile as \
                     `error[E0592]: duplicate definitions with name `{method}``; rename the \
                     procedure",
                    procedure = procedure.name,
                ),
                procedure.name_span,
            ));
        }
    }

    Ok(())
}
