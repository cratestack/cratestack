//! Reject a model field set that declares a nullable-column field `foo`
//! alongside a real field literally named `fooIsSet`.
//!
//! Same defect class as `builder_setter_collisions` (that module's doc
//! comment coins the phrase): `cratestack-client-dart/src/patch_touch.rs`
//! mechanically derives a sibling `{field}IsSet` bool for every
//! `TypeArity::Optional` (nullable-column) field that lands in a model's
//! generated `Update{Model}Input` Dart class — the tri-state "untouched vs.
//! explicitly cleared" flag cratestack#663 introduced. That derivation has
//! no collision guard of its own: a schema that also declares a real field
//! named `fooIsSet` gets two Dart members fighting over the same generated
//! identifier (a duplicate constructor parameter, a duplicate field
//! declaration, and every reference the compiler further confuses trying
//! to recover) — `dart analyze` reports it as eight separate errors
//! (`duplicate_field_formal_parameter`, `duplicate_definition`,
//! `duplicate_named_argument` x2, `argument_type_not_assignable` x2,
//! `unchecked_use_of_nullable_value`, `missing_default_value_for_parameter`)
//! with no span pointing back at the schema line at fault. This check pins
//! the parse-time rejection instead.
//!
//! Scoped to only `model.fields`: `Update{Model}Input` (`DataClassKind::
//! Patch` in `cratestack-client-dart::context::build_template_context`) is
//! the only generated Dart class this touch flag ever appears on — `type`/
//! `view`/`auth` blocks and procedure argument lists never go through
//! `DataClassKind::Patch`, so they need no call site here.

use cratestack_core::SourceSpan;

use crate::diagnostics::{SchemaError, span_error};

/// `fields` is `(field_name, field_span, is_nullable_patch_field)` triples
/// in declaration order. `is_nullable_patch_field` must mirror exactly
/// which fields `cratestack-client-dart` actually gives an `IsSet` sibling
/// to — `TypeArity::Optional`, minus the primary-key field (dropped by
/// `Update{Model}Input`'s own `!is_primary_key` filter) and minus relation
/// fields (dropped by `scalar_model_fields`, the same filter
/// `Update{Model}Input`'s field list is built from). Passing `true` for a
/// field that filter would actually exclude over-rejects a schema that
/// generates no colliding identifier at all; passing `false` for a field it
/// would include lets a real collision through unchecked.
pub(super) fn validate_no_touch_flag_collision<'a>(
    fields: impl IntoIterator<Item = (&'a str, SourceSpan, bool)> + Clone,
    model_name: &str,
) -> Result<(), SchemaError> {
    for (nullable_name, _nullable_span, is_nullable_patch_field) in fields.clone() {
        if !is_nullable_patch_field {
            continue;
        }
        let reserved = format!("{nullable_name}IsSet");

        for (other_name, other_span, _) in fields.clone() {
            if other_name != reserved {
                continue;
            }
            return Err(span_error(
                format!(
                    "model `{model_name}` declares nullable field `{nullable_name}` alongside a \
                     field named `{other_name}` — the generated Dart `Update{model_name}Input` \
                     pairs every nullable patch field with a sibling `{{field}}IsSet` bool \
                     tracking whether the caller touched it (cratestack#663), and \
                     `{nullable_name}IsSet` is exactly the real field `{other_name}`'s own \
                     generated identifier. Rename `{other_name}`.",
                ),
                other_span,
            ));
        }
    }

    Ok(())
}
