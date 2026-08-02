use std::fs;
use std::path::Path;

use minijinja::Environment;

use crate::error::TypeScriptGeneratorError;

mod specs;

// `templates.rs` grew a fan-out mechanism for the `swr` preset (issue
// #304) on top of an already-227-line file — over this repo's ~200-LoC
// convention — so the const spec tables + `template_specs_for` moved to
// `specs.rs`, leaving this file with just the shared types (`OutputPath`,
// `TemplateSpec`) and the environment-registration logic every preset's
// pipeline (default and `swr` alike) renders through.
pub(crate) use specs::template_specs_for;

/// Where a rendered template lands. Issue #304 added `PerModel`: before it,
/// `output_path` was a bare `&'static str` and `generate_package()` mapped
/// one template to exactly one file, unconditionally — physically unable
/// to emit a file per model. `PerModel` is the fan-out point the `swr`
/// preset's per-model template uses; every other spec (the entire
/// default/REST/RPC/GRPC surface in `specs.rs`) stays `Fixed`, so their
/// rendering is byte-for-byte what it was before this enum existed.
#[derive(Debug, Clone, Copy)]
pub(crate) enum OutputPath {
    /// A single output file, known at compile time.
    Fixed(&'static str),
    /// Rendered once per model in the schema; the concrete path is
    /// computed at render time from the model's name (kebab-case, per
    /// this repo's file-naming convention) under `src/models/`.
    PerModel,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TemplateSpec {
    pub(crate) template_name: &'static str,
    pub(crate) output_path: OutputPath,
    pub(crate) default_source: &'static str,
}

/// Registers every spec's template source (disk override if `template_dir`
/// has a matching file, the compiled-in default otherwise) into a fresh
/// minijinja `Environment`. Shared by both the default pipeline
/// (`generator.rs`) and the `swr` pipeline (`crate::swr`) — this function
/// only reads `template_name`/`default_source`, never `output_path`, so it
/// is identical for `Fixed` and `PerModel` specs alike.
pub(crate) fn build_environment(
    template_dir: Option<&Path>,
    specs: &[TemplateSpec],
) -> Result<Environment<'static>, TypeScriptGeneratorError> {
    let mut environment = Environment::new();
    environment.set_trim_blocks(true);
    environment.set_lstrip_blocks(true);

    for spec in specs {
        let source = load_template_source(template_dir, spec)?;
        environment
            .add_template_owned(spec.template_name.to_owned(), source)
            .map_err(|error| {
                TypeScriptGeneratorError::TemplateRegistration(spec.template_name, error)
            })?;
    }

    Ok(environment)
}

fn load_template_source(
    template_dir: Option<&Path>,
    spec: &TemplateSpec,
) -> Result<String, TypeScriptGeneratorError> {
    let Some(template_dir) = template_dir else {
        return Ok(spec.default_source.to_owned());
    };
    let path = template_dir.join(spec.template_name);
    if !path.exists() {
        return Ok(spec.default_source.to_owned());
    }

    fs::read_to_string(&path).map_err(|source| TypeScriptGeneratorError::TemplateRead {
        path: path.display().to_string(),
        template_name: spec.template_name,
        source,
    })
}
