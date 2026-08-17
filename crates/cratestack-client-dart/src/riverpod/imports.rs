//! Import-line computation shared by the `riverpod` preset's per-locus
//! builders (`build_model`, `build_shared`). Every generated file only
//! imports what it actually references — an unused import is a `dart
//! analyze --fatal-warnings` failure (see `justfile`'s `verify-dart`).
use std::collections::BTreeSet;

use cratestack_core::route_naming;
use cratestack_core::{Field, Model, TypeDecl};

use crate::riverpod::partition::referenced_name;

/// `lib/src/models/<file_stem>.dart` — the snake_case convention
/// `analysis_options.yaml` (`file_names` lint) requires for Dart source
/// files, matching `cratestack_core::route_naming::to_snake_case`'s
/// canonical convention for routes elsewhere in this generator
/// (cratestack#345).
pub(crate) fn model_file_stem(model_name: &str) -> String {
    route_naming::to_snake_case(model_name)
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

/// Model names referenced anywhere across a set of "owned" nested `type`
/// declarations — the single computation point every per-locus builder
/// (`build_model`, `build_shared_types`, `build_procedures`) calls for its
/// own `partition.owned_names(&locus)` type decls, instead of each
/// hand-rolling the same `direct_model_refs` scan over `type_decl.fields`.
///
/// This exists because issue #626 was exactly that duplication going
/// wrong: `build_model.rs`/`build_shared_types.rs` both called
/// `direct_model_refs` per owned type decl (the fix for issue #137), but
/// `build_procedures.rs` never did, so a procedure-only `type` referencing
/// a `model` silently dropped the model's import in `procedures.dart`. A
/// third builder hand-rolling this scan again would risk the same
/// omission a third time — routing every locus through one function makes
/// that harder, not just less likely.
pub(crate) fn owned_type_decl_model_refs<'a>(
    type_decls: impl IntoIterator<Item = &'a TypeDecl>,
    model_names: &BTreeSet<&str>,
) -> BTreeSet<String> {
    type_decls
        .into_iter()
        .flat_map(|type_decl| direct_model_refs(type_decl.fields.iter(), model_names))
        .collect()
}

/// Renders a sorted, deduplicated set of Dart `import` statements.
pub(crate) fn render_import_lines(lines: BTreeSet<String>) -> Vec<String> {
    lines.into_iter().collect()
}
