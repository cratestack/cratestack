use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};

pub(crate) fn render_schema_error(
    schema: &PathBuf,
    error: &cratestack_parser::SchemaError,
) -> String {
    error.render(
        &schema.display().to_string(),
        &std::fs::read_to_string(schema).unwrap_or_default(),
    )
}

pub(crate) fn json_check_success(schema: &PathBuf) -> serde_json::Value {
    serde_json::json!({
        "ok": true,
        "schema": schema.display().to_string(),
        "diagnostics": [],
    })
}

pub(crate) fn json_check_failure(
    schema: &PathBuf,
    error: &cratestack_parser::SchemaError,
) -> serde_json::Value {
    let span = error.span();
    serde_json::json!({
        "ok": false,
        "schema": schema.display().to_string(),
        "diagnostics": [
            {
                "message": error.message(),
                "line": error.line(),
                "start": span.start,
                "end": span.end,
            }
        ],
    })
}

pub(crate) fn parse_schema_or_render(schema: &PathBuf) -> Result<cratestack_core::Schema> {
    cratestack_parser::parse_schema_file(schema)
        .map_err(|error| anyhow!(render_schema_error(schema, &error)))
}

pub(crate) fn validate_mount_path(mount_path: &str) -> Result<()> {
    if !mount_path.starts_with('/') {
        bail!("mount path '{mount_path}' must begin with '/'");
    }
    if mount_path.trim() == "/" {
        bail!("mount path '/' is not supported; use a non-root path such as '/studio'");
    }
    Ok(())
}

pub(crate) fn validate_service_url(service_url: &str) -> Result<()> {
    let parsed = url::Url::parse(service_url)
        .map_err(|error| anyhow!("service url '{service_url}' must be absolute: {error}"))?;
    if !parsed.has_host() {
        bail!("service url '{service_url}' must be absolute");
    }
    Ok(())
}

pub(crate) fn validate_studio_context_inputs(
    schema: &[PathBuf],
    service_url: &[String],
    context: &[String],
) -> Result<()> {
    if schema.is_empty() {
        bail!("at least one --schema must be provided");
    }
    if schema.len() != service_url.len() {
        bail!("generate-studio requires the same number of --schema and --service-url values");
    }
    if !context.is_empty() && context.len() != schema.len() {
        bail!("generate-studio requires either zero --context values or one per --schema");
    }
    Ok(())
}

pub(crate) fn validate_context_key(key: &str) -> Result<()> {
    if key.is_empty() {
        bail!("studio context key must not be empty");
    }
    if key.chars().any(|character| {
        !(character.is_ascii_alphanumeric() || character == '-' || character == '_')
    }) {
        bail!("studio context key '{key}' is not URL-safe");
    }
    Ok(())
}

pub(crate) fn validate_studio_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("studio name must not be empty");
    }
    if name.chars().any(|character| {
        !(character.is_ascii_alphanumeric() || character == '-' || character == '_')
    }) {
        bail!("studio name '{name}' is not cargo-safe or filesystem-safe");
    }
    Ok(())
}

pub(crate) fn ensure_output_dir_is_empty(out: &PathBuf) -> Result<()> {
    if !out.exists() {
        return Ok(());
    }
    let mut entries = std::fs::read_dir(out)?;
    if entries.next().is_some() {
        bail!(
            "output directory '{}' already exists and is not empty",
            out.display()
        );
    }
    Ok(())
}

pub(crate) fn derive_service_name(schema: &PathBuf, name: &str) -> String {
    schema
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.file_name())
        .and_then(|value| value.to_str())
        .filter(|value| value.ends_with("-service") || value.ends_with("-gateway"))
        .map(str::to_owned)
        .or_else(|| {
            schema
                .file_stem()
                .and_then(|value| value.to_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| name.to_owned())
}

pub(crate) fn resolve_context_keys(
    schema: &[PathBuf],
    explicit_contexts: &[String],
) -> Result<Vec<String>> {
    let context_keys = if explicit_contexts.is_empty() {
        schema
            .iter()
            .enumerate()
            .map(|(index, schema_path)| derive_context_key(schema_path, index + 1))
            .collect::<Vec<_>>()
    } else {
        explicit_contexts.to_vec()
    };

    let mut seen = BTreeSet::new();
    for key in &context_keys {
        validate_context_key(key)?;
        if !seen.insert(key.clone()) {
            bail!("studio context key '{key}' is duplicated");
        }
    }

    Ok(context_keys)
}

fn derive_context_key(schema: &PathBuf, ordinal: usize) -> String {
    let value = schema
        .file_stem()
        .and_then(|value| value.to_str())
        .map(slugify_path_token)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("context-{ordinal}"));

    if value
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
    {
        format!("context-{value}")
    } else {
        value
    }
}

pub(crate) fn slugify_path_token(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else if character == '_' {
                '_'
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedFile {
    pub(crate) file_name: String,
    pub(crate) contents: String,
}

pub(crate) trait GeneratedFileLike {
    fn into_generated_file(self) -> GeneratedFile;
}

impl GeneratedFileLike for cratestack_client_dart::GeneratedDartFile {
    fn into_generated_file(self) -> GeneratedFile {
        GeneratedFile {
            file_name: self.file_name,
            contents: self.contents,
        }
    }
}

impl GeneratedFileLike for cratestack_client_typescript::GeneratedTypeScriptFile {
    fn into_generated_file(self) -> GeneratedFile {
        GeneratedFile {
            file_name: self.file_name,
            contents: self.contents,
        }
    }
}

impl GeneratedFileLike for cratestack_studio_generator::GeneratedStudioFile {
    fn into_generated_file(self) -> GeneratedFile {
        GeneratedFile {
            file_name: self.file_name,
            contents: self.contents,
        }
    }
}

pub(crate) fn into_generated_files<T: GeneratedFileLike>(files: Vec<T>) -> Vec<GeneratedFile> {
    files
        .into_iter()
        .map(GeneratedFileLike::into_generated_file)
        .collect()
}

pub(crate) fn write_generated_files(out: &PathBuf, files: Vec<GeneratedFile>) -> Result<()> {
    std::fs::create_dir_all(out)?;
    for file in files {
        let destination = out.join(file.file_name);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(destination, file.contents)?;
    }
    Ok(())
}
