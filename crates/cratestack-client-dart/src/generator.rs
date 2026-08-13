use cratestack_core::Schema;

use crate::config::{
    DartGeneratorConfig, DartGeneratorError, DartPreset, GeneratedDartFile, GeneratedDartPackage,
};
use crate::context::build_template_context;
use crate::templates::{build_environment, template_specs_for};

/// Entry point for both presets. `DartPreset::Default` renders the
/// original fixed template set below — untouched by issue #301, so its
/// output stays byte-identical (enforced by `tests/snapshot.rs`).
/// `DartPreset::Riverpod` renders that same set for the files it still
/// reuses verbatim (pubspec, README, runtime, constants, example/test)
/// and replaces the rest (models/apis/queries/library entrypoint) with
/// the fan-out, per-model layout — see `crate::riverpod`.
pub fn generate_package(
    schema: &Schema,
    config: &DartGeneratorConfig,
) -> Result<GeneratedDartPackage, DartGeneratorError> {
    match config.preset {
        DartPreset::Default => generate_default_package(schema, config),
        DartPreset::Riverpod => crate::riverpod::generate_package(schema, config),
    }
}

pub(crate) fn generate_default_package(
    schema: &Schema,
    config: &DartGeneratorConfig,
) -> Result<GeneratedDartPackage, DartGeneratorError> {
    let specs = template_specs_for(schema.transport);
    let environment = build_environment(config.template_dir.as_deref(), &specs)?;
    let context = build_template_context(schema, config)?;
    let files = specs
        .iter()
        .map(|spec| {
            let template = environment
                .get_template(spec.template_name)
                .map_err(|error| DartGeneratorError::TemplateRender(spec.template_name, error))?;
            let contents = template
                .render(&context)
                .map_err(|error| DartGeneratorError::TemplateRender(spec.template_name, error))?;
            Ok(GeneratedDartFile {
                file_name: spec
                    .output_path
                    .replace("{{ package_name }}", &context.package_name),
                contents,
            })
        })
        .collect::<Result<Vec<_>, DartGeneratorError>>()?;

    Ok(GeneratedDartPackage { files })
}
