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
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PubspecFileContext {
    pub(crate) package_name: String,
}

pub(crate) fn build_pubspec_file(config: &DartGeneratorConfig) -> PubspecFileContext {
    PubspecFileContext {
        package_name: config.library_name.clone(),
    }
}
