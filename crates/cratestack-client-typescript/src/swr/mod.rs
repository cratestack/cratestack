//! The `--swr` flag (issues #304/#305/epic #298, turned from a
//! mutually-exclusive `--preset swr` into an additive flag by issue #591):
//! one `src/swr/models/<model>.ts` file per model (types + plain
//! framework-free async functions), a shared-types file for types 2+
//! models reference, and a `src/swr/procedures.ts` for procedures in the
//! same shape — plus, per model/procedures file, a sibling `.hooks.ts`
//! file of `useSWR`/`useSWRMutation` hooks wrapping those functions. This
//! module is a sibling pipeline to `crate::generator`'s default path, not
//! a modification of it — see `generator.rs`'s doc comment for why
//! keeping them separate is what makes the default layout's
//! byte-identical guarantee easy to trust.
//!
//! ## Why `src/swr/`, not a second package (issue #591)
//!
//! Before #591, `--preset swr` picked this layout *instead of* the
//! default one — a consumer who wanted both had to run the generator
//! twice, into two directories, and depend on two packages. `--swr` nests
//! this entire subtree under `src/swr/` in the *same* package the default
//! layout occupies at `src/` instead: every template below is reused
//! verbatim from when it was a standalone package (its internal relative
//! imports — `./`, `./models/`, `../queries.js`, etc. — are unchanged,
//! since the whole subtree still moves together), only the *top-level*
//! output path each file lands at gained a `src/swr/` prefix. The
//! package's own `package.json`/`tsconfig.json`/`README.md` are no longer
//! duplicated here — the default layout's copies already cover the whole
//! package, `tsconfig.json`'s `"include": ["src/**/*.ts"]` already reaches
//! `src/swr/**` with no change needed, and `package.json.j2` gains a
//! `"./swr"` (+ `"./swr/models/*"`, `"./swr/procedures"`,
//! `"./swr/procedures.hooks"`) `exports` subpath and the `swr`/`react`
//! peer/dev dependencies only when `swr: true`.
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

mod collisions;
mod context;
mod context_imports;
mod hook_naming;
mod model_summary;
mod ownership;
mod ownership_graph;
mod templates;
mod views;

use cratestack_core::Schema;

use crate::config::{GeneratedTypeScriptFile, TypeScriptGeneratorConfig};
use crate::error::TypeScriptGeneratorError;
use crate::templates::{OutputPath, build_environment};
use collisions::{reject_model_file_name_collisions, reject_procedure_name_collisions};

pub(crate) fn generate(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
) -> Result<Vec<GeneratedTypeScriptFile>, TypeScriptGeneratorError> {
    let specs = templates::swr_template_specs_for(schema.transport);
    let environment = build_environment(config.template_dir.as_deref(), &specs)?;
    let ownership = ownership::compute_type_ownership(schema);
    let shared_context = context::build_shared_context(schema, config, &ownership);
    let model_contexts = context::build_model_file_contexts(schema, config, &ownership);
    reject_model_file_name_collisions(&model_contexts)?;
    reject_procedure_name_collisions(
        &shared_context.procedures_file.procedures,
        &model_contexts,
        schema.transport,
    )?;

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
                        file_name: format!("src/swr/models/{}{suffix}", model_context.file_stem),
                        contents,
                    });
                }
            }
        }
    }

    Ok(files)
}
