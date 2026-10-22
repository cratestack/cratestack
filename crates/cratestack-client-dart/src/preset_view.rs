//! Which preset a render is for, as the templates see it.
use serde::Serialize;

use crate::config::{DartGeneratorConfig, DartPreset};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PresetView {
    /// The `riverpod` layout: `Riverpod` or `Fespalier` (issue #303).
    /// `README.md.j2` is one of the files `crate::riverpod::generate_package`
    /// reuses verbatim from `generate_default_package` (see that module's
    /// doc), so this flag gates riverpod-only prose (its
    /// `@riverpod`/`dart_mappable` reasons for needing `build_runner`, on top
    /// of the `@CratestackBuilder` one every preset shares — see
    /// `setup.md.j2`'s "Code Generation" section) without forking the
    /// template.
    pub(crate) is_riverpod_preset: bool,
    /// `DartPreset::Fespalier` (cratestack#1204): the runtime docs and the
    /// README name `CratestackFespalierTransport` instead of
    /// `CratestackDioTransport`, which that preset does not emit. Implies
    /// `is_riverpod_preset`.
    pub(crate) is_fespalier_preset: bool,
    /// `config.fespalier_ref`: the git ref the README names for the
    /// `fespalier_transport` dependency. Meaningful only when
    /// `is_fespalier_preset`.
    pub(crate) fespalier_ref: String,
}

impl PresetView {
    pub(crate) fn new(config: &DartGeneratorConfig) -> Self {
        Self {
            is_riverpod_preset: config.preset.is_riverpod_layout(),
            is_fespalier_preset: config.preset == DartPreset::Fespalier,
            fespalier_ref: config.fespalier_ref.clone(),
        }
    }
}
