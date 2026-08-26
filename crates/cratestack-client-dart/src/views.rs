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
    /// so this flag is how the shared template gates riverpod-only prose
    /// (its own `@riverpod`/`dart_mappable` codegen reasons for needing
    /// `build_runner`, on top of the `@CratestackBuilder(...)` reason every
    /// preset shares as of issue #668 phase 2 — see `setup.md.j2`'s "Code
    /// Generation" section) without forking the template.
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
    /// Whether any entry in `model_apis` has a `computed_params_class_name`
    /// — gates `models.dart.j2`'s `import 'dart:convert';`, needed by
    /// `<Model>ComputedParams.operator ==`/`hashCode`
    /// (`computed_params_class.dart.j2`, wire-equality via
    /// `jsonEncode(toWire())`). Unlike the riverpod preset's per-model
    /// files (which only import what that one model uses), this preset's
    /// `models.dart` is a single file for every model, so the import is
    /// gated on "does *any* model need it", not per-model.
    pub(crate) has_computed_params_class: bool,
    /// `cratestack_annotations: {{ cratestack_annotations_version_requirement }}`
    /// in `pubspec.yaml`'s `dependencies:` (issue #668 phase 2) —
    /// `^{CARGO_PKG_VERSION}` of this crate, same lockstep convention as
    /// `cratestack_cbor_version_requirement` above (`dart-packages/
    /// cratestack_annotations`'s own version is bumped alongside the Cargo
    /// workspace version by `just bump`'s `dart-packages/*/pubspec.yaml`
    /// rewrite). Unlike `cratestack_cbor_version_requirement`, never empty
    /// — every generated package now carries the `@CratestackBuilder`
    /// annotation on every data class, unconditionally.
    pub(crate) cratestack_annotations_version_requirement: String,
    /// `cratestack_builder: {{ cratestack_builder_version_requirement }}`
    /// in `pubspec.yaml`'s `dev_dependencies:`, alongside `build_runner` —
    /// see `cratestack_annotations_version_requirement`'s doc for the
    /// lockstep-versioning rationale, identical here.
    pub(crate) cratestack_builder_version_requirement: String,
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

// `DataClassView`/`DataClassKind` live in `crate::data_class_view` (split
// out per the repo's 200-LoC file convention) and are re-exported here so
// every existing `use crate::views::{..., DataClassView, DataClassKind}`
// call site keeps working unchanged.
pub(crate) use crate::data_class_view::{DataClassKind, DataClassView};

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

// `ModelApiView`/`ComputedParamsFieldView` live in
// `crate::computed_params_view` (split out per the repo's 200-LoC file
// convention, same reasoning as `FieldView`'s split above) and are
// re-exported here so every existing `use crate::views::{...,
// ModelApiView, ...}` call site keeps working unchanged.
pub(crate) use crate::computed_params_view::{ComputedParamsFieldView, ModelApiView};

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
