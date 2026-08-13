//! Composite `@@id([...])` primary keys: detection, and the one message
//! every entry point uses to reject them.
//!
//! `cratestack-parser` accepts `@@id([a, b])` as valid, and
//! `cratestack-migrate` already emits correct composite `PRIMARY KEY`
//! DDL for it. Nothing downstream of that does: query builders,
//! axum/RPC routing, and all three client generators assume exactly one
//! scalar PK column throughout (`ModelDescriptor<M, PK>` and friends).
//! Tracked as <https://github.com/cratestack/cratestack/issues/136>.
//!
//! `include_*_schema!` has rejected these since that gap was found, with
//! a `compile_error!` naming the model. The **CLI generators did not** —
//! they went straight to a `primary_key_field(model).expect(...)` and
//! panicked with `validated schemas always have an id field`, which is
//! both a panic instead of an error and a false statement: the parser
//! validates such a schema happily. Confirmed by hand on 2026-08-13
//! against `generate-typescript` and `generate-dart`, both of which
//! aborted with that panic on a schema containing one composite-PK model.
//!
//! This module exists so the predicate and the wording live in exactly
//! one place. A second copy of `starts_with("@@id(")` somewhere else is
//! how the CLI path drifted from the macro path to begin with.

use crate::schema::{Model, Schema};

/// The first model declaring a composite primary key, if any.
///
/// Matches on the raw attribute text (`@@id(`) rather than a parsed
/// form, because that is what the parser preserves and what the macro
/// path has always matched on — keeping the two identical is the point
/// of this function existing.
pub fn find_composite_id_model(schema: &Schema) -> Option<&Model> {
    schema
        .models
        .iter()
        .find(|model| model.attributes.iter().any(|a| a.raw.starts_with("@@id(")))
}

/// The rejection message, verbatim, for every entry point that has to
/// refuse a composite-PK schema. Callers wrap it in whatever their error
/// type is — `compile_error!` for the macros, a generator error for the
/// CLI paths — but the text a user reads is the same either way.
pub fn composite_id_unsupported_message(model_name: &str) -> String {
    format!(
        "model `{model_name}` declares a composite primary key via `@@id([...])`, which is not \
         yet supported by codegen (query builders, routing, and generated clients still assume a \
         single scalar `@id`); see https://github.com/cratestack/cratestack/issues/136 for status"
    )
}
