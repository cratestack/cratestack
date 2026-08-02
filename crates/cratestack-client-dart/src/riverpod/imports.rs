//! Import-line computation shared by the `riverpod` preset's per-locus
//! builders (`build_model`, `build_shared`). Every generated file only
//! imports what it actually references — an unused import is a `dart
//! analyze --fatal-warnings` failure (see `justfile`'s `verify-dart`).
use std::collections::BTreeSet;

use cratestack_core::{Field, Model};

use crate::idents::to_snake_case;
use crate::riverpod::partition::referenced_name;

/// `lib/src/models/<file_stem>.dart` — the snake_case convention
/// `analysis_options.yaml` (`file_names` lint) requires for Dart source
/// files, matching `crate::idents::to_snake_case`'s existing convention
/// for routes/identifiers elsewhere in this generator.
pub(crate) fn model_file_stem(model_name: &str) -> String {
    to_snake_case(model_name)
}

pub(crate) fn model_file_path(model_name: &str) -> String {
    format!("{}.dart", model_file_stem(model_name))
}

/// Other models a model's own (non-input) fields relate to — the set of
/// `import '<other>.dart';` lines its own `lib/src/models/<model>.dart`
/// needs so the model class's relation-field types resolve.
pub(crate) fn model_relation_targets(
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> BTreeSet<String> {
    model
        .fields
        .iter()
        .filter(|field| model_names.contains(field.ty.name.as_str()))
        .filter(|field| field.ty.name != model.name)
        .map(|field| field.ty.name.clone())
        .collect()
}

/// Model names directly referenced by a set of fields (e.g. a shared or
/// procedure-owned nested `type`'s own fields) — the rare
/// `type_references_model.cstack`-style case (issue #137) where a plain
/// `type` block names a `model` directly, requiring an import of that
/// model's file from a non-model locus.
pub(crate) fn direct_model_refs<'a>(
    fields: impl Iterator<Item = &'a Field>,
    model_names: &BTreeSet<&str>,
) -> BTreeSet<String> {
    fields
        .map(|field| referenced_name(&field.ty))
        .filter(|name| model_names.contains(name.as_str()))
        .collect()
}

/// Renders a sorted, deduplicated set of Dart `import` statements.
pub(crate) fn render_import_lines(lines: BTreeSet<String>) -> Vec<String> {
    lines.into_iter().collect()
}
