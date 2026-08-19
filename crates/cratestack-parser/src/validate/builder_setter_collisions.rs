//! Reject a field set that declares both `build` and `set_build`.
//!
//! `cratestack-macros/src/builder/emit.rs`'s `setter_ident` renames a field
//! literally named `build` to a `set_build` setter, so the terminal
//! `build()` method (defined on the same builder type) stays callable. If
//! the same field set *also* declares a real field named `set_build`, that
//! field's setter is already `set_build` — the rename collides with it,
//! producing `error[E0592]: duplicate definitions with name `set_build``
//! at the `include_*_schema!` call site instead of at the schema.
//!
//! Every field set that reaches [`crate::builder`] generation is covered:
//! `model.fields` (checked once, post mixin-expansion — see
//! `parse::models::expand_model_mixins` — which is enough for the model
//! struct *and* every model-derived builder: `Create{M}Input`,
//! `Update{M}Input`, `{M}Where`, `{M}OrderByClause`, `{M}FindManyInput` all
//! draw their field names from the same merged set, so a clean model
//! implies a clean derived builder too), `type.fields`, `view.fields`, and
//! `procedure.args` (mixins never back a `type`/`view`/`auth` block or a
//! procedure's argument list, and `auth` blocks have no generated builder
//! at all — see `snake_case_collisions::validate_type_declaration_collisions`'s
//! module doc — so those two are the only other field sets that need their
//! own call site).

use cratestack_core::SourceSpan;

use cratestack_core::route_naming::to_snake_case;

use crate::diagnostics::{SchemaError, span_error};

/// `names` is `(field_name, field_span)` pairs in declaration order.
pub(super) fn validate_no_build_setter_collision<'a>(
    names: impl IntoIterator<Item = (&'a str, SourceSpan)>,
    owner_kind: &str,
    owner_name: &str,
) -> Result<(), SchemaError> {
    let mut build_span: Option<SourceSpan> = None;
    let mut set_build_span: Option<SourceSpan> = None;

    for (name, span) in names {
        // Normalized, not literal. The Rust shim renames a `build` field's
        // setter to `set_build`, but the Dart generator renames it to
        // `setBuild` — so a schema declaring `build` alongside a camelCase
        // `setBuild` field produced two identical Dart setters (`dart
        // analyze`: `duplicate_definition`) while a literal `"set_build"`
        // comparison saw nothing. `to_snake_case` maps both spellings onto
        // the one name that actually collides in either language.
        match to_snake_case(name).as_str() {
            "build" => build_span = Some(span),
            "set_build" => set_build_span = Some(span),
            _ => {}
        }
    }

    if let (Some(_), Some(set_build_span)) = (build_span, set_build_span) {
        return Err(span_error(
            format!(
                "{owner_kind} `{owner_name}` declares both a `build` field and a `set_build` \
                 field — the generated builder renames `build`'s own setter to `set_build` \
                 (so it doesn't collide with the terminal `build()` method), which then \
                 collides with the setter for the real `set_build` field (the Dart \
                 generator has the same clash as `setBuild`). Rename one of them.",
            ),
            set_build_span,
        ));
    }

    Ok(())
}
