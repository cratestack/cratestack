//! `.cstack` IR → JSON Schema 2020-12 for a procedure's arguments (input)
//! and object return type (output). Phase 2 of the MCP operator
//! (cratestack#1037, epic cratestack#1033, ADR 0002 § Tools).
//!
//! **The schema follows serde, never the reverse.** Generated `Args` and
//! output types derive `Serialize`/`Deserialize`, so serde is the wire
//! contract. Each mapping here was measured against what `serde_json`
//! does with the real generated types (see `scalar.rs`), and the
//! round-trip suites re-measure it on every run: `cratestack-api`'s
//! `tests/json_schema_round_trip.rs` (and its `decimal = BigDecimal`
//! twin, `tests/json_schema_bigdecimal.rs`) and `cratestack-pg`'s
//! `tests/json_schema_models.rs`. Nothing here changes a serde
//! representation to make a schema easier.
//!
//! **Why it lives in `cratestack-macros` (L1).** Its inputs are the IR
//! (`cratestack-core`, L0) and its output is a `serde_json::Value`, so L1
//! is the lowest layer that can host it, and `docs/adr/layers.toml` needs
//! no change. Phase 3 calls it at macro-expansion time, inside the same
//! proc-macro that already holds the parsed schema, and bakes the result
//! into generated code as `&'static str`. Being a proc-macro dependency,
//! nothing here reaches a consumer's runtime graph, and `serde_json` was
//! already in every facade's graph through `cratestack-core`. The
//! alternative home, `cratestack-mcp` (L4), would need the IR at runtime
//! and could not run during expansion.
//!
//! Phase 2 emits nothing into users' code. The only caller outside this
//! module is `include::json_schema_probe`, a `#[doc(hidden)]` macro the
//! round-trip suites use to reach the generator through real macro
//! expansion.
//!
//! **Where JSON Schema is looser than serde.** Each gap is pinned by a
//! test in the round-trip suites, so a change on either side is noticed:
//!
//! - `Int`: JSON Schema's `integer` accepts `1.0`; serde_json's `i64`
//!   rejects any float-shaped number.
//! - `DateTime`: the pattern checks the RFC 3339 shape, not the calendar.
//!   `2024-02-30T00:00:00Z` passes the pattern and fails chrono. Validators
//!   that assert `format: date-time` catch it.
//! - `Decimal`: the patterns have no digit or exponent limit.
//!   `rust_decimal` holds at most 28–29 significant digits, and
//!   `bigdecimal` rejects an exponent past `i64`.
//! - `Float` on output: serde_json writes a non-finite `f64` as `null`,
//!   which the `number` schema rejects. No JSON Schema is right for both
//!   directions here, since serde rejects `null` on input.
//! - `DateTime` on output: chrono writes a year past 9999 as `+10000-…`,
//!   which is not RFC 3339, so the schema rejects it.

mod error;
mod generator;
mod parts;
mod scalar;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_shapes;

use cratestack_core::{Procedure, Schema, TypeArity};
use serde_json::{Map, Value};

use crate::shared::decimal_backend::DecimalBackend;

pub(crate) use error::JsonSchemaError;
use generator::Generator;
use parts::{is_required, object_schema, with_docs};

/// MCP's default dialect (2026-07-28 spec, "Tools").
pub(crate) const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// The schema of `<procedure>::Args` (`crate::procedure::types`): one
/// property per argument, under the argument's own name.
///
/// `decimal` is the invocation's `decimal = ...` argument, passed in
/// rather than read from the ambient `with_decimal_backend` scope so that
/// tests can call this directly.
pub(crate) fn procedure_input_schema(
    schema: &Schema,
    procedure: &Procedure,
    decimal: Option<DecimalBackend>,
) -> Result<Value, JsonSchemaError> {
    let mut generator = Generator::new(schema, decimal);
    let mut properties = Map::new();
    let mut required = Vec::new();
    for arg in &procedure.args {
        let property = generator
            .type_ref(&arg.ty)
            .map_err(|e| e.within(format!("argument `{}`", arg.name)))
            .map_err(|e| e.within(format!("procedure `{}`", procedure.name)))?;
        properties.insert(arg.name.clone(), with_docs(property, &arg.docs));
        if is_required(&arg.ty) {
            required.push(arg.name.as_str());
        }
    }
    Ok(generator.finish(object_schema(properties, required)))
}

/// The schema of `<procedure>::Output`, when that is a JSON object: a
/// required `type`, `model` or `Page<T>`. MCP's `outputSchema` must have
/// an object root, so any other return type (a scalar, an enum, a list,
/// anything optional) gets `Ok(None)` and no mapping is attempted.
pub(crate) fn procedure_output_schema(
    schema: &Schema,
    procedure: &Procedure,
    decimal: Option<DecimalBackend>,
) -> Result<Option<Value>, JsonSchemaError> {
    let ty = &procedure.return_type;
    let is_declared_object = schema.types.iter().any(|t| t.name == ty.name)
        || schema.models.iter().any(|m| m.name == ty.name);
    if !(ty.is_page() || (is_declared_object && ty.arity == TypeArity::Required)) {
        return Ok(None);
    }
    let mut generator = Generator::new(schema, decimal);
    let mut root = generator
        .type_ref(ty)
        .map_err(|e| e.within("return type"))
        .map_err(|e| e.within(format!("procedure `{}`", procedure.name)))?;
    if let Some(object) = root.as_object_mut() {
        // A declared type comes back as a bare `$ref`. Stating `type`
        // beside it keeps the root visibly an object, which MCP clients
        // check without resolving references.
        object.insert("type".to_owned(), Value::from("object"));
    }
    Ok(Some(generator.finish(root)))
}
