//! Collision detection under the SQL/codegen name normalization
//! (`to_snake_case`) applied at codegen time — see
//! `cratestack-macros/src/shared/sql.rs`,
//! `cratestack-macros/src/model/descriptor/columns.rs`, and
//! `cratestack-macros/src/model/descriptor.rs` (table names). Two raw
//! schema names that are distinct as Rust identifiers (`myField` vs
//! `my_field`, or `model Foo` vs `model foo`) can still normalize to the
//! *same* SQL column, table name, or generated resolver-method name — a
//! silent DDL/codegen corruption bug the parser's plain raw-name
//! duplicate check never catches, since that only ever compares the raw
//! spelling.
//!
//! `to_snake_case` itself lives in `cratestack-core` (shared with
//! `cratestack-macros` so both stay in sync — see
//! `cratestack_core::route_naming`); this module only diffs names against
//! that shared implementation, it never reimplements it.

use std::collections::BTreeMap;

use cratestack_core::route_naming::to_snake_case;
use cratestack_core::{Field, Model, Schema, SourceSpan};

use crate::diagnostics::{SchemaError, span_error};

/// Scans `entries` (in declaration order) for the first pair whose names
/// normalize to the same string under [`to_snake_case`] while remaining
/// distinct as raw names. Returns `(first_name, colliding_name,
/// colliding_span, normalized)` for the first collision found, or `None`.
///
/// Exact raw-name duplicates are intentionally *not* reported here — every
/// caller already has its own plain duplicate-name check, which fires
/// first and produces a more direct message ("duplicate field `x`") for
/// that case.
fn find_snake_case_collision<'a>(
    entries: impl IntoIterator<Item = (&'a str, SourceSpan)>,
) -> Option<(&'a str, &'a str, SourceSpan, String)> {
    let mut seen: BTreeMap<String, &str> = BTreeMap::new();
    for (name, span) in entries {
        let normalized = to_snake_case(name);
        match seen.get(normalized.as_str()) {
            Some(&existing) if existing != name => {
                return Some((existing, name, span, normalized));
            }
            Some(_) => continue,
            None => {
                seen.insert(normalized, name);
            }
        }
    }
    None
}

/// Reject fields on `owner_kind`/`owner_name` (a model/mixin/type/auth
/// block/view — anything whose fields become SQL columns) whose names
/// collide after `to_snake_case` normalization, naming both offending
/// field names and pointing at the second (later-declared) one's span.
pub(super) fn validate_field_column_collisions(
    fields: &[Field],
    owner_kind: &str,
    owner_name: &str,
) -> Result<(), SchemaError> {
    let entries = fields.iter().map(|field| (field.name.as_str(), field.span));
    if let Some((existing, colliding, span, normalized)) = find_snake_case_collision(entries) {
        return Err(span_error(
            format!(
                "field `{colliding}` on {owner_kind} `{owner_name}` collides with field \
                 `{existing}` — both normalize to the SQL/codegen column name `{normalized}` \
                 (see `cratestack_core::route_naming::to_snake_case`); rename one of them",
            ),
            span,
        ));
    }
    Ok(())
}

/// Reject two `model` declarations whose names collide after
/// `to_snake_case` normalization. Distinct from
/// [`validate_field_column_collisions`]: this guards the *table* name
/// (`pluralize(to_snake_case(model.name))`), the generated Rust accessor
/// constant (`to_snake_case(model.name).to_uppercase()`), and REST route
/// paths, all of which are keyed off the model name rather than a field
/// name. Without this, `model Foo` and `model foo` both pass the parser's
/// raw-name uniqueness check (`type_names::ensure_unique`) — they're
/// distinct raw identifiers — but collide into the same generated Postgres
/// table and the same Rust `FOO_MODEL` constant, which today only ever
/// surfaces as an opaque `error[E0428]: the name FOO_MODEL is defined
/// multiple times` at the macro call site.
pub(super) fn validate_model_name_collisions(models: &[Model]) -> Result<(), SchemaError> {
    let entries = models
        .iter()
        .map(|model| (model.name.as_str(), model.name_span));
    if let Some((existing, colliding, span, normalized)) = find_snake_case_collision(entries) {
        return Err(span_error(
            format!(
                "model `{colliding}` collides with model `{existing}` — both normalize to \
                 `{normalized}` for the generated SQL table name, Rust accessor constant, and \
                 REST route path (see `cratestack_core::route_naming::to_snake_case`); rename \
                 one of them",
            ),
            span,
        ));
    }
    Ok(())
}

/// Reject cross-kind type declaration collisions after `to_snake_case`
/// normalization. This catches cases where a `type`, `enum`, `model`,
/// `mixin`, and `auth` declarations have names that collide under
/// normalization.
///
/// Generation mapping (which kinds share generated symbols):
/// - `type` blocks → Rust struct in `types` module
/// - `enum` blocks → Rust enum in `types` module
/// - `model` blocks → Rust struct in `models` module
/// - `mixin` blocks → NO code generated, just metadata
/// - `auth` blocks → NO code generated, just configuration
///
/// Both `types` and `models` modules are re-exported at parent level via
/// `pub use types::*; pub use models::*;`, so a `type Foo` and
/// `model Foo` would collide despite being in different modules.
/// Similarly, `type Foo` and `enum Foo` collide in the same module.
///
/// Pairs that actually share generated symbols:
/// - type-vs-enum (both in `types` module, re-exported)
/// - type-vs-model (both re-exported to parent)
/// - enum-vs-model (both re-exported to parent)
///
/// Pairs that do NOT generate code to collide but are still rejected
/// for clarity/consistency:
/// - type-vs-mixin, enum-vs-mixin, model-vs-mixin (mixin is metadata-only)
/// - type-vs-auth, enum-vs-auth, model-vs-auth (auth is metadata-only)
/// - mixin-vs-auth (both metadata-only)
pub(super) fn validate_type_declaration_collisions(schema: &Schema) -> Result<(), SchemaError> {
    // Collect all type declarations with their kind and span.
    #[derive(Clone, Copy)]
    enum DeclKind {
        Type,
        Enum,
        Model,
        Mixin,
        Auth,
    }

    let mut entries: Vec<(&str, SourceSpan, DeclKind)> = Vec::new();

    for ty in &schema.types {
        entries.push((ty.name.as_str(), ty.span, DeclKind::Type));
    }
    for enum_decl in &schema.enums {
        entries.push((enum_decl.name.as_str(), enum_decl.span, DeclKind::Enum));
    }
    for model in &schema.models {
        entries.push((model.name.as_str(), model.span, DeclKind::Model));
    }
    for mixin in &schema.mixins {
        entries.push((mixin.name.as_str(), mixin.span, DeclKind::Mixin));
    }
    if let Some(auth) = &schema.auth {
        entries.push((auth.name.as_str(), auth.span, DeclKind::Auth));
    }

    // Scan for normalized-name collisions across declaration kinds.
    let mut seen: BTreeMap<String, (&str, DeclKind)> = BTreeMap::new();
    for (name, span, kind) in entries {
        let normalized = to_snake_case(name);
        if let Some((existing_name, existing_kind)) = seen.get(normalized.as_str()) {
            if existing_name != &name {
                // Names are distinct as raw identifiers but collide under normalization.
                let kind_name = match kind {
                    DeclKind::Type => "type",
                    DeclKind::Enum => "enum",
                    DeclKind::Model => "model",
                    DeclKind::Mixin => "mixin",
                    DeclKind::Auth => "auth",
                };
                let existing_kind_name = match existing_kind {
                    DeclKind::Type => "type",
                    DeclKind::Enum => "enum",
                    DeclKind::Model => "model",
                    DeclKind::Mixin => "mixin",
                    DeclKind::Auth => "auth",
                };
                return Err(span_error(
                    format!(
                        "{} `{name}` collides with {} `{existing_name}` — both normalize to \
                         `{normalized}` for the generated Rust type name (see \
                         `cratestack_core::route_naming::to_snake_case`); rename one of them",
                        kind_name,
                        existing_kind_name,
                        name = name,
                        existing_name = existing_name
                    ),
                    span,
                ));
            }
        } else {
            seen.insert(normalized, (name, kind));
        }
    }

    Ok(())
}
