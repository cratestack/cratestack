//! `FieldView` — one struct field's worth of view data, shared by every
//! `DataClassView` (`crate::views`) regardless of which builder produced it
//! (`crate::builders::build_data_class`, `crate::find_many_views`'s five
//! call sites). Split out from `crate::views` per the repo's 200-LoC file
//! convention.
//!
//! Issue #668 phase 2/3: this used to also carry a full set of
//! fluent-builder-codegen fields (`builder_setter`, `builder_backing_type`,
//! `builder_required`, `list_needs_default`, `list_elem_type`,
//! `add_setter`, `is_list`) so `templates/model_builder_class.dart.j2`
//! could render an inline `{Class}Builder` class. That template is gone —
//! `package:cratestack_builder` now generates the builder from the emitted
//! Dart source itself (via `@CratestackBuilder(...)`, see
//! `DataClassView::builder_args`), and every one of those fields was
//! recoverable from the Dart source the analyzer already has (confirmed by
//! `dart-packages/cratestack_builder/lib/src/builder_generator.dart`'s own
//! doc comment), so they were dead weight here and have been deleted along
//! with the derivations that produced them. The three exceptions that
//! genuinely aren't recoverable (`listDefaults`, `touchFlagFields`,
//! `nonDefaultingListFields`) are threaded through `DataClassView::
//! builder_args` instead, computed in `crate::builders::build_data_class`
//! from `is_nullable_patch_field` below plus each field's raw arity/
//! relation-ness — not carried on `FieldView` itself.

use serde::Serialize;

use crate::patch_touch::patch_touch_fields;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FieldView {
    pub(crate) identifier: String,
    pub(crate) wire_name: String,
    pub(crate) dart_type: String,
    pub(crate) required: bool,
    pub(crate) from_wire_expr: String,
    pub(crate) to_wire_expr: String,
    /// Whether this field is a `Patch`-kind field on a nullable
    /// (`TypeArity::Optional`) column — the one case where a `null` on the
    /// wire is genuinely ambiguous between "untouched" and "explicitly
    /// cleared" (cratestack#663). See `crate::patch_touch`'s module doc for
    /// the full rationale.
    pub(crate) is_nullable_patch_field: bool,
    /// `{identifier}IsSet` — see `crate::patch_touch`.
    pub(crate) touch_flag_identifier: String,
    /// The `toWire()` map-literal guard for this field — see
    /// `crate::patch_touch`.
    pub(crate) wire_write_condition: Option<String>,
}

impl FieldView {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        identifier: String,
        wire_name: String,
        dart_type: String,
        required: bool,
        is_patch: bool,
        is_optional: bool,
        from_wire_expr: String,
        to_wire_expr: String,
    ) -> Self {
        let patch_touch = patch_touch_fields(&identifier, is_patch, is_optional);
        FieldView {
            identifier,
            wire_name,
            dart_type,
            required,
            from_wire_expr,
            to_wire_expr,
            is_nullable_patch_field: patch_touch.is_nullable_patch_field,
            touch_flag_identifier: patch_touch.touch_flag_identifier,
            wire_write_condition: patch_touch.wire_write_condition,
        }
    }
}
