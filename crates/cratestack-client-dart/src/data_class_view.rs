//! `DataClassView`/`DataClassKind` — one generated Dart data class's view
//! data (models, `Create{Model}Input`, `Update{Model}Input`, `{Model}Where`/
//! `{Model}OrderByClause`/`{Model}FindMany`, `type` blocks, per-procedure
//! argument classes), and the enum that discriminates which builder-codegen
//! rules apply to it. Split out from `crate::views` per the repo's 200-LoC
//! file convention (mirrors `crate::field_view`'s own split for the same
//! reason) and re-exported there so every existing `use crate::views::{...,
//! DataClassView, DataClassKind}` call site keeps working unchanged.

use serde::Serialize;

use crate::field_view::FieldView;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DataClassView {
    pub(crate) name: String,
    pub(crate) has_fields: bool,
    /// The exact text between `@CratestackBuilder(...)`'s parens (issue
    /// #668 phase 2/3) — empty string for the fully-default case, so the
    /// template can always write `@CratestackBuilder({{ builder_args }})`
    /// unconditionally. Bundles the three pieces of builder-codegen
    /// knowledge that aren't recoverable from the Dart source
    /// `package:cratestack_builder` reads:
    ///
    /// - `listDefaults` — a projection model's list field and a patch
    ///   input's list field emit byte-identical Dart (`this.tags` +
    ///   `final List<String>? tags;`), yet must build differently: an
    ///   unset list defaults to `[]` everywhere except a patch/
    ///   `Update{Model}Input` class, where it must stay `null` so
    ///   "untouched" stays distinguishable from "explicitly set to empty"
    ///   (cratestack#661/#663).
    /// - `touchFlagFields` — which fields carry a Rust-synthesized sibling
    ///   `{field}IsSet` touch flag, so the generated setter can mark it
    ///   touched too. Threaded explicitly rather than recovered from the
    ///   `{field}`/`{field}IsSet` naming shape: a schema can legally
    ///   declare an unrelated `bool` field that happens to end in `IsSet`
    ///   (`cratestack-parser`'s `tests_patch_touch_flag_collisions.rs`),
    ///   and a name-based heuristic can't tell the two apart.
    /// - `nonDefaultingListFields` — a to-many *relation*-valued field on a
    ///   model class (issue #661): Rust's own model builder drops relation
    ///   fields entirely (`scalar_model_fields`), so defaulting an unset
    ///   one to `[]`/giving it an `add{Field}` setter has no Rust
    ///   counterpart and conflates "not included in the response" with
    ///   "included and empty".
    ///
    /// See `crate::builders::build_data_class`'s `render_builder_args`, the
    /// one place that computes this.
    pub(crate) builder_args: String,
    /// Whether this class gets `@CratestackBuilder(...)` at all (issue
    /// #668 phase 2). `true` for every call site — including
    /// `crate::riverpod::build_shared_types_file`: an orphan `type` block
    /// can land in `lib/src/models/shared_types.dart` (defaults to
    /// `Owner::Shared`, see `tests/fixtures/riverpod_shared_type_orphan
    /// .cstack`), and origin/main's inline builder emission covered that
    /// file too (`shared_types.dart` is not builder-free — see the fixture
    /// above), so this file is not a deliberate exception.
    pub(crate) emit_builder: bool,
    pub(crate) fields: Vec<FieldView>,
}

#[derive(Clone, Copy)]
pub(crate) enum DataClassKind {
    Plain,
    Patch,
    ProjectionModel,
}
