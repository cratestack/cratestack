//! Serializable render contexts for the `riverpod` preset's fan-out
//! templates (`templates/riverpod/*.j2`). Reuses `crate::views`'s
//! sub-views (`EnumView`, `DataClassView`, `SelectionModelView`,
//! `ModelApiView`, `ModelAccessorView`, `ProcedureView`) verbatim — only
//! how they're grouped per output file is new here.
use serde::Serialize;

use crate::views::{
    DataClassView, EnumView, ModelAccessorView, ModelApiView, ProcedureView, SelectionModelView,
};

/// Renders one `lib/src/models/<model>.dart`. `selection` is only used
/// for its `ProjectedX` fields here — the `Selection`/`IncludeSelection`
/// classes render from `QueriesFileContext` instead (see its doc for
/// why).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ModelFileContext {
    pub(crate) client_class_name: String,
    pub(crate) provider_prefix: String,
    pub(crate) imports: Vec<String>,
    /// `part '<file_stem>.g.dart';` target (issue #302) — the
    /// `build_runner`-expanded companion this file's `@riverpod`
    /// annotations need. Rendered as a plain string rather than a bool
    /// gate: every riverpod-preset model file carries at least the
    /// `get`/`list` providers, so the directive is unconditional.
    pub(crate) part_file_name: String,
    /// `part '<file_stem>.mapper.dart';` target (issue #325) — the
    /// `dart_mappable_builder`-expanded companion every generated data
    /// class's `@MappableClass()` needs, run in the same `build_runner`
    /// pass as `part_file_name`'s `riverpod_generator` output above.
    pub(crate) mapper_part_file_name: String,
    pub(crate) enum_types: Vec<EnumView>,
    pub(crate) data_classes: Vec<DataClassView>,
    pub(crate) selection: SelectionModelView,
    pub(crate) model_api: ModelApiView,
    pub(crate) accessor: ModelAccessorView,
    /// Issue #302's per-operation `@riverpod` providers, built on top of
    /// `accessor`/`model_api` — see `crate::riverpod::provider_naming`'s
    /// module doc for the naming/collision rule.
    pub(crate) operations: ModelOperationsView,
    /// Issue #331: `model_providers.dart.j2` is `{% include %}`d
    /// verbatim from both `rest_model.dart.j2` and `rpc_model.dart.j2`
    /// (see `build_model_file`'s `is_rest` parameter) — REST's `get`/
    /// `list` providers forward a typed `CratestackFetchQuery`/
    /// `CratestackListQuery` (already imported unconditionally on the
    /// REST path via `../queries.dart`), RPC's `list` provider forwards
    /// an `IMap<String, Object?>` filter/pagination bag instead (no
    /// RPC-side typed query builder exists — see this story's PR body
    /// for why `IMap`, not a bare `Map`: the same missing-value-equality
    /// bug this story's REST fix addresses on `CratestackListQuery`
    /// would otherwise reappear on the RPC `list` provider's own family
    /// argument). One shared template with this flag, not two forked
    /// templates, since every other line (the five providers' shapes,
    /// the write controllers) is identical either way.
    pub(crate) is_rest: bool,
}

/// Collision-checked identifiers (`crate::riverpod::provider_naming`) for
/// one model's five `@riverpod` operation providers — always all five,
/// mirroring `model_api`'s own unconditional list/get/create/update/
/// delete surface (this generator's REST/RPC paths never gate `create`
/// on `@@allow`; only the gRPC path does — see
/// `crate::naming::model_allows_create`'s doc).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ModelOperationsView {
    /// `@riverpod Future<Model> {get_function_name}(Ref ref, K id)`.
    pub(crate) get_function_name: String,
    /// `@riverpod Future<List<Model>|Page<Model>> {list_function_name}(Ref ref)`.
    pub(crate) list_function_name: String,
    /// `@riverpod class {create_controller_name} extends _$...`.
    pub(crate) create_controller_name: String,
    pub(crate) update_controller_name: String,
    pub(crate) delete_controller_name: String,
}

/// Renders `lib/src/models/shared_types.dart` — always emitted (unlike
/// the per-locus files, it isn't conditional on the partition finding
/// something to share): it also carries the `Page`/`PageInfo` wrapper
/// types, which every `@@paged` model's own file needs regardless of
/// whether anything else is genuinely shared.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SharedTypesFileContext {
    /// Extra imports beyond the always-present `dart:typed_data`/
    /// `../runtime.dart` (hardcoded in the template) — only needed for
    /// the rare case of a shared `type` block directly naming a `model`
    /// (issue #137's `type_references_model.cstack` shape).
    pub(crate) imports: Vec<String>,
    pub(crate) enum_types: Vec<EnumView>,
    pub(crate) data_classes: Vec<DataClassView>,
}

/// Renders `lib/src/queries.dart` (REST only) — the transport's generic
/// query-builder helpers (unchanged from the `default` preset) plus,
/// still here rather than per-model, every model's `Selection`/
/// `IncludeSelection` pair. Those two classes reference each other's
/// private `_node` field across models with a relation (e.g.
/// `PostSelection.author()` reaches into `AuthorIncludeSelection._node`)
/// — Dart's `_`-prefixed privacy is per-*file*, so splitting them into
/// separate per-model files breaks that cross-reference. `ProjectedX`
/// has no such private cross-reference (`ProjectedPost.author` calls the
/// public `ProjectedAuthor.fromWire` factory), so it stays relocated
/// into the owning model's own file.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct QueriesFileContext {
    pub(crate) imports: Vec<String>,
    pub(crate) selection_models: Vec<SelectionModelView>,
}

/// Renders `lib/src/procedures.dart` — always emitted (mirrors the
/// `default` preset, which always renders `ProceduresApi` even when the
/// schema declares zero procedures).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProceduresFileContext {
    pub(crate) client_class_name: String,
    pub(crate) provider_prefix: String,
    pub(crate) imports: Vec<String>,
    /// `part 'procedures.g.dart';` — see `ModelFileContext::part_file_name`.
    /// Always `"procedures.g.dart"` (never gated on `!procedures.is_empty()`
    /// like the `default` preset's own `{% if procedures %}`-free
    /// `ProceduresApi` class isn't either — an empty `part` file is a
    /// harmless no-op `build_runner` output).
    pub(crate) part_file_name: String,
    /// `part 'procedures.mapper.dart';` — see
    /// `ModelFileContext::mapper_part_file_name`'s doc. The *value* is
    /// always `"procedures.mapper.dart"`, but unlike `part_file_name`
    /// above, `rest_procedures.dart.j2`/`rpc_procedures.dart.j2` only
    /// emit the directive itself when `data_classes` is non-empty — see
    /// those templates' own comment for why an unconditional directive
    /// here would be a real `flutter analyze` failure on a schema with
    /// zero procedures.
    pub(crate) mapper_part_file_name: String,
    pub(crate) enum_types: Vec<EnumView>,
    pub(crate) data_classes: Vec<DataClassView>,
    /// Issue #302: `procedures[i]` and `procedure_operations[i]` are the
    /// same procedure, in the same order — kept as two parallel `Vec`s
    /// rather than folding `ProcedureOperationView` into `ProcedureView`
    /// so `crate::builders_model::build_procedure` (shared with the
    /// `default` preset) never needs to know about riverpod-only naming.
    pub(crate) procedures: Vec<ProcedureView>,
    pub(crate) procedure_operations: Vec<ProcedureOperationView>,
}

/// One procedure's `@riverpod` provider identifier plus which shape it
/// needs — a function (`ProcedureKind::Query`) or a controller class
/// (`ProcedureKind::Mutation`), matching `ProcedureView::kind`'s already-
/// computed `"query"`/`"mutation"` literal so the template can gate on
/// the same field it already has.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProcedureOperationView {
    pub(crate) kind: &'static str,
    /// The function name (query) or class name (mutation) —
    /// collision-checked the same way as `ModelOperationsView`'s fields.
    pub(crate) symbol: String,
    /// `ProcedureView::return_type` with `dart_type(..., force_nullable:
    /// true)` — a mutation controller's `build()` always starts at
    /// `null` (no result yet), so its declared state type must be
    /// nullable even when the procedure's own return type isn't. Can't
    /// just template-append `?` to `return_type`: a procedure whose
    /// schema return type is itself already optional (`Foo?`) already
    /// carries a trailing `?`, and Dart doesn't allow `Foo??`. Only
    /// meaningful for `kind == "mutation"`.
    pub(crate) nullable_return_type: String,
    /// Dart method name for the mutation controller's own action method
    /// — `ProcedureView::method_name` verbatim, *unless* it collides
    /// with a name `riverpod_generator`'s `_$AsyncClassModifier` base
    /// class already declares (`update`, confirmed empirically to
    /// produce a real `invalid_override` `dart analyze` error — see
    /// `templates/riverpod/model_providers.dart.j2`'s `save` rename for
    /// the same collision on the model side, where it's unconditional
    /// rather than schema-dependent). Only meaningful for
    /// `kind == "mutation"`; query providers are top-level functions,
    /// not class methods, so they're never subject to this override
    /// check.
    pub(crate) mutation_method_name: String,
}

/// Renders `lib/src/client.dart` — the package-wide DI surface
/// (`xAdapterProvider`/`xClientProvider`/`{{ client_class_name }}`) that
/// every per-model `Provider<XApi>` watches. Never per-model.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ClientFileContext {
    pub(crate) client_class_name: String,
    pub(crate) provider_prefix: String,
    pub(crate) base_path_literal: String,
    pub(crate) imports: Vec<String>,
    pub(crate) model_accessors: Vec<ModelAccessorView>,
}

/// Renders `lib/<package_name>.dart`, the library entrypoint.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct LibraryFileContext {
    pub(crate) exports: Vec<String>,
}
