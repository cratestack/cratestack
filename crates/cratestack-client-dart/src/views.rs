use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TemplateContext {
    pub(crate) package_name: String,
    pub(crate) client_class_name: String,
    pub(crate) provider_prefix: String,
    pub(crate) base_path_literal: String,
    /// Escaped Dart string-literal body (no surrounding quotes, same
    /// convention as `base_path_literal`) of the schema's SHA-256, or
    /// `None` when the generator wasn't given one (issue #178) — the
    /// `constants.dart` template renders `cratestackSchemaSha256` as
    /// `null` in that case, and every runtime adapter omits the
    /// `x-cratestack-schema-sha` header rather than sending an empty
    /// value.
    pub(crate) schema_sha256: Option<String>,
    pub(crate) enum_types: Vec<EnumView>,
    pub(crate) data_classes: Vec<DataClassView>,
    pub(crate) selection_groups: Vec<SelectionGroupView>,
    pub(crate) selection_models: Vec<SelectionModelView>,
    pub(crate) model_accessors: Vec<ModelAccessorView>,
    pub(crate) model_apis: Vec<ModelApiView>,
    pub(crate) procedures: Vec<ProcedureView>,
    pub(crate) query_procedures: Vec<ProcedureView>,
    pub(crate) mutation_procedures: Vec<ProcedureView>,
    pub(crate) sample_model: Option<SampleModelView>,
    /// `config.preset == DartPreset::Riverpod` (issue #303). `README.md.j2`
    /// is one of the files `crate::riverpod::generate_package` reuses
    /// verbatim from `generate_default_package` (see that module's doc),
    /// so this flag is how the shared template gates the riverpod-only
    /// `build_runner` section without forking the template — `false` for
    /// every `DartPreset::Default` render, which is exactly what keeps
    /// the default preset's output byte-identical (`tests/snapshot.rs`).
    pub(crate) is_riverpod_preset: bool,
    /// `config.native_cbor` (issue #563) — gates whether the generated
    /// runtime imports `package:cbor` (sync, pure Dart) or
    /// `cratestack_cbor` (async, native) and whether `pubspec.yaml`
    /// depends on `cbor` or `cratestack_cbor`. `false` for every render
    /// that doesn't pass it explicitly, which is what keeps the default
    /// output byte-identical — see `DartGeneratorConfig::native_cbor`'s
    /// doc comment for the full rationale.
    pub(crate) native_cbor: bool,
    /// `cratestack_cbor: {{ cratestack_cbor_version_requirement }}` in
    /// `pubspec.yaml` when `native_cbor` is set — `^{CARGO_PKG_VERSION}`
    /// of this crate, matching how `cratestack_cbor`'s own
    /// `dart-packages/cratestack_cbor/pubspec.yaml` version is bumped in
    /// lockstep with the Cargo workspace version by `just bump` (see the
    /// justfile's `':(glob)dart-packages/*/pubspec.yaml'` rewrite), the
    /// same lockstep convention `cratestack-client-typescript`'s
    /// `refine_version_requirement` already uses for `@cratestack/refine`.
    /// Empty string when `native_cbor` is `false` (unused by the template
    /// in that case).
    pub(crate) cratestack_cbor_version_requirement: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EnumView {
    pub(crate) name: String,
    pub(crate) variants: Vec<EnumVariantView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EnumVariantView {
    pub(crate) identifier: String,
    pub(crate) wire_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DataClassView {
    pub(crate) name: String,
    pub(crate) has_fields: bool,
    pub(crate) fields: Vec<FieldView>,
}

// `FieldView` lives in `crate::field_view` (split out per the repo's
// 200-LoC file convention) and is re-exported here so every existing
// `use crate::views::{..., FieldView}` call site keeps working unchanged.
pub(crate) use crate::field_view::FieldView;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SelectionGroupView {
    pub(crate) field_group_name: String,
    pub(crate) fields: Vec<ConstantView>,
    pub(crate) include_group_name: String,
    pub(crate) includes: Vec<ConstantView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ConstantView {
    pub(crate) const_name: String,
    pub(crate) value: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ModelAccessorView {
    pub(crate) accessor: String,
    pub(crate) api_class_name: String,
    pub(crate) provider_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SampleModelView {
    pub(crate) model_name: String,
    pub(crate) accessor: String,
    pub(crate) field_group_name: String,
    pub(crate) include_group_name: String,
    pub(crate) first_field: Option<ConstantView>,
    pub(crate) first_include: Option<ConstantView>,
}

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
    /// before Stage 3) and a template `{% if %}` can't destructure an
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

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SelectionModelView {
    pub(crate) model_name: String,
    pub(crate) selection_class_name: String,
    pub(crate) include_selection_class_name: String,
    pub(crate) projected_class_name: String,
    pub(crate) scalar_fields: Vec<SelectedFieldAccessorView>,
    pub(crate) relations: Vec<SelectedRelationAccessorView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SelectedFieldAccessorView {
    pub(crate) identifier: String,
    pub(crate) wire_name: String,
    pub(crate) dart_type: String,
    pub(crate) decode_expr: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SelectedRelationAccessorView {
    pub(crate) identifier: String,
    pub(crate) wire_name: String,
    pub(crate) target_selection_class_name: String,
    pub(crate) target_include_selection_class_name: String,
    pub(crate) target_projected_class_name: String,
    pub(crate) is_list: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProcedureView {
    /// Raw schema procedure name (e.g. `publishPost`). Used to build
    /// the server-side op id `procedure.<name>` in RPC mode and to
    /// build the REST URL `/$procs/<name>` in REST mode.
    pub(crate) name: String,
    pub(crate) method_name: String,
    pub(crate) args_name: String,
    pub(crate) return_type: String,
    pub(crate) route: String,
    pub(crate) return_decode_expr: String,
    pub(crate) kind: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) enum DataClassKind {
    Plain,
    Patch,
    ProjectionModel,
}
