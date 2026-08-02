//! The `swr` preset (issue #304, epic #298): one `src/models/<model>.ts`
//! file per model (types + plain framework-free async functions), a
//! shared-types file for types 2+ models reference, and a
//! `src/procedures.ts` for procedures in the same shape. This module is
//! a sibling pipeline to `crate::generator`'s default path, not a
//! modification of it — see `generator.rs`'s doc comment for why keeping
//! them separate is what makes the default preset's byte-identical
//! guarantee easy to trust.

mod context;
mod context_imports;
mod ownership;
mod ownership_graph;
mod templates;
mod views;

use cratestack_core::{Schema, TransportStyle};

use crate::config::{GeneratedTypeScriptFile, TypeScriptGeneratorConfig};
use crate::error::TypeScriptGeneratorError;
use crate::templates::{OutputPath, build_environment};

pub(crate) fn generate(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
) -> Result<Vec<GeneratedTypeScriptFile>, TypeScriptGeneratorError> {
    if schema.transport == TransportStyle::Grpc {
        return Err(TypeScriptGeneratorError::SwrPresetUnsupportedForGrpc);
    }

    let specs = templates::swr_template_specs_for(schema.transport);
    let environment = build_environment(config.template_dir.as_deref(), &specs)?;
    let ownership = ownership::compute_type_ownership(schema);
    let shared_context = context::build_shared_context(schema, config, &ownership);
    let model_contexts = context::build_model_file_contexts(schema, config, &ownership);

    let mut files = Vec::new();
    for spec in &specs {
        let template = environment
            .get_template(spec.template_name)
            .map_err(|error| TypeScriptGeneratorError::TemplateRender(spec.template_name, error))?;
        match spec.output_path {
            OutputPath::Fixed(path) => {
                let contents = template.render(&shared_context).map_err(|error| {
                    TypeScriptGeneratorError::TemplateRender(spec.template_name, error)
                })?;
                files.push(GeneratedTypeScriptFile {
                    file_name: path.to_owned(),
                    contents,
                });
            }
            OutputPath::PerModel => {
                for model_context in &model_contexts {
                    let contents = template.render(model_context).map_err(|error| {
                        TypeScriptGeneratorError::TemplateRender(spec.template_name, error)
                    })?;
                    files.push(GeneratedTypeScriptFile {
                        file_name: format!("src/models/{}.ts", model_context.file_stem),
                        contents,
                    });
                }
            }
        }
    }

    Ok(files)
}
