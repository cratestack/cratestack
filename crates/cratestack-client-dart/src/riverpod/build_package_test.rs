//! Builds `test/{{ package_name }}_test.dart` for the riverpod preset —
//! issue #302's real, executed proof (not just a Rust-side text
//! assertion) that overriding `{{ provider_prefix }}AdapterProvider`
//! alone — the pre-existing Dio/adapter override point, unmodified by
//! this story — changes what a generated `@riverpod` operation provider
//! actually returns. Reuses `crate::context::build_template_context`
//! (the `default` preset's own context builder) for the pre-existing
//! smoke content (options/query-builder checks, the model/procedure
//! roll call, the `sample_model` wire round-trip) so this file doesn't
//! silently drop coverage the `default` preset's own test already has —
//! only the override-proof `test(...)` block at the end is new.
//!
//! That pre-existing smoke content is always wrapped in its own real
//! `test(...)` case in the emitted file (not bare top-level `assert`s,
//! and not conditional on `override_proof`) — a schema with no models at
//! all, or whose first model in schema order is paged, gets no
//! `override_proof` (see `first_model_list_provider`'s doc comment
//! below) and therefore no override-propagation `test(...)` block; with
//! bare asserts and an unconditionally-imported `flutter_riverpod`, that
//! shape of schema used to leave both `flutter_riverpod` and
//! `flutter_test` unused, failing the generated package's own
//! `flutter analyze` (`flutter_lints/flutter.yaml` enables
//! `unused_import`). `flutter_riverpod`'s import is now conditional on
//! `override_proof` in both `rest_package_test.dart.j2` and
//! `rpc_package_test.dart.j2`, same as `fast_immutable_collections`
//! already was in the RPC template.
use serde::Serialize;

use crate::config::{DartGeneratorConfig, DartGeneratorError};
use crate::context::build_template_context;
use crate::views::TemplateContext;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PackageTestFileContext {
    #[serde(flatten)]
    pub(crate) base: TemplateContext,
    pub(crate) override_proof: Option<OverrideProofView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OverrideProofView {
    pub(crate) model_name: String,
    /// The full `...Provider` variable name — `{{ list_function_name }}Provider`
    /// — not just the bare function name, since the template reads
    /// `<name>.future` directly off the generated provider instance.
    pub(crate) list_provider_name: String,
    pub(crate) adapter_provider_name: String,
}

pub(crate) fn build_package_test_file(
    schema: &cratestack_core::Schema,
    config: &DartGeneratorConfig,
    provider_prefix: &str,
    // `(model_name, list_function_name, is_paged)` for the *first* model
    // in schema order — matches `mod.rs`'s own model loop order, so the
    // name is exactly what `reserve_operation_symbol` actually assigned
    // that model's `list` provider, not a value recomputed in isolation.
    // A paged first model gets no override-propagation proof at all: the
    // fake adapter's canned response (`[{}]`) is shaped for `list()`'s
    // plain-list decode path, not `Page<T>.fromWire`'s `{items,
    // pageInfo}` shape.
    first_model_list_provider: Option<(&str, &str, bool)>,
) -> Result<PackageTestFileContext, DartGeneratorError> {
    let base = build_template_context(schema, config)?;
    let override_proof = first_model_list_provider
        .filter(|(_, _, is_paged)| !is_paged)
        .map(|(model_name, list_function_name, _)| OverrideProofView {
            model_name: model_name.to_owned(),
            list_provider_name: format!("{list_function_name}Provider"),
            adapter_provider_name: format!("{provider_prefix}AdapterProvider"),
        });
    Ok(PackageTestFileContext {
        base,
        override_proof,
    })
}
