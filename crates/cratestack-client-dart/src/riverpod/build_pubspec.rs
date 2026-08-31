//! Builds the riverpod preset's own `pubspec.yaml` (issue #302). Can't
//! reuse `templates/pubspec.yaml.j2` verbatim (the way this preset reuses
//! `README.md`/`CHANGELOG.md`/etc. — see `crate::riverpod`'s module doc):
//! that template is the `default` preset's own file, and its output is a
//! byte-identical contract (`tests/snapshot.rs`) this story must not
//! touch. `riverpod_annotation` (a real dependency — the generated
//! `@riverpod`/`Ref` source uses it directly, not just at codegen time)
//! plus the `riverpod_generator`/`build_runner` dev dependencies this
//! preset's `part '<file>.g.dart'` directives need are riverpod-only, so
//! they get their own template instead of a conditional branch inside the
//! shared one. `flutter_riverpod`/`cbor`/`dio`/`flutter_lints` stay
//! exactly as `pubspec.yaml.j2` already pins them — this is additive, not
//! a redesign.
use crate::config::DartGeneratorConfig;
use crate::package_floors::{
    CRATESTACK_ANNOTATIONS_FLOOR, CRATESTACK_BUILDER_FLOOR, CRATESTACK_CBOR_FLOOR, requirement,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PubspecFileContext {
    pub(crate) package_name: String,
    /// `config.native_cbor` (issue #563) — see
    /// `crate::views::TemplateContext::native_cbor`'s doc comment; this
    /// preset needs its own copy because it needs its own `pubspec.yaml`
    /// (see this module's doc), not because the flag's meaning differs.
    pub(crate) native_cbor: bool,
    /// See `crate::views::TemplateContext::cratestack_cbor_version_requirement`.
    pub(crate) cratestack_cbor_version_requirement: String,
    /// See `crate::views::TemplateContext::cratestack_annotations_version_requirement`.
    pub(crate) cratestack_annotations_version_requirement: String,
    /// See `crate::views::TemplateContext::cratestack_builder_version_requirement`.
    pub(crate) cratestack_builder_version_requirement: String,
}

pub(crate) fn build_pubspec_file(config: &DartGeneratorConfig) -> PubspecFileContext {
    // cratestack#779: a floor, not `^{CARGO_PKG_VERSION}` — same
    // constant this preset's `default` sibling emits
    // (`crate::context::build_template_context`), for the same reason.
    let cratestack_cbor_version_requirement = if config.native_cbor {
        requirement(CRATESTACK_CBOR_FLOOR)
    } else {
        String::new()
    };
    PubspecFileContext {
        package_name: config.library_name.clone(),
        native_cbor: config.native_cbor,
        cratestack_cbor_version_requirement,
        // cratestack#754: API-compatibility floors, deliberately not
        // `^{CARGO_PKG_VERSION}` — see `crate::package_floors`.
        cratestack_annotations_version_requirement: requirement(CRATESTACK_ANNOTATIONS_FLOOR),
        cratestack_builder_version_requirement: requirement(CRATESTACK_BUILDER_FLOOR),
    }
}
