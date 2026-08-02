//! Serialize view types the `swr` preset's minijinja templates render
//! against. Mirrors the split `crate::views` already has for the default
//! preset (plain data in, no rendering logic), scoped to the shapes the
//! new per-file layout needs on top of it.

use serde::Serialize;

use crate::views::{EnumView, InterfaceView, ModelApiView, ProcedureView};

/// A single `import type { .. } from "<path>";` line. `path` is always
/// computed in Rust (never string-built in a template) so every template
/// stays a dumb printer — see `crate::swr::context::build_imports`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SwrImport {
    pub(crate) path: String,
    pub(crate) type_names: Vec<String>,
    pub(crate) type_names_joined: String,
}

impl SwrImport {
    pub(crate) fn new(path: impl Into<String>, mut type_names: Vec<String>) -> Self {
        type_names.sort();
        let type_names_joined = type_names.join(", ");
        Self {
            path: path.into(),
            type_names,
            type_names_joined,
        }
    }
}

/// `src/models/shared.ts`'s content: `Page`/`PageInfo` are always static
/// boilerplate (every template gets them for free, not from this struct);
/// `enums`/`interfaces` are exactly the enums/`type` blocks the ownership
/// computation (`crate::swr::ownership`) placed here because 2+ consumers
/// reach them (or 0 — see that module's doc comment).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SwrSharedView {
    pub(crate) enums: Vec<EnumView>,
    pub(crate) interfaces: Vec<InterfaceView>,
    pub(crate) imports: Vec<SwrImport>,
}

/// One entry in the model list `README.md`/`index.ts` iterate — just
/// enough to print an import line and a usage snippet, not the full
/// per-model file content (see [`SwrModelFileContext`] for that).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SwrModelSummary {
    pub(crate) name: String,
    pub(crate) file_stem: String,
    pub(crate) accessor: String,
    pub(crate) allows_create: bool,
    pub(crate) list_fn: String,
    pub(crate) get_fn: String,
    pub(crate) create_fn: String,
    pub(crate) update_fn: String,
    pub(crate) delete_fn: String,
}

/// `src/procedures.ts`'s content.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SwrProceduresView {
    /// Enums/`type` blocks owned solely by procedures (see
    /// `crate::swr::ownership`) — defined inline here, not imported.
    pub(crate) owned_enums: Vec<EnumView>,
    pub(crate) owned_interfaces: Vec<InterfaceView>,
    pub(crate) imports: Vec<SwrImport>,
    /// Each procedure's own `<Name>Args` wrapper — always procedure-owned
    /// (`crate::naming::procedure_wrapper_name` already scopes the name),
    /// never subject to the shared/owned computation.
    pub(crate) args_interfaces: Vec<InterfaceView>,
    pub(crate) procedures: Vec<ProcedureView>,
}

/// The flat context every *fixed* (non-per-model) `swr` template renders
/// against: `package.json`, `README.md`, the reused `runtime.ts` /
/// `queries.ts` / `links.ts` / `cbor-*.ts` / `stream-terminal.ts`
/// templates (same field names the default preset's `TemplateContext`
/// exposes for those, so they need no template changes at all),
/// `src/models/shared.ts`, `src/procedures.ts`, and `src/index.ts`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SwrSchemaContext {
    pub(crate) package_name: String,
    pub(crate) base_path: String,
    pub(crate) schema_sha256: String,
    pub(crate) shared: SwrSharedView,
    pub(crate) models: Vec<SwrModelSummary>,
    pub(crate) procedures_file: SwrProceduresView,
}

/// The per-model context `swr-models-{rest,rpc}.ts.j2` renders once per
/// model. `model` reuses `crate::views::ModelApiView` verbatim (route,
/// primary key type, list-return shape, ...) — the plain functions call
/// the exact same runtime methods the default preset's client classes do,
/// just as free functions instead of class methods (issue #304 explicitly
/// does not change operation semantics).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SwrModelFileContext {
    pub(crate) file_stem: String,
    pub(crate) model: ModelApiView,
    pub(crate) model_interface: InterfaceView,
    pub(crate) create_input: Option<InterfaceView>,
    pub(crate) update_input: InterfaceView,
    /// Enums/`type` blocks owned solely by this model — inlined here,
    /// never imported (see `crate::swr::ownership`'s module doc for why
    /// this can never miss a cross-file reference).
    pub(crate) owned_enums: Vec<EnumView>,
    pub(crate) owned_interfaces: Vec<InterfaceView>,
    pub(crate) imports: Vec<SwrImport>,
    pub(crate) list_fn: String,
    pub(crate) get_fn: String,
    pub(crate) create_fn: String,
    pub(crate) update_fn: String,
    pub(crate) delete_fn: String,
}
