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
/// normalization, for the declaration kinds that actually land in a shared
/// generated Rust namespace.
///
/// Generation mapping (which kinds share generated symbols) — see
/// `cratestack-macros/src/include/server.rs` and `.../embedded.rs`:
/// - `type` blocks → Rust struct emitted into a `types` module
/// - `enum` blocks → Rust enum emitted into the same `types` module
/// - `model` blocks → Rust struct emitted into a `models` module
/// - `mixin` blocks → **no per-declaration symbol at all.** A mixin's own
///   name only ever appears as one string literal inside the single shared
///   `pub const MIXINS: &[&str]` introspection array; its *fields* are
///   spliced into the fields of whatever model uses it (already guarded by
///   [`validate_field_column_collisions`] at the merged-model level), but
///   the mixin declaration's own name never becomes a generated identifier.
/// - `auth` blocks → **no generated symbol keyed by the block's own name
///   either.** `schema.auth` is a single, unnamed-in-codegen block: its
///   name is used only to resolve policy field paths
///   (`cratestack-macros/src/policy/auth.rs`), never to name a struct,
///   const, or module.
///
/// Both `types` and `models` modules are re-exported at the parent level via
/// `pub use types::*; pub use models::*;`, so a `type Foo` and `model Foo`
/// collide despite being in different modules. Similarly, `type Foo` and
/// `enum Foo` collide directly in the same `types` module.
///
/// Pairs that actually share a generated symbol, and are rejected here:
/// - type-vs-enum (both land in the `types` module)
/// - type-vs-model (both re-exported to the parent module)
/// - enum-vs-model (both re-exported to the parent module)
///
/// `mixin` and `auth` are deliberately **excluded** from this check — per
/// cratestack#429's explicit acceptance criterion ("do not reject pairs
/// that share no generated symbol"), since neither one's own declaration
/// name is ever used to generate a Rust identifier, only their `MIXINS`
/// array entry (a string, not an identifier) or field-level metadata that's
/// already covered by [`validate_field_column_collisions`].
pub(super) fn validate_type_declaration_collisions(schema: &Schema) -> Result<(), SchemaError> {
    #[derive(Clone, Copy)]
    enum DeclKind {
        Type,
        Enum,
        Model,
    }

    impl DeclKind {
        fn label(self) -> &'static str {
            match self {
                DeclKind::Type => "type",
                DeclKind::Enum => "enum",
                DeclKind::Model => "model",
            }
        }
    }

    // Only `type`/`enum`/`model` share a generated namespace (see the
    // doc comment above) — `mixin` and `auth` are intentionally omitted.
    let mut kind_by_name: BTreeMap<&str, DeclKind> = BTreeMap::new();
    let mut entries: Vec<(&str, SourceSpan)> = Vec::new();

    for ty in &schema.types {
        kind_by_name.insert(ty.name.as_str(), DeclKind::Type);
        entries.push((ty.name.as_str(), ty.span));
    }
    for enum_decl in &schema.enums {
        kind_by_name.insert(enum_decl.name.as_str(), DeclKind::Enum);
        entries.push((enum_decl.name.as_str(), enum_decl.span));
    }
    for model in &schema.models {
        kind_by_name.insert(model.name.as_str(), DeclKind::Model);
        entries.push((model.name.as_str(), model.span));
    }

    if let Some((existing, colliding, span, normalized)) = find_snake_case_collision(entries) {
        let existing_kind = kind_by_name[existing].label();
        let colliding_kind = kind_by_name[colliding].label();
        return Err(span_error(
            format!(
                "{colliding_kind} `{colliding}` collides with {existing_kind} `{existing}` — \
                 both normalize to `{normalized}` for the generated Rust type name (see \
                 `cratestack_core::route_naming::to_snake_case`); rename one of them",
            ),
            span,
        ));
    }

    Ok(())
}
