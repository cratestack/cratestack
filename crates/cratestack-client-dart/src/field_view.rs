//! `FieldView` — one struct field's worth of view data, shared by every
//! `DataClassView` (`crate::views`) regardless of which builder produced it
//! (`crate::builders::build_data_class`, `crate::find_many_views`'s five
//! call sites). Split out from `crate::views` per the repo's 200-LoC file
//! convention once the fluent-builder-related fields below were added.

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
    /// Fluent-builder setter name (issue: builder pattern for generated
    /// Dart data classes). Identical to `identifier` except for the one
    /// reserved collision: a field literally named `build` would collide
    /// with the builder's own terminal `build()` method, so it gets
    /// `setBuild` instead — mirrors `cratestack-macros::builder`'s
    /// `set_build` shim for the analogous Rust-side collision, though the
    /// two are unrelated implementations.
    pub(crate) builder_setter: String,
    /// The builder's private backing-field type: always a nullable spelling
    /// of `dart_type` (so the field can start life unset), even when
    /// `dart_type` itself is already nullable (e.g. required `Json` fields,
    /// whose Dart type is `Object?` even though the constructor marks them
    /// `required` — see `dart_types::dart_type`'s `Json` arm). Computed
    /// once here so the template never has to string-match `dart_type`
    /// itself to decide whether to append `?`.
    pub(crate) builder_backing_type: String,
    /// Whether `build()` needs an explicit `as {dart_type}` narrowing cast
    /// off the nullable backing field, i.e. whether `builder_backing_type`
    /// and `dart_type` actually differ. `false` only for required `Json`
    /// fields, whose `dart_type` is already `Object?` — identical to
    /// `builder_backing_type` — so the cast would be a same-type no-op:
    /// `dart analyze --fatal-warnings` (what this repo's own CI and `just
    /// verify-dart` run) flags a same-type `as` with `unnecessary_cast`.
    pub(crate) builder_cast_needed: bool,
    /// Whether `TypeArity::List` — mechanically distinct from `required`
    /// (issue #661). `required` still feeds the **constructor** template
    /// unchanged (`this file's` `FieldView::new` keeps that parameter
    /// byte-for-byte the same value callers always passed — dropping
    /// `List` from it would turn a generated constructor's `required
    /// this.tags` into an optional named parameter, a breaking change to
    /// already-shipped generated Dart). The **builder** template instead
    /// branches on this flag plus `builder_required` below to give list
    /// fields their own, more permissive, notion of "required".
    pub(crate) is_list: bool,
    /// The builder-only "must be filled or `build()` throws" flag —
    /// `required && !is_list`. A list field is never builder-required: an
    /// unset list builds as `[]` in both languages (issue #661), matching
    /// the Rust builder's pre-existing `is_required` (`TypeArity::List`
    /// already returns `false` there — `crates/cratestack-macros/src/
    /// builder/fields.rs`). Scalar/required fields are unaffected —
    /// `builder_required` equals the old `required` value for every field
    /// this repo's fixtures exercise apart from `Plain`-kind lists.
    pub(crate) builder_required: bool,
    /// Whether an unset list backing field needs `?? <Elem>[]` in
    /// `build()` to produce a non-nullable list — true for every list
    /// field except a `Patch`-kind one (`is_patch`, e.g.
    /// `UpdatePostInput.tags`), where the backing field must stay `null`
    /// on the wire: that's the pre-existing "this field was never
    /// touched" representation every other optional/patch field already
    /// relies on, and defaulting it to `[]` would silently turn
    /// "untouched" into "set to an empty list" for update-input callers.
    /// A `ProjectionModel`-kind list field (e.g. `Post.tags`) has the same
    /// nullable `dart_type` as a `Patch` field (both force nullability for
    /// unrelated reasons — see `dart_field_type`), but is *not* a patch:
    /// an unset list there defaults to `[]`, matching the Rust model
    /// builder's `unwrap_or_default()` for the identical field (issue
    /// #661 AC1 — "an unset list field builds as `[]` in both languages",
    /// which does not carve out model classes).
    pub(crate) list_needs_default: bool,
    /// The list's element Dart type, e.g. `String` for a `String[]` field —
    /// `dart_type` stripped of its outer `List<...>` (and trailing `?`, for
    /// nullable list types). Empty string when `is_list` is `false`
    /// (unused by the template in that case).
    pub(crate) list_elem_type: String,
    /// `add{Field}` — the fluent append-setter name (issue #661), derived
    /// *mechanically* from `identifier` (capitalize the first character,
    /// no singularization: `tags` -> `addTags`, `children` ->
    /// `addChildren`). Empty string when `is_list` is `false` (unused by
    /// the template in that case). A schema field literally named
    /// `add{Field}` is rejected at parse time (`cratestack-parser`'s
    /// builder-name-collision check), so this name can never collide with
    /// another generated member.
    pub(crate) add_setter: String,
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
        is_list: bool,
        is_patch: bool,
        is_optional: bool,
        from_wire_expr: String,
        to_wire_expr: String,
    ) -> Self {
        let builder_setter = if identifier == "build" {
            "setBuild".to_owned()
        } else {
            identifier.clone()
        };
        let builder_backing_type = if dart_type.ends_with('?') {
            dart_type.clone()
        } else {
            format!("{dart_type}?")
        };
        let builder_cast_needed = builder_backing_type != dart_type;
        let builder_required = required && !is_list;
        let list_needs_default = is_list && !is_patch;
        let list_elem_type = if is_list {
            list_element_type(&dart_type).unwrap_or_default()
        } else {
            String::new()
        };
        let add_setter = if is_list {
            format!("add{}", capitalize_first(&identifier))
        } else {
            String::new()
        };
        let patch_touch = patch_touch_fields(&identifier, is_patch, is_optional);
        FieldView {
            identifier,
            wire_name,
            dart_type,
            required,
            from_wire_expr,
            to_wire_expr,
            builder_setter,
            builder_backing_type,
            builder_cast_needed,
            is_list,
            builder_required,
            list_needs_default,
            list_elem_type,
            add_setter,
            is_nullable_patch_field: patch_touch.is_nullable_patch_field,
            touch_flag_identifier: patch_touch.touch_flag_identifier,
            wire_write_condition: patch_touch.wire_write_condition,
        }
    }
}

/// Strips a list `dart_type`'s outer `List<...>` (and a trailing `?`, for
/// a nullable list type such as `Patch`/`ProjectionModel` fields' `List<
/// String>?`) down to the bare element type, e.g. `List<String>?` ->
/// `String`. Every list `dart_type` this crate ever produces
/// (`dart_types::dart_type`'s `TypeArity::List` arm) has exactly this
/// shape, so the strip is unconditional rather than a fallible parse.
fn list_element_type(dart_type: &str) -> Option<String> {
    let base = dart_type.strip_suffix('?').unwrap_or(dart_type);
    base.strip_prefix("List<")
        .and_then(|rest| rest.strip_suffix('>'))
        .map(str::to_owned)
}

/// Uppercases just the first character — the mechanical transform behind
/// `add_setter` (`tags` -> `Tags`, prefixed with `add`). `identifier` is
/// always non-empty (every schema field has a name) and already
/// Dart-identifier-safe (`idents::dart_identifier`), so a plain
/// first-char-uppercase is enough; no full case-conversion pass is needed
/// since `identifier` is already camelCase.
fn capitalize_first(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
