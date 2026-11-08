//! `cratestack check`: loads the whole schema graph from the root
//! (docs/design/multi-file-schemas.md section 11). Given a part it follows
//! `part of` to the root, checks the whole schema, and says so.

use std::path::PathBuf;

use anyhow::Result;
use cratestack_parser::{FileRole, LoadOptions, canonical_schema_path, load_schema};

use crate::cli_support::{json_check_failure, json_check_success, render_schema_error};
use crate::cli_types::OutputFormat;

fn role_name(role: FileRole) -> &'static str {
    match role {
        FileRole::Root => "root",
        FileRole::Part => "part",
        FileRole::Import => "import",
        FileRole::ImportPart => "import part",
    }
}

pub(super) fn handle_check(schema: PathBuf, format: OutputFormat, list_files: bool) -> Result<()> {
    let options = LoadOptions {
        follow_part_of: true,
        ..Default::default()
    };
    match load_schema(&schema, &options) {
        Ok(loaded) => {
            let root = loaded.root();
            let redirected = root.path != canonical_schema_path(&schema);
            match format {
                OutputFormat::Human => {
                    if redirected {
                        println!(
                            "`{}` is a part of `{}`; checked the whole schema",
                            schema.display(),
                            root.display
                        );
                        println!("schema OK: {}", root.display);
                    } else {
                        println!("schema OK: {}", schema.display());
                    }
                    if list_files {
                        println!("files ({}):", loaded.files.len());
                        for file in &loaded.files {
                            println!("  {:<11} {}", role_name(file.role), file.display);
                        }
                    }
                }
                OutputFormat::Json => {
                    let mut payload = json_check_success(&schema);
                    if redirected {
                        payload["root"] = root.display.clone().into();
                    }
                    if list_files {
                        payload["files"] = loaded
                            .files
                            .iter()
                            .map(|f| serde_json::json!({"path": f.display, "role": role_name(f.role)}))
                            .collect();
                    }
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                }
            }
            Ok(())
        }
        Err(error) => match format {
            OutputFormat::Human => Err(anyhow::anyhow!(render_schema_error(&error))),
            OutputFormat::Json => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_check_failure(&schema, &error))?
                );
                std::process::exit(1);
            }
        },
    }
}
