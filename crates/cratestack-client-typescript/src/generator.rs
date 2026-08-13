use cratestack_core::{Schema, TransportStyle};

use crate::config::{
    GeneratedTypeScriptFile, GeneratedTypeScriptPackage, TypeScriptGeneratorConfig,
    TypeScriptPreset,
};
use crate::context::build_template_context;
use crate::error::TypeScriptGeneratorError;
use crate::templates::{OutputPath, build_environment, template_specs_for};

pub fn generate_package(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
) -> Result<GeneratedTypeScriptPackage, TypeScriptGeneratorError> {
    // Issue #571: reject the combinations `--refine` cannot produce a
    // type-checking file for, before rendering anything. Both are
    // structural (see each error variant's doc comment), so failing here
    // is strictly better than emitting a `refine.ts` that breaks `tsc`
    // in the consumer's package — the generator's own output would look
    // successful and the failure would surface a build step later.
    if config.refine {
        if config.preset != TypeScriptPreset::Default {
            return Err(TypeScriptGeneratorError::RefineUnsupportedPreset);
        }
        if schema.transport != TransportStyle::Rest {
            return Err(TypeScriptGeneratorError::RefineRequiresRest);
        }
    }
    let files = match config.preset {
        TypeScriptPreset::Default => generate_default_package(schema, config)?,
        TypeScriptPreset::Swr => crate::swr::generate(schema, config)?,
    };
    Ok(GeneratedTypeScriptPackage { files })
}

/// Today's monolithic layout. Deliberately untouched by issue #304 beyond
/// destructuring `OutputPath::Fixed` (every spec here is `Fixed` — see
/// `crate::templates::OutputPath`'s doc comment): same specs, same order,
/// same context, same rendering, so this keeps producing byte-identical
/// output to before the `swr` preset existed — enforced by the unmodified
/// snapshot tests in `tests/snapshot.rs`.
fn generate_default_package(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
) -> Result<Vec<GeneratedTypeScriptFile>, TypeScriptGeneratorError> {
    let specs = template_specs_for(schema.transport, config.refine)?;
    let environment = build_environment(config.template_dir.as_deref(), &specs)?;
    let context = build_template_context(schema, config)?;
    specs
        .iter()
        .map(|spec| {
            let OutputPath::Fixed(output_path) = spec.output_path else {
                unreachable!("default/REST/RPC/GRPC template specs are always OutputPath::Fixed");
            };
            let template = environment
                .get_template(spec.template_name)
                .map_err(|error| {
                    TypeScriptGeneratorError::TemplateRender(spec.template_name, error)
                })?;
            let contents = template.render(&context).map_err(|error| {
                TypeScriptGeneratorError::TemplateRender(spec.template_name, error)
            })?;
            Ok(GeneratedTypeScriptFile {
                file_name: output_path.to_owned(),
                contents,
            })
        })
        .collect::<Result<Vec<_>, TypeScriptGeneratorError>>()
}
