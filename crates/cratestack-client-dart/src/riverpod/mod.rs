//! `--preset riverpod` (issue #301): one file per model
//! (`lib/src/models/<model>.dart`) instead of the `default` preset's
//! monolithic `lib/src/models.dart`/`lib/src/apis.dart`. See
//! `crate::config::DartPreset` and `crate::riverpod::partition` for the
//! ownership rule this splits the schema by.
//!
//! Strategy: reuse `crate::generator::generate_default_package` verbatim
//! for every file this preset doesn't touch (README, CHANGELOG,
//! analysis_options, `constants.dart`, `runtime.dart`, `example/main.dart`,
//! `test/*_test.dart` — none of them depend on the model/apis file
//! layout), then replace `models.dart`/`apis.dart`/`queries.dart`/the
//! library entrypoint with the fan-out output built here, plus
//! `pubspec.yaml` (issue #302 needs its own copy — `riverpod_annotation`/
//! `riverpod_generator`/`build_runner` are riverpod-preset-only, and the
//! `default` preset's `pubspec.yaml.j2` is a hard byte-identical
//! contract — see `build_pubspec`'s module doc).
mod build_client;
mod build_library;
mod build_model;
mod build_package_test;
mod build_procedures;
mod build_pubspec;
mod build_queries;
mod build_shared_types;
mod imports;
mod partition;
mod provider_naming;
mod templates;
mod views;

use std::collections::BTreeMap;

use cratestack_core::{EnumDecl, Schema, TransportStyle, TypeDecl};
use minijinja::Environment;
use serde::Serialize;

use crate::config::{
    DartGeneratorConfig, DartGeneratorError, GeneratedDartFile, GeneratedDartPackage,
};
use crate::generator::generate_default_package;
use crate::idents::{to_camel_case, to_pascal_case};
use build_client::build_client_file;
use build_library::build_library_file;
use build_model::build_model_file;
use build_package_test::build_package_test_file;
use build_procedures::build_procedures_file;
use build_pubspec::build_pubspec_file;
use build_queries::build_queries_file;
use build_shared_types::build_shared_types_file;
use partition::partition_types;
use provider_naming::seed_occupied_symbols;

pub(crate) fn generate_package(
    schema: &Schema,
    config: &DartGeneratorConfig,
) -> Result<GeneratedDartPackage, DartGeneratorError> {
    if schema.transport == TransportStyle::Grpc {
        return Err(DartGeneratorError::RiverpodPresetGrpcUnsupported);
    }
    let is_rest = schema.transport == TransportStyle::Rest;

    let base_package = generate_default_package(schema, config)?;
    let library_entrypoint = format!("lib/{}.dart", config.library_name);
    let package_test_path = format!("test/{}_test.dart", config.library_name);
    let replaced = [
        "lib/src/models.dart",
        "lib/src/apis.dart",
        "lib/src/queries.dart",
        "pubspec.yaml",
        library_entrypoint.as_str(),
        package_test_path.as_str(),
    ];
    let mut files: Vec<GeneratedDartFile> = base_package
        .files
        .into_iter()
        .filter(|file| !replaced.contains(&file.file_name.as_str()))
        .collect();

    let partition = partition_types(schema);
    let client_class_name = format!("{}CratestackClient", to_pascal_case(&config.library_name));
    let provider_prefix = to_camel_case(&config.library_name);
    let type_by_name: BTreeMap<&str, &TypeDecl> = schema
        .types
        .iter()
        .map(|ty| (ty.name.as_str(), ty))
        .collect();
    let enum_by_name: BTreeMap<&str, &EnumDecl> = schema
        .enums
        .iter()
        .map(|decl| (decl.name.as_str(), decl))
        .collect();
    // Issue #302: every `@riverpod` provider's Dart identifier is
    // reserved from this single, schema-wide set, in the same order
    // files are generated below (models, then procedures) — see
    // `provider_naming`'s module doc for why collision detection has to
    // be stateful rather than proven correct by construction.
    let mut occupied_provider_symbols = seed_occupied_symbols(schema, &provider_prefix, is_rest);

    let environment = templates::build_environment(config.template_dir.as_deref())?;
    let model_template = if is_rest {
        templates::REST_MODEL
    } else {
        templates::RPC_MODEL
    };

    // Issue #302: captures the first model's `list` provider name (plus
    // `is_paged`) as `build_model_file` actually resolved it, never
    // recomputed in isolation — `build_package_test_file` needs exactly
    // this for its override-propagation proof (see its own doc for why
    // `is_paged` matters).
    let mut first_model_list_provider: Option<(String, String, bool)> = None;

    for model in &schema.models {
        let (rel_path, context) = build_model_file(
            schema,
            model,
            &partition,
            &type_by_name,
            &enum_by_name,
            &provider_prefix,
            &client_class_name,
            is_rest,
            &mut occupied_provider_symbols,
        );
        if first_model_list_provider.is_none() {
            first_model_list_provider = Some((
                context.model_api.model_name.clone(),
                context.operations.list_function_name.clone(),
                context.model_api.is_paged,
            ));
        }
        let contents = render(&environment, model_template, &context)?;
        files.push(GeneratedDartFile {
            file_name: format!("lib/src/models/{rel_path}"),
            contents,
        });
    }

    let shared_types_context = build_shared_types_file(schema, &partition);
    files.push(GeneratedDartFile {
        file_name: "lib/src/models/shared_types.dart".to_owned(),
        contents: render(&environment, templates::SHARED_TYPES, &shared_types_context)?,
    });

    let client_template = if is_rest {
        templates::REST_CLIENT
    } else {
        templates::RPC_CLIENT
    };
    let client_context = build_client_file(schema, config, &provider_prefix, &client_class_name);
    files.push(GeneratedDartFile {
        file_name: "lib/src/client.dart".to_owned(),
        contents: render(&environment, client_template, &client_context)?,
    });

    let procedures_template = if is_rest {
        templates::REST_PROCEDURES
    } else {
        templates::RPC_PROCEDURES
    };
    let procedures_context = build_procedures_file(
        schema,
        &partition,
        &provider_prefix,
        &client_class_name,
        &mut occupied_provider_symbols,
    );
    files.push(GeneratedDartFile {
        file_name: "lib/src/procedures.dart".to_owned(),
        contents: render(&environment, procedures_template, &procedures_context)?,
    });

    if is_rest {
        let queries_context = build_queries_file(schema);
        files.push(GeneratedDartFile {
            file_name: "lib/src/queries.dart".to_owned(),
            contents: render(&environment, templates::QUERIES, &queries_context)?,
        });
    }

    let library_context = build_library_file(schema, is_rest);
    files.push(GeneratedDartFile {
        file_name: library_entrypoint,
        contents: render(&environment, templates::LIBRARY, &library_context)?,
    });

    let pubspec_context = build_pubspec_file(config);
    files.push(GeneratedDartFile {
        file_name: "pubspec.yaml".to_owned(),
        contents: render(&environment, templates::PUBSPEC, &pubspec_context)?,
    });

    let package_test_template = if is_rest {
        templates::REST_PACKAGE_TEST
    } else {
        templates::RPC_PACKAGE_TEST
    };
    let package_test_context = build_package_test_file(
        schema,
        config,
        &provider_prefix,
        first_model_list_provider
            .as_ref()
            .map(|(model_name, list_function_name, is_paged)| {
                (model_name.as_str(), list_function_name.as_str(), *is_paged)
            }),
    )?;
    files.push(GeneratedDartFile {
        file_name: package_test_path,
        contents: render(&environment, package_test_template, &package_test_context)?,
    });

    Ok(GeneratedDartPackage { files })
}

fn render<S: Serialize>(
    environment: &Environment<'static>,
    name: &'static str,
    context: &S,
) -> Result<String, DartGeneratorError> {
    let template = environment
        .get_template(name)
        .map_err(|error| DartGeneratorError::TemplateRender(name, error))?;
    template
        .render(context)
        .map_err(|error| DartGeneratorError::TemplateRender(name, error))
}
