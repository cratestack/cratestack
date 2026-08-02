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
    pub(crate) enum_types: Vec<EnumView>,
    pub(crate) data_classes: Vec<DataClassView>,
    pub(crate) selection: SelectionModelView,
    pub(crate) model_api: ModelApiView,
    pub(crate) accessor: ModelAccessorView,
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
    pub(crate) enum_types: Vec<EnumView>,
    pub(crate) data_classes: Vec<DataClassView>,
    pub(crate) procedures: Vec<ProcedureView>,
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
