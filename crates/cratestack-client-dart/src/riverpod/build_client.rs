//! Builds `lib/src/client.dart` — the package-wide DI surface
//! (`{{ client_class_name }}`, `xAdapterProvider`, `xClientProvider`,
//! REST's `xBasePathProvider`) relocated verbatim from today's
//! `rest-apis.dart.j2`/`rpc-apis.dart.j2`. Deliberately package-wide, not
//! per-model — every per-model `Provider<XApi>` watches
//! `xClientProvider` from here (see `crate::riverpod::build_model`).
use std::collections::BTreeSet;

use cratestack_core::Schema;

use crate::builders_model::build_model_accessor;
use crate::config::DartGeneratorConfig;
use crate::idents::escape_dart_string;
use crate::riverpod::imports::{model_file_path, render_import_lines};
use crate::riverpod::views::ClientFileContext;

pub(crate) fn build_client_file(
    schema: &Schema,
    config: &DartGeneratorConfig,
    provider_prefix: &str,
    client_class_name: &str,
) -> ClientFileContext {
    let model_accessors = schema
        .models
        .iter()
        .map(|model| build_model_accessor(model, provider_prefix))
        .collect::<Vec<_>>();

    let mut imports: BTreeSet<String> = BTreeSet::new();
    imports.insert("import 'package:flutter_riverpod/flutter_riverpod.dart';".to_owned());
    imports.insert("import 'runtime.dart';".to_owned());
    imports.insert("import 'procedures.dart';".to_owned());
    for model in &schema.models {
        imports.insert(format!("import 'models/{}';", model_file_path(&model.name)));
    }

    ClientFileContext {
        client_class_name: client_class_name.to_owned(),
        provider_prefix: provider_prefix.to_owned(),
        base_path_literal: escape_dart_string(&config.base_path),
        imports: render_import_lines(imports),
        model_accessors,
        has_procedures: !schema.procedures.is_empty(),
    }
}
