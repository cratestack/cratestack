//! The `swr` preset (issues #304/#305, epic #298): one
//! `src/models/<model>.ts` file per model (types + plain framework-free
//! async functions), a shared-types file for types 2+ models reference,
//! and a `src/procedures.ts` for procedures in the same shape — plus,
//! per model/procedures file, a sibling `.hooks.ts` file of `useSWR`/
//! `useSWRMutation` hooks wrapping those functions. This module is a
//! sibling pipeline to `crate::generator`'s default path, not a
//! modification of it — see `generator.rs`'s doc comment for why keeping
//! them separate is what makes the default preset's byte-identical
//! guarantee easy to trust.
//!
//! ## Why hooks are a *sibling* file, not appended to the plain-function
//! ## file (issue #305)
//!
//! The epic's own desired-output sketch shows one file per model holding
//! both; a literal reading of that would append the hooks straight into
//! `src/models/<model>.ts`. That's incompatible with issue #304's own
//! framework-free guarantee, `tests/swr_runtime.rs`: ECMAScript modules
//! resolve and evaluate *every* top-level static import in a file the
//! moment that file is loaded, regardless of which named export the
//! importer actually asked for — `import { getWidget } from
//! "./models/widget"` would still eagerly resolve a same-file `import
//! useSWR from "swr"`, and fail outright with zero `node_modules`
//! present (exactly the scenario that test exercises). So the plain
//! functions and their hooks are two files sharing one model directory
//! entry (`widget.ts` / `widget.hooks.ts`) — "per-model", matching
//! issue #305's "no separate whole-schema hooks dump file" requirement,
//! without smuggling a hook-framework dependency into a file that must
//! stay importable with nothing but the runtime installed.

mod context;
mod context_imports;
mod hook_naming;
mod model_summary;
mod ownership;
mod ownership_graph;
mod templates;
mod views;

use std::collections::HashMap;

use cratestack_core::{Schema, TransportStyle};

use crate::config::{GeneratedTypeScriptFile, TypeScriptGeneratorConfig};
use crate::error::TypeScriptGeneratorError;
use crate::templates::{OutputPath, build_environment};
use views::SwrModelFileContext;

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
    reject_model_file_name_collisions(&model_contexts)?;

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
            OutputPath::PerModel(suffix) => {
                for model_context in &model_contexts {
                    let contents = template.render(model_context).map_err(|error| {
                        TypeScriptGeneratorError::TemplateRender(spec.template_name, error)
                    })?;
                    files.push(GeneratedTypeScriptFile {
                        file_name: format!("src/models/{}{suffix}", model_context.file_stem),
                        contents,
                    });
                }
            }
        }
    }

    Ok(files)
}

/// Issue #344: `PerModel` output paths (`src/models/{{ file_stem }}.ts`
/// and its `.hooks.ts` sibling) are keyed solely by
/// `SwrModelFileContext::file_stem`, which `crate::naming::to_kebab_case`
/// derives from the model's schema name through the same lossy tokenizer
/// every other derived-name helper in this crate shares. Two distinct
/// models that tokenize identically (`UserGroup`/`User_Group`, see
/// `tests/fixtures/swr_key_collision.cstack`) would otherwise silently
/// clobber each other's generated file with no error — this check runs
/// once, before any file is rendered, so a collision is refused up front
/// rather than discovered by diffing generator output.
fn reject_model_file_name_collisions(
    model_contexts: &[SwrModelFileContext],
) -> Result<(), TypeScriptGeneratorError> {
    let mut seen_by_file_stem: HashMap<&str, &str> = HashMap::new();
    for model_context in model_contexts {
        let file_stem = model_context.file_stem.as_str();
        let model_name = model_context.model.name.as_str();
        if let Some(&first) = seen_by_file_stem.get(file_stem) {
            return Err(TypeScriptGeneratorError::SwrModelFileNameCollision {
                first: first.to_owned(),
                second: model_name.to_owned(),
                file_stem: file_stem.to_owned(),
            });
        }
        seen_by_file_stem.insert(file_stem, model_name);
    }
    Ok(())
}
