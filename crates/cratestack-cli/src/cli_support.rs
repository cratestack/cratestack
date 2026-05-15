use std::path::PathBuf;

use anyhow::{Result, anyhow};

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
