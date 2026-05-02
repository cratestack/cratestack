use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "cratestack")]
#[command(about = "CrateStack schema tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Check {
        #[arg(long)]
        schema: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },
    GenerateDart {
        #[arg(long)]
        schema: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "cratestack_client")]
        library_name: String,
        #[arg(long, default_value = "/api")]
        base_path: String,
        #[arg(long)]
        template_dir: Option<PathBuf>,
    },
    #[command(name = "generate-typescript", alias = "generate-ts")]
    GenerateTypeScript {
        #[arg(long)]
        schema: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "cratestack-client")]
        package_name: String,
        #[arg(long, default_value = "/api")]
        base_path: String,
        #[arg(long)]
        template_dir: Option<PathBuf>,
    },
    GenerateStudio {
        #[arg(long, required = true)]
        schema: Vec<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long, required = true)]
        service_url: Vec<String>,
        #[arg(long)]
        context: Vec<String>,
        #[arg(long, default_value = "/studio")]
        mount_path: String,
        #[arg(long, value_enum, default_value_t = StudioProfileArg::Dev)]
        profile: StudioProfileArg,
        #[arg(long)]
        template_dir: Option<PathBuf>,
    },
    PrintIr {
        #[arg(long)]
        schema: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum StudioProfileArg {
    Dev,
    Prod,
}

fn render_schema_error(schema: &PathBuf, error: &cratestack_parser::SchemaError) -> String {
    error.render(
        &schema.display().to_string(),
        &std::fs::read_to_string(schema).unwrap_or_default(),
    )
}

fn json_check_success(schema: &PathBuf) -> serde_json::Value {
    serde_json::json!({
        "ok": true,
        "schema": schema.display().to_string(),
        "diagnostics": [],
    })
}

fn json_check_failure(
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

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Check { schema, format } => match cratestack_parser::parse_schema_file(&schema) {
            Ok(_) => match format {
                OutputFormat::Human => {
                    println!("schema OK: {}", schema.display());
                }
                OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json_check_success(&schema))?
                    );
                }
            },
            Err(error) => match format {
                OutputFormat::Human => {
                    return Err(anyhow::anyhow!(render_schema_error(&schema, &error)));
                }
                OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json_check_failure(&schema, &error))?
                    );
                    std::process::exit(1);
                }
            },
        },
        Command::GenerateDart {
            schema,
            out,
            library_name,
            base_path,
            template_dir,
        } => {
            let parsed = parse_schema_or_render(&schema)?;

            let package = cratestack_client_dart::generate_package(
                &parsed,
                &cratestack_client_dart::DartGeneratorConfig {
                    library_name,
                    base_path,
                    template_dir,
                },
            )?;

            write_generated_files(
                &out,
                package
                    .files
                    .into_iter()
                    .map(|file| GeneratedFile {
                        file_name: file.file_name,
                        contents: file.contents,
                    })
                    .collect(),
            )?;
            println!("generated Dart client package: {}", out.display());
        }
        Command::GenerateTypeScript {
            schema,
            out,
            package_name,
            base_path,
            template_dir,
        } => {
            let parsed = parse_schema_or_render(&schema)?;

            let package = cratestack_client_typescript::generate_package(
                &parsed,
                &cratestack_client_typescript::TypeScriptGeneratorConfig {
                    package_name,
                    base_path,
                    template_dir,
                },
            )?;

            write_generated_files(
                &out,
                package
                    .files
                    .into_iter()
                    .map(|file| GeneratedFile {
                        file_name: file.file_name,
                        contents: file.contents,
                    })
                    .collect(),
            )?;
            println!("generated TypeScript client package: {}", out.display());
        }
        Command::GenerateStudio {
            schema,
            out,
            name,
            service_url,
            context,
            mount_path,
            profile,
            template_dir,
        } => {
            validate_studio_name(&name)?;
            validate_mount_path(&mount_path)?;
            validate_studio_context_inputs(&schema, &service_url, &context)?;
            for url in &service_url {
                validate_service_url(url)?;
            }
            ensure_output_dir_is_empty(&out)?;
            let parsed_schemas = schema
                .iter()
                .map(parse_schema_or_render)
                .collect::<Result<Vec<_>>>()?;
            let context_keys = resolve_context_keys(&schema, &context)?;
            let generation_contexts = parsed_schemas
                .iter()
                .zip(schema.iter())
                .zip(service_url.iter())
                .zip(context_keys.iter())
                .map(|(((parsed, schema_path), upstream_url), context_key)| {
                    let service_name = derive_service_name(schema_path, &name);
                    cratestack_studio_generator::StudioGeneratorContext {
                        key: context_key.clone(),
                        display_name: service_name.clone(),
                        service_name,
                        schema_path: schema_path.clone(),
                        service_url: upstream_url.clone(),
                        schema: parsed,
                    }
                })
                .collect::<Vec<_>>();

            let package = cratestack_studio_generator::generate_package(
                &generation_contexts,
                &cratestack_studio_generator::StudioGeneratorConfig {
                    name,
                    mount_path,
                    profile: match profile {
                        StudioProfileArg::Dev => cratestack_studio_generator::StudioProfile::Dev,
                        StudioProfileArg::Prod => cratestack_studio_generator::StudioProfile::Prod,
                    },
                    template_dir,
                },
            )?;

            write_generated_files(
                &out,
                package
                    .files
                    .into_iter()
                    .map(|file| GeneratedFile {
                        file_name: file.file_name,
                        contents: file.contents,
                    })
                    .collect(),
            )?;
            println!("generated Studio app: {}", out.display());
        }
        Command::PrintIr { schema } => {
            let parsed = parse_schema_or_render(&schema)?;
            println!("{parsed:#?}");
        }
    }

    Ok(())
}

fn parse_schema_or_render(schema: &PathBuf) -> Result<cratestack_core::Schema> {
    cratestack_parser::parse_schema_file(schema)
        .map_err(|error| anyhow!(render_schema_error(schema, &error)))
}

fn validate_mount_path(mount_path: &str) -> Result<()> {
    if !mount_path.starts_with('/') {
        bail!("mount path '{mount_path}' must begin with '/'");
    }
    if mount_path.trim() == "/" {
        bail!("mount path '/' is not supported; use a non-root path such as '/studio'");
    }
    Ok(())
}

fn validate_service_url(service_url: &str) -> Result<()> {
    let parsed = url::Url::parse(service_url)
        .map_err(|error| anyhow!("service url '{service_url}' must be absolute: {error}"))?;
    if !parsed.has_host() {
        bail!("service url '{service_url}' must be absolute");
    }
    Ok(())
}

fn validate_studio_context_inputs(
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

fn validate_context_key(key: &str) -> Result<()> {
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

fn validate_studio_name(name: &str) -> Result<()> {
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

fn ensure_output_dir_is_empty(out: &PathBuf) -> Result<()> {
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

fn derive_service_name(schema: &PathBuf, name: &str) -> String {
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

fn resolve_context_keys(schema: &[PathBuf], explicit_contexts: &[String]) -> Result<Vec<String>> {
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

fn slugify_path_token(value: &str) -> String {
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
struct GeneratedFile {
    file_name: String,
    contents: String,
}

fn write_generated_files(out: &PathBuf, files: Vec<GeneratedFile>) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use clap::Parser;

    use super::{
        Cli, Command, StudioProfileArg, ensure_output_dir_is_empty, json_check_failure,
        json_check_success, resolve_context_keys, validate_context_key, validate_mount_path,
        validate_service_url, validate_studio_context_inputs, validate_studio_name,
    };

    #[test]
    fn json_success_payload_has_empty_diagnostics() {
        let payload = json_check_success(&Path::new("schema.cstack").to_path_buf());
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["schema"], "schema.cstack");
        assert_eq!(payload["diagnostics"], serde_json::json!([]));
    }

    #[test]
    fn json_failure_payload_exposes_structured_diagnostic_fields() {
        let error = cratestack_parser::parse_schema("model User {\n  email String\n}\n")
            .expect_err("schema should fail validation");
        let payload = json_check_failure(&Path::new("schema.cstack").to_path_buf(), &error);
        let diagnostic = &payload["diagnostics"][0];

        assert_eq!(payload["ok"], false);
        assert_eq!(diagnostic["line"], 1);
        assert!(diagnostic["start"].as_u64().is_some());
        assert!(diagnostic["end"].as_u64().is_some());
        assert!(
            diagnostic["message"]
                .as_str()
                .expect("message should be a string")
                .contains("missing an @id field")
        );
    }

    #[test]
    fn generate_studio_clap_defaults() {
        let cli = Cli::parse_from([
            "cratestack",
            "generate-studio",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
            "--name",
            "vendor-studio",
            "--service-url",
            "http://127.0.0.1:8082",
        ]);

        match cli.command {
            Command::GenerateStudio {
                schema,
                service_url,
                context,
                mount_path,
                profile,
                ..
            } => {
                assert_eq!(schema, vec![PathBuf::from("schema.cstack")]);
                assert_eq!(service_url, vec!["http://127.0.0.1:8082".to_owned()]);
                assert!(context.is_empty());
                assert_eq!(mount_path, "/studio");
                assert_eq!(profile, StudioProfileArg::Dev);
            }
            _ => panic!("expected generate-studio command"),
        }
    }

    #[test]
    fn generate_typescript_clap_defaults() {
        let cli = Cli::parse_from([
            "cratestack",
            "generate-typescript",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
        ]);

        match cli.command {
            Command::GenerateTypeScript {
                schema,
                out,
                package_name,
                base_path,
                template_dir,
            } => {
                assert_eq!(schema, PathBuf::from("schema.cstack"));
                assert_eq!(out, PathBuf::from("out"));
                assert_eq!(package_name, "cratestack-client");
                assert_eq!(base_path, "/api");
                assert_eq!(template_dir, None);
            }
            _ => panic!("expected generate-typescript command"),
        }
    }

    #[test]
    fn validate_mount_path_rejects_missing_leading_slash() {
        let error = validate_mount_path("studio").expect_err("mount path should fail");
        assert!(error.to_string().contains("must begin with '/'"));
    }

    #[test]
    fn validate_mount_path_rejects_root_mount() {
        let error = validate_mount_path("/").expect_err("root mount path should fail");
        assert!(error.to_string().contains("not supported"));
    }

    #[test]
    fn validate_service_url_rejects_relative_url() {
        let error = validate_service_url("vendor-service:8082")
            .expect_err("relative service url should fail");
        assert!(error.to_string().contains("must be absolute"));
    }

    #[test]
    fn validate_studio_context_inputs_rejects_mismatched_lengths() {
        let error = validate_studio_context_inputs(
            &[PathBuf::from("a.cstack"), PathBuf::from("b.cstack")],
            &["http://127.0.0.1:8081".to_owned()],
            &[],
        )
        .expect_err("mismatched inputs should fail");
        assert!(
            error
                .to_string()
                .contains("same number of --schema and --service-url")
        );
    }

    #[test]
    fn validate_context_key_rejects_spaces() {
        let error = validate_context_key("vendor admin")
            .expect_err("spaces should fail context key validation");
        assert!(error.to_string().contains("not URL-safe"));
    }

    #[test]
    fn resolve_context_keys_derives_unique_defaults() {
        let keys = resolve_context_keys(
            &[
                PathBuf::from("services/vendor-service/schema/vendor.cstack"),
                PathBuf::from("services/auth-service/schema/auth.cstack"),
            ],
            &[],
        )
        .expect("context keys should derive");

        assert_eq!(keys, vec!["vendor".to_owned(), "auth".to_owned()]);
    }

    #[test]
    fn validate_studio_name_rejects_invalid_chars() {
        let error = validate_studio_name("vendor studio")
            .expect_err("spaces should fail studio name validation");
        assert!(
            error
                .to_string()
                .contains("not cargo-safe or filesystem-safe")
        );
    }

    #[test]
    fn ensure_output_dir_is_empty_rejects_non_empty_directory() {
        let temp_dir = tempfile::tempdir().expect("temp dir should create");
        let file_path = temp_dir.path().join("existing.txt");
        std::fs::write(&file_path, "hello").expect("temp file should write");

        let error = ensure_output_dir_is_empty(&PathBuf::from(temp_dir.path()))
            .expect_err("non-empty dir should fail");
        assert!(
            error
                .to_string()
                .contains("already exists and is not empty")
        );
    }
}
