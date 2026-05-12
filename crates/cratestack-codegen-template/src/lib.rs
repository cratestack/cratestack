//! Shared minijinja-template scaffolding.
//!
//! All three codegen crates in the workspace (`cratestack-client-dart`,
//! `cratestack-client-typescript`, `cratestack-studio-generator`) wrapped
//! the same template-runner pattern in three nearly-identical 90-line
//! modules:
//!
//! - a private `TemplateSpec { template_name, output_path, default_source }`
//!   struct;
//! - a `Generated{Lang}File { file_name, contents }` + `Generated{Lang}Package
//!   { files: Vec<…> }` pair;
//! - a `{Lang}GeneratorError` enum with `TemplateRead` / `TemplateRegistration`
//!   / `TemplateRender` variants;
//! - a `build_environment` that registered every spec (with the optional
//!   `template_dir` override) and a `load_template_source` that read the
//!   file or fell back to `default_source`;
//! - a `generate_package` that rendered each spec into the public output
//!   type.
//!
//! This module collapses that scaffolding into one place. Each generator
//! still owns its templates, its `Config` struct, and the template-context
//! builder that turns a `Schema` into renderable view models — those parts
//! are genuinely per-target. The boilerplate around them now lives here.

use std::fs;
use std::path::Path;

use minijinja::Environment;
use serde::Serialize;

/// A single template entry — its name in the `minijinja` registry, the
/// output file path relative to the generated package root, and the
/// fallback source baked into the binary when the caller doesn't supply a
/// `template_dir`.
#[derive(Debug, Clone, Copy)]
pub struct TemplateSpec {
    pub template_name: &'static str,
    pub output_path: &'static str,
    pub default_source: &'static str,
}

/// A rendered template — file path + contents. Generated packages are
/// `Vec<GeneratedFile>` under the hood; each consumer crate re-exposes the
/// pair under a domain-specific alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub file_name: String,
    pub contents: String,
}

/// Collection of rendered templates. Order matches `TemplateSpec` order so
/// downstream writers can iterate deterministically.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GeneratedPackage {
    pub files: Vec<GeneratedFile>,
}

/// Failure mode of the template pipeline. Each variant carries the
/// `template_name` of the offending spec so error messages name exactly
/// what failed.
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("failed to read template '{template_name}' from {path}: {source}")]
    Read {
        path: String,
        template_name: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to register template '{0}': {1}")]
    Registration(&'static str, #[source] minijinja::Error),
    #[error("failed to render template '{0}': {1}")]
    Render(&'static str, #[source] minijinja::Error),
}

/// Build a `minijinja::Environment` pre-populated with every spec.
///
/// `template_dir` is the optional override directory: when present, the
/// runner looks for `template_dir/<template_name>` for each spec and only
/// falls back to `default_source` if that file is absent. The `customize`
/// closure runs after the environment is configured (`trim_blocks`,
/// `lstrip_blocks`) but before templates are registered — generators that
/// need custom filters (e.g. the studio's `tojson`) plug them in there.
pub fn build_environment(
    specs: &[TemplateSpec],
    template_dir: Option<&Path>,
    customize: impl FnOnce(&mut Environment<'static>),
) -> Result<Environment<'static>, TemplateError> {
    let mut environment = Environment::new();
    environment.set_trim_blocks(true);
    environment.set_lstrip_blocks(true);
    customize(&mut environment);

    for spec in specs {
        let source = load_template_source(template_dir, spec)?;
        environment
            .add_template_owned(spec.template_name.to_owned(), source)
            .map_err(|error| TemplateError::Registration(spec.template_name, error))?;
    }

    Ok(environment)
}

/// Render every spec against the supplied context.
///
/// `rewrite_path` lets callers substitute placeholders in the spec's
/// `output_path` (the Dart generator rewrites `{{ package_name }}` to the
/// configured library name, for example). Pass the identity closure when
/// no rewriting is needed.
pub fn render_package<C, F>(
    specs: &[TemplateSpec],
    environment: &Environment<'static>,
    context: &C,
    rewrite_path: F,
) -> Result<GeneratedPackage, TemplateError>
where
    C: Serialize,
    F: Fn(&str) -> String,
{
    let files = specs
        .iter()
        .map(|spec| {
            let template = environment
                .get_template(spec.template_name)
                .map_err(|error| TemplateError::Render(spec.template_name, error))?;
            let contents = template
                .render(context)
                .map_err(|error| TemplateError::Render(spec.template_name, error))?;
            Ok(GeneratedFile {
                file_name: rewrite_path(spec.output_path),
                contents,
            })
        })
        .collect::<Result<Vec<_>, TemplateError>>()?;
    Ok(GeneratedPackage { files })
}

fn load_template_source(
    template_dir: Option<&Path>,
    spec: &TemplateSpec,
) -> Result<String, TemplateError> {
    let Some(template_dir) = template_dir else {
        return Ok(spec.default_source.to_owned());
    };
    let path = template_dir.join(spec.template_name);
    if !path.exists() {
        return Ok(spec.default_source.to_owned());
    }

    fs::read_to_string(&path).map_err(|source| TemplateError::Read {
        path: path.display().to_string(),
        template_name: spec.template_name,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use std::path::PathBuf;

    #[derive(Serialize)]
    struct Ctx {
        name: &'static str,
    }

    #[test]
    fn renders_default_source_when_no_template_dir() {
        const SPECS: &[TemplateSpec] = &[TemplateSpec {
            template_name: "greet.txt.j2",
            output_path: "greet.txt",
            default_source: "hello, {{ name }}",
        }];
        let env = build_environment(SPECS, None, |_| {}).expect("environment");
        let pkg = render_package(SPECS, &env, &Ctx { name: "world" }, |p| p.to_owned())
            .expect("render");
        assert_eq!(pkg.files.len(), 1);
        assert_eq!(pkg.files[0].file_name, "greet.txt");
        assert_eq!(pkg.files[0].contents, "hello, world");
    }

    #[test]
    fn rewrite_path_substitutes_output_filename() {
        const SPECS: &[TemplateSpec] = &[TemplateSpec {
            template_name: "lib.txt.j2",
            output_path: "lib/{{ pkg }}.txt",
            default_source: "",
        }];
        let env = build_environment(SPECS, None, |_| {}).expect("environment");
        let pkg = render_package(SPECS, &env, &Ctx { name: "ignored" }, |path| {
            path.replace("{{ pkg }}", "my_pkg")
        })
        .expect("render");
        assert_eq!(pkg.files[0].file_name, "lib/my_pkg.txt");
    }

    #[test]
    fn missing_override_falls_back_to_default_source() {
        const SPECS: &[TemplateSpec] = &[TemplateSpec {
            template_name: "tpl.j2",
            output_path: "out",
            default_source: "default",
        }];
        // template_dir exists but doesn't contain the spec file — the
        // runner must silently fall back to default_source rather than
        // surfacing a TemplateError::Read for a missing file.
        let env = build_environment(SPECS, Some(&PathBuf::from("/nonexistent")), |_| {})
            .expect("environment");
        let pkg = render_package(SPECS, &env, &Ctx { name: "" }, |p| p.to_owned()).expect("render");
        assert_eq!(pkg.files[0].contents, "default");
    }

    #[test]
    fn customize_can_register_filters() {
        const SPECS: &[TemplateSpec] = &[TemplateSpec {
            template_name: "upper.j2",
            output_path: "out",
            default_source: "{{ name | shout }}",
        }];
        let env = build_environment(SPECS, None, |env| {
            env.add_filter("shout", |value: &str| value.to_uppercase());
        })
        .expect("environment");
        let pkg = render_package(SPECS, &env, &Ctx { name: "hi" }, |p| p.to_owned()).expect("render");
        assert_eq!(pkg.files[0].contents, "HI");
    }
}
