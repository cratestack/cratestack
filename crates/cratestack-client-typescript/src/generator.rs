use cratestack_core::Schema;

use crate::config::{
    GeneratedTypeScriptFile, GeneratedTypeScriptPackage, TypeScriptGeneratorConfig,
};
use crate::context::build_template_context;
use crate::templates::{TypeScriptGeneratorError, build_environment, template_specs_for};

pub fn generate_package(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
) -> Result<GeneratedTypeScriptPackage, TypeScriptGeneratorError> {
    let specs = template_specs_for(schema.transport);
    let environment = build_environment(config.template_dir.as_deref(), &specs)?;
    let context = build_template_context(schema, config);
    let files = specs
        .iter()
        .map(|spec| {
            let template = environment
                .get_template(spec.template_name)
                .map_err(|error| {
                    TypeScriptGeneratorError::TemplateRender(spec.template_name, error)
                })?;
            let contents = template.render(&context).map_err(|error| {
                TypeScriptGeneratorError::TemplateRender(spec.template_name, error)
            })?;
            Ok(GeneratedTypeScriptFile {
                file_name: spec.output_path.to_owned(),
                contents,
            })
        })
        .collect::<Result<Vec<_>, TypeScriptGeneratorError>>()?;

    Ok(GeneratedTypeScriptPackage { files })
}
