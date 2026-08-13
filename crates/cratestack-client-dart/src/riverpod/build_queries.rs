//! Builds `lib/src/queries.dart` (REST only) — see `QueriesFileContext`'s
//! doc for why the per-model `Selection`/`IncludeSelection` pair stays
//! here rather than moving into each model's own file.
use crate::builders_model::build_selection_model;
use crate::naming::{enum_name_set, model_name_set};
use crate::riverpod::imports::model_file_path;
use crate::riverpod::views::QueriesFileContext;
use cratestack_core::Schema;

pub(crate) fn build_queries_file(schema: &Schema) -> QueriesFileContext {
    let model_names = model_name_set(&schema.models);
    let enum_names = enum_name_set(&schema.enums);

    let selection_models = schema
        .models
        .iter()
        .map(|model| build_selection_model(model, &schema.models, &model_names, &enum_names))
        .collect::<Vec<_>>();

    let mut imports = vec!["import 'runtime.dart';".to_owned()];
    for model in &schema.models {
        imports.push(format!("import 'models/{}';", model_file_path(&model.name)));
    }

    QueriesFileContext {
        imports,
        selection_models,
    }
}
