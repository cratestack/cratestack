//! `--refine` (issue #571): the `@cratestack/refine` resource manifest.
//!
//! `@cratestack/refine` is a runtime `DataProvider` over this crate's
//! generated REST client. It needs four facts per resource that the
//! generated client encodes **only in its TypeScript types** and exposes
//! nowhere at runtime — there is no `client.widgets.$meta` to introspect:
//!
//!   * which generated model API backs the resource (`client.widgets`),
//!   * the `@id` field's name (refine assumes `id`; cratestack's `@id` may
//!     be on any field),
//!   * whether the model is `@@paged` (decides whether `.list()` resolves
//!     to `Page<T>` with a real `totalCount` or a bare `T[]`),
//!   * which field carries `@version`, if any (decides whether
//!     `update`/`deleteOne` send an `If-Match`).
//!
//! #577 shipped that manifest as something the consumer hand-writes. This
//! module emits it instead: every one of those four facts is already in
//! the schema this generator is reading, so a hand-written copy is a
//! second source of truth that drifts silently the moment a model gains
//! `@version` or `@@paged`.
//!
//! Scope is REST + `--preset default` only, enforced in
//! `crate::generator` rather than here — see
//! `TypeScriptGeneratorError::RefineRequiresRest`/`RefineUnsupportedPreset`
//! for why each other combination cannot work.

use cratestack_core::{Model, Schema};
use serde::Serialize;

use crate::naming::{pluralize, to_camel_case};
use crate::types::{is_paged_model, primary_key_field, version_field};

/// One `ResourceConfig` entry in the emitted `ResourceMap`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct RefineResourceView {
    /// Doubles as the refine resource *name* (a string key) and the
    /// generated client's *accessor* (a property on the client class).
    /// They are deliberately the same value — `client.widgets` is what a
    /// resource named `widgets` binds to, and keeping them in lockstep is
    /// what makes the emitted map readable next to a hand-written one.
    ///
    /// Derived identically to `crate::views::ModelApiView::accessor`
    /// (`pluralize(to_camel_case(name))`) — it has to be, since the
    /// template dereferences it off the client. Two model names can
    /// normalize to the same accessor (`UserGroup` / `User_Group`, the
    /// collision `crate::views::disambiguate_model_api_keys` documents),
    /// but that is already a duplicate-class-member TypeScript error in
    /// the generated `client.ts` itself — every accessor is a `readonly`
    /// property on one class — so this module inherits that failure rather
    /// than introducing a new one, and deliberately does not disambiguate
    /// (a renamed resource key would silently stop matching the `<Refine
    /// resources={...}>` name the app declares).
    pub(crate) accessor: String,
    /// The schema's `@id` field name, verbatim.
    pub(crate) primary_key: String,
    /// Whether the model declares `@@paged`.
    pub(crate) paged: bool,
    /// The `@version` field's name, or `None` when the model has none —
    /// the template omits `versionField` entirely in that case, which is
    /// what `ResourceConfig` documents as "send no `If-Match`".
    pub(crate) version_field: Option<String>,
}

pub(crate) fn build_refine_resources(schema: &Schema) -> Vec<RefineResourceView> {
    schema.models.iter().map(build_refine_resource).collect()
}

fn build_refine_resource(model: &Model) -> RefineResourceView {
    // Same `expect` as `crate::views::build_model_api` — a schema that
    // reached codegen has been through the parser's semantic checker,
    // which requires an `@id` field on every model.
    let primary_key = primary_key_field(model).expect("validated schemas always have an id field");
    RefineResourceView {
        accessor: pluralize(&to_camel_case(&model.name)),
        primary_key: primary_key.name.clone(),
        paged: is_paged_model(model),
        version_field: version_field(model).map(|field| field.name.clone()),
    }
}
