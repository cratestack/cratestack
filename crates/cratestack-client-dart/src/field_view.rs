//! `FieldView` — one struct field's worth of view data, shared by every
//! `DataClassView` (`crate::views`) regardless of which builder produced it
//! (`crate::builders::build_data_class`, `crate::find_many_views`'s five
//! call sites). Split out from `crate::views` per the repo's 200-LoC file
//! convention once the fluent-builder-related fields below were added.

use serde::Serialize;

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
}

impl FieldView {
    pub(crate) fn new(
        identifier: String,
        wire_name: String,
        dart_type: String,
        required: bool,
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
        }
    }
}
