//! `ModelApiView`/`ComputedParamsFieldView` — the per-model `computedParams`
//! surface's view data (`docs/design/computed-fields.md`'s "Downstream"
//! section), shared by every REST/RPC/riverpod client-method template.
//! Split out from `crate::views` per the repo's 200-LoC file convention
//! (mirrors `crate::field_view`'s own split for the same reason) and
//! re-exported there so every existing `use crate::views::{...}` call
//! site keeps working unchanged.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ModelApiView {
    pub(crate) api_class_name: String,
    pub(crate) model_name: String,
    pub(crate) create_input_name: String,
    pub(crate) update_input_name: String,
    pub(crate) route: String,
    pub(crate) detail_route: String,
    pub(crate) primary_key_type: String,
    pub(crate) is_paged: bool,
    pub(crate) list_return_type: String,
    pub(crate) list_decode_expr: String,
    /// Whether the model declares at least one *parameterized*
    /// `@computed(params: <Type>?)` field (`docs/design/computed-fields.md`)
    /// — not merely `@computed` in general. Gates whether `get`/`list`
    /// render the optional `computedParams` parameter: the server 422s a
    /// `computedParams` key that doesn't name a parameterized field, so a
    /// model whose only computed fields are bare (no params type at all)
    /// must not accept the parameter in the first place — it could never
    /// be satisfied. A model with no computed fields at all obviously has
    /// no resolver to parameterize either, so the parameter is omitted
    /// entirely (rather than emitted-but-always-rejected) in both cases.
    pub(crate) has_parameterized_computed_fields: bool,
    /// `Some("{Model}ComputedParams")` when
    /// `has_parameterized_computed_fields` is `true`, `None` otherwise —
    /// the two are always in lockstep (`build_model_api` sets both from
    /// the same computation), kept as two fields rather than one because
    /// every template call site already branches on the bool (existed
    /// before the typed client computedParams surface — see
    /// `docs/design/computed-fields.md`'s "Downstream" section) and a
    /// template `{% if %}` can't destructure an
    /// `Option`'s inner value in the same expression. Names the typed
    /// per-model class (`docs/design/computed-fields.md`) that replaces
    /// the v1 untyped `Map<String, Object?>?` escape hatch on `get`/
    /// `list` -- one optional field per *parameterized* `@computed(params:
    /// <Type>?)` field, so the class only needs generating (and importing)
    /// where the gate is already `true`.
    pub(crate) computed_params_class_name: Option<String>,
    /// One entry per model field carrying `@computed(params: <Type>?)`,
    /// in declaration order -- empty exactly when
    /// `computed_params_class_name` is `None`. Drives both the class
    /// body (`templates/computed_params_class.dart.j2`) and the
    /// `get`/`list` call sites that fold it onto the wire.
    pub(crate) computed_params_fields: Vec<ComputedParamsFieldView>,
}

/// One field of a model's generated `{Model}ComputedParams` class --
/// see `ModelApiView::computed_params_fields`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ComputedParamsFieldView {
    /// Dart identifier for the params class's own field -- the computed
    /// field's own name (e.g. `proxyUrl`), same convention as every other
    /// generated field (`dart_identifier(&field.name)`, no case
    /// conversion: schema field names are already camelCase).
    pub(crate) identifier: String,
    /// The wire key this params entry is nested under inside the
    /// `computedParams` JSON object -- identical to `identifier` before
    /// Dart-identifier-escaping, i.e. the raw schema field name.
    pub(crate) wire_name: String,
    /// The declared params `type`'s Dart class name (e.g. `ProxyParams`)
    /// -- always a generated data class, never a scalar (the schema
    /// parser requires `@computed(params: <Type>?)`'s `<Type>` to be a
    /// declared `type`), so no `dart_type`/scalar-import mapping is
    /// needed here.
    pub(crate) params_type: String,
}
