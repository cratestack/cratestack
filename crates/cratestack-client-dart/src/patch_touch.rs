//! Tri-state "untouched vs. explicitly cleared" bookkeeping for a `Patch`-
//! kind `FieldView` (cratestack#663). Split out of `crate::field_view` per
//! the repo's 200-LoC file convention.
//!
//! `Update{Model}Input`'s wire representation of "the caller didn't touch
//! this field" has to be a single, unambiguous shape — and for a nullable
//! (`TypeArity::Optional`) column, `null` alone can't be that shape: it
//! also has to mean "explicitly cleared" (cratestack#567). The generated
//! Rust client expresses the two states as `Option<Option<T>>` (outer =
//! touched, inner = value-or-clear); Dart's generated data classes are
//! flat, materialized objects rather than a nested-option type, so the
//! same distinction here is a plain sibling `bool` field
//! (`{identifier}IsSet`) next to `identifier`'s own value, rather than
//! wrapping `identifier` itself in a generated tri-state type — that keeps
//! every field's own Dart type exactly what `dart_types::dart_field_type`
//! already says (`String?`, not some wrapper), so reading a constructed
//! `Update*Input` back stays ordinary field access.
//!
//! `Required`/`List`-arity `Patch` fields don't need this: `null`
//! unambiguously means "untouched" there (a `NOT NULL` column can never be
//! explicitly cleared, and a list field's clear-vs-untouched distinction
//! doesn't exist on the Rust side either — see `struct_field_definition`'s
//! `TypeArity::List` arm in `crates/cratestack-macros/src/model/
//! struct_only/field_definition.rs`), so a plain `{identifier} != null`
//! wire-write condition is already correct for them.
//!
//! A nullable-column field's own `wire_write_condition` is **not** just
//! `{identifier}IsSet`, even though that flag alone looks sufficient: the
//! generated constructor is a plain, public, non-builder-only `const`
//! constructor, so `UpdateWidgetInput(weight: 5)` — direct construction,
//! bypassing the builder entirely — is ordinary, idiomatic Dart, and it
//! leaves `weightIsSet` at its default `false`. Guarding on `weightIsSet`
//! alone would silently drop that caller-supplied value off the wire
//! (cratestack#663 review). The condition is `{identifier}IsSet ||
//! {identifier} != null`: a non-null value can only mean "write this",
//! regardless of which constructor path set it, and "untouched" is by
//! definition `{identifier} == null`, so the added disjunct can never
//! misfire for the untouched case.

/// The three [`crate::field_view::FieldView`] fields this module computes,
/// bundled so [`crate::field_view::FieldView::new`] can destructure the
/// tuple once rather than repeating the `is_patch && is_optional`
/// condition at each call site.
pub(crate) struct PatchTouchFields {
    pub(crate) is_nullable_patch_field: bool,
    /// `{identifier}IsSet`, or empty when `is_nullable_patch_field` is
    /// `false` (unused by the template in that case).
    pub(crate) touch_flag_identifier: String,
    /// The `toWire()` map-literal guard for this field, as a raw Dart
    /// boolean expression, or `None` for "always include" (every `Plain`/
    /// `ProjectionModel`-kind field — those aren't patches, so there's no
    /// "untouched" state to hide).
    pub(crate) wire_write_condition: Option<String>,
}

pub(crate) fn patch_touch_fields(
    identifier: &str,
    is_patch: bool,
    is_optional: bool,
) -> PatchTouchFields {
    let is_nullable_patch_field = is_patch && is_optional;
    let touch_flag_identifier = if is_nullable_patch_field {
        format!("{identifier}IsSet")
    } else {
        String::new()
    };
    let wire_write_condition = if !is_patch {
        None
    } else if is_nullable_patch_field {
        // `{field}IsSet` alone isn't enough: `Update{Model}Input`'s
        // constructor is public and plain (not builder-only), so
        // `UpdateWidgetInput(weight: 5)` — direct construction, no builder
        // involved — is a legitimate, idiomatic way to build one, and it
        // leaves `weightIsSet` at its default `false`. A guard of just
        // `weightIsSet` would silently drop that caller-supplied value off
        // the wire. `weight != null` closes that gap: a non-null value can
        // only mean "the caller wants this written", regardless of which
        // constructor path produced it, and it can never fire for the
        // "untouched" state (untouched is defined as `weight == null`), so
        // it can't resurrect the "untouched sent as null" bug this whole
        // feature exists to fix.
        Some(format!("{touch_flag_identifier} || {identifier} != null"))
    } else {
        Some(format!("{identifier} != null"))
    };
    PatchTouchFields {
        is_nullable_patch_field,
        touch_flag_identifier,
        wire_write_condition,
    }
}
