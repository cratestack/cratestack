//! Reject a field set that declares both `build` and `set_build`, or a
//! list field alongside its own generated `add_{field}` append setter.
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
//!
//! [`validate_no_add_setter_collision`] is the same defect class, one
//! layer over: `cratestack-macros/src/builder/fields.rs::build_spec`
//! derives every list-arity field's append setter mechanically as
//! `add_{field.name}` (Rust) / `add{Field}` (Dart, capitalized), with no
//! singularization (issue #661 — `children` is a real list field in this
//! repo's own stress fixtures, and a rules-based singularizer would mangle
//! it to `childre`). A field set that declares both a list field `tags`
//! and a field literally named `add_tags` (or the camelCase `addTags`)
//! therefore gets two identically-named setters, the same
//! `E0592`/`duplicate_definition` failure as `build`/`set_build`, just one
//! generation step removed from a literal name in the schema.

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

/// `fields` is `(field_name, field_span, is_list)` triples in declaration
/// order, `is_list` true iff the field's declared arity is `[]` (list).
///
/// For every list-arity field `f`, the generated builder reserves the
/// normalized name `add_f` (Rust spells it `add_f` literally; Dart spells
/// it `addF` — the same `to_snake_case` normalization the `build`/
/// `set_build` check above uses maps both onto one comparison). A second
/// field in the same set whose own normalized name equals that reserved
/// name collides with the append setter the *first* field's list-ness
/// already generates.
///
/// Deliberately scoped to only the fields that are actually list-arity:
/// `add_foo` beside a scalar `foo` generates no append setter at all and
/// must keep parsing (an earlier revision of the sibling
/// `builder_collisions.rs` validator over-scoped an analogous check and
/// had to be reverted after it started rejecting unrelated, non-colliding
/// declarations).
pub(super) fn validate_no_add_setter_collision<'a>(
    fields: impl IntoIterator<Item = (&'a str, SourceSpan, bool)> + Clone,
    owner_kind: &str,
    owner_name: &str,
) -> Result<(), SchemaError> {
    for (list_name, _list_span, is_list) in fields.clone() {
        if !is_list {
            continue;
        }
        let reserved = format!("add_{}", to_snake_case(list_name));

        for (other_name, other_span, _) in fields.clone() {
            if to_snake_case(other_name) != reserved {
                continue;
            }
            return Err(span_error(
                format!(
                    "{owner_kind} `{owner_name}` declares list field `{list_name}` alongside a \
                     field named `{other_name}` — the generated append setter for `{list_name}` \
                     is `.add_{}(item)` in Rust and `.add{}(item)` in Dart (issue #661, derived \
                     mechanically from the field name, no singularization), which collides with \
                     the setter `{other_name}` already generates. Rename `{other_name}`.",
                    to_snake_case(list_name),
                    capitalize_first(&to_camel_ish(list_name)),
                ),
                other_span,
            ));
        }
    }

    Ok(())
}

/// A rough camelCase-ish rendering of a schema field name for the Dart half
/// of the collision message only — diagnostics text, not codegen. Turns a
/// snake_case name into camelCase (`add_tags` -> `addTags`) and leaves an
/// already-camelCase name untouched, matching what the Dart generator's own
/// identifier conversion would produce for the common cases this
/// diagnostic fires on.
fn to_camel_ish(name: &str) -> String {
    let mut output = String::new();
    let mut upper_next = false;
    for ch in name.chars() {
        if ch == '_' {
            upper_next = true;
            continue;
        }
        if upper_next {
            output.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            output.push(ch);
        }
    }
    output
}

/// Uppercases just the first character — mirrors
/// `cratestack-client-dart::field_view::capitalize_first`, reimplemented
/// here rather than shared because `cratestack-parser` does not depend on
/// `cratestack-client-dart` and this is diagnostic text, not codegen.
fn capitalize_first(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
