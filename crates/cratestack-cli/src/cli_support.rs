use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

/// Renders a `SchemaError` produced by parsing `schema` earlier in the same
/// call. Takes no `schema` argument (cratestack#916 removed it): the error
/// already carries its own file identity and source text from the moment
/// `parse_schema_file` produced it, so there's no longer a second
/// (potentially stale, or simply wrong) disk read to keep in sync with it.
pub(crate) fn render_schema_error(error: &cratestack_parser::SchemaError) -> String {
    error.render()
}

pub(crate) fn json_check_success(schema: &Path) -> serde_json::Value {
    serde_json::json!({
        "ok": true,
        "schema": schema.display().to_string(),
        "diagnostics": [],
    })
}

pub(crate) fn json_check_failure(
    schema: &Path,
    error: &cratestack_parser::SchemaError,
) -> serde_json::Value {
    let span = error.span();
    serde_json::json!({
        "ok": false,
        "schema": schema.display().to_string(),
        "diagnostics": [
            {
                "message": error.message(),
                // cratestack#916: which file the diagnostic belongs to —
                // always `schema` itself while `check` only ever parses one
                // file, but exposed per-diagnostic (not just the top-level
                // "schema" key) so this shape doesn't have to change again
                // the day `check` can report across several files.
                "file": error.file(),
                "line": error.line(),
                "start": span.start,
                "end": span.end,
            }
        ],
    })
}

pub(crate) fn parse_schema_or_render(schema: &PathBuf) -> Result<cratestack_core::Schema> {
    cratestack_parser::parse_schema_file(schema)
        .map_err(|error| anyhow!(render_schema_error(&error)))
}

#[cfg(test)]
mod tests {
    const WIDGET: &str = "model Widget {\n  id Int @id\n}\n";

    /// The value `cratestack-core`'s own golden test pins, reached here
    /// through the real parser: the CLI's generated clients bake in the
    /// same identity `include_*_schema!` computes (cratestack#1065).
    #[test]
    fn the_cli_hashes_the_canonical_identity_of_the_parsed_schema() {
        let schema = cratestack_parser::parse_schema(WIDGET).expect("schema should parse");
        assert_eq!(
            cratestack_core::schema_digest_hex(&schema),
            "95c11ca292e854994d452dcc0d88c7de6ab309b0422fc1e30ab46e56a7757a5f"
        );
    }

    #[test]
    fn a_comment_does_not_change_the_identity_the_cli_bakes_in() {
        let plain = cratestack_parser::parse_schema(WIDGET).unwrap();
        let commented = cratestack_parser::parse_schema(
            "// note\n/// doc\nmodel   Widget {\n  /// pk\n  id  Int  @id\n}",
        )
        .unwrap();
        assert_eq!(
            cratestack_core::schema_digest_hex(&plain),
            cratestack_core::schema_digest_hex(&commented)
        );
    }
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

impl GeneratedFileLike for cratestack_mock_wiremock::GeneratedWireMockFile {
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
