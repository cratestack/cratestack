mod diagnostics;
mod line_helpers;
mod parse;
mod relation_actions;
mod relation_helpers;
mod validate;

#[cfg(test)]
mod tests_basic;
#[cfg(test)]
mod tests_docs;
#[cfg(test)]
mod tests_enums;
#[cfg(test)]
mod tests_extensions;
#[cfg(test)]
mod tests_field_attrs;
#[cfg(test)]
mod tests_list_arity;
#[cfg(test)]
mod tests_mixins;
#[cfg(test)]
mod tests_model_attrs;
#[cfg(test)]
mod tests_model_index;
#[cfg(test)]
mod tests_model_unique;
#[cfg(test)]
mod tests_procedures;
#[cfg(test)]
mod tests_relation_actions;
#[cfg(test)]
mod tests_relations;
#[cfg(test)]
mod tests_relations_policy;
#[cfg(test)]
mod tests_stream_attribute;
#[cfg(test)]
mod tests_transport;
#[cfg(test)]
mod tests_types;
#[cfg(test)]
mod tests_validators;
#[cfg(test)]
mod tests_vector;
#[cfg(test)]
mod tests_version;
#[cfg(test)]
mod tests_views;

use std::path::Path;

pub use diagnostics::SchemaError;

/// Canonical scalar type names built into the `.cstack` language (e.g.
/// `String`, `Int`, `Decimal`, ...), including `Page` (which is valid only
/// as a procedure return type — see `validate::type_names::validate_type_ref`
/// — not as a plain field type).
///
/// This is the same list `cratestack-lsp`'s autocompletion and the
/// `cratestack-pg`/`cratestack-sqlite` emitter/decoder round-trip tests
/// assert against, so a new builtin scalar only has to be added here once —
/// see cratestack#232 for why that matters (this list had already silently
/// drifted from the LSP's hand-copied one before this accessor existed).
pub fn builtin_type_names() -> &'static [&'static str] {
    validate::builtin_type_names()
}

#[cfg(test)]
use relation_helpers::parse_relation_attribute;

pub fn parse_schema(source: &str) -> Result<cratestack_core::Schema, SchemaError> {
    parse_schema_named("<schema>", source)
}

pub fn parse_schema_named(
    path: &str,
    source: &str,
) -> Result<cratestack_core::Schema, SchemaError> {
    let schema = parse::parse_schema_only(source)?;
    validate::validate_schema(path, source, &schema)?;
    Ok(schema)
}

/// Parse a `.cstack` source into a [`cratestack_core::Schema`] WITHOUT
/// running [`validate::validate_schema`].
///
/// Prefer [`parse_schema`] for any new source — this exists for two
/// legitimate cases where the validated pipeline understates what a
/// `Schema` value can actually be:
///
/// 1. A committed `migrations/*/schema.snapshot.json` can predate a
///    validation rule added later. `cratestack-cli`'s `migrate diff`
///    deserializes that "previous" snapshot directly and never re-runs
///    `validate_schema` on it (only the *new* side, parsed fresh from the
///    `.cstack` source, goes through [`parse_schema_file`]) — so an emitter
///    can still legitimately be handed a shape the current validator would
///    reject at the source level.
/// 2. Tests that deliberately exercise an emitter's rendering logic for
///    such an already-invalid shape, to prove the emitter itself still
///    behaves sanely if that shape arrives via (1) — see
///    `cratestack-migrate`'s `emit::postgres::tests::enums` for an example
///    (a list-valued enum column, rejected by cratestack#229/#236 at parse
///    time, but still real input to the Postgres emitter via a pre-#236
///    snapshot).
pub fn parse_schema_unvalidated(source: &str) -> Result<cratestack_core::Schema, SchemaError> {
    parse::parse_schema_only(source)
}

pub fn parse_schema_file(path: impl AsRef<Path>) -> Result<cratestack_core::Schema, SchemaError> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path).map_err(|error| {
        SchemaError::new(
            format!("failed to read schema file {}: {error}", path.display()),
            0..0,
            1,
        )
    })?;
    parse_schema_named(&path.display().to_string(), &source)
}
