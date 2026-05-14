use std::path::PathBuf;

use anyhow::Result;

use crate::cli_support::{
    ensure_output_dir_is_empty, into_generated_files, json_check_failure, json_check_success,
    parse_schema_or_render, render_schema_error, resolve_context_keys, validate_mount_path,
    validate_service_url, validate_studio_context_inputs, validate_studio_name,
    write_generated_files,
};
use crate::cli_types::{Cli, Command, MigrateAction, OutputFormat, StudioProfileArg};

pub(crate) fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Check { schema, format } => handle_check(schema, format)?,
        Command::GenerateDart {
            schema,
            out,
            library_name,
            base_path,
            template_dir,
        } => handle_generate_dart(schema, out, library_name, base_path, template_dir)?,
        Command::GenerateTypeScript {
            schema,
            out,
            package_name,
            base_path,
            template_dir,
        } => handle_generate_typescript(schema, out, package_name, base_path, template_dir)?,
        Command::GenerateStudio {
            schema,
            out,
            name,
            service_url,
            context,
            mount_path,
            profile,
            template_dir,
        } => handle_generate_studio(
            schema,
            out,
            name,
            service_url,
            context,
            mount_path,
            profile,
            template_dir,
        )?,
        Command::PrintIr { schema } => handle_print_ir(schema)?,
        Command::Migrate { action } => match action {
            MigrateAction::Diff {
                schema,
                out_dir,
                backend,
                name,
                allow_destructive,
            } => crate::migrate::handle_diff(schema, out_dir, backend, name, allow_destructive)?,
        },
    }

    Ok(())
}

fn handle_check(schema: PathBuf, format: OutputFormat) -> Result<()> {
    match cratestack_parser::parse_schema_file(&schema) {
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
    }

    Ok(())
}

fn handle_generate_dart(
    schema: PathBuf,
    out: PathBuf,
    library_name: String,
    base_path: String,
    template_dir: Option<PathBuf>,
) -> Result<()> {
    let parsed = parse_schema_or_render(&schema)?;
    let package = cratestack_client_dart::generate_package(
        &parsed,
        &cratestack_client_dart::DartGeneratorConfig {
            library_name,
            base_path,
            template_dir,
        },
    )?;

    write_generated_files(&out, into_generated_files(package.files))?;
    println!("generated Dart client package: {}", out.display());
    Ok(())
}

fn handle_generate_typescript(
    schema: PathBuf,
    out: PathBuf,
    package_name: String,
    base_path: String,
    template_dir: Option<PathBuf>,
) -> Result<()> {
    let parsed = parse_schema_or_render(&schema)?;
    let package = cratestack_client_typescript::generate_package(
        &parsed,
        &cratestack_client_typescript::TypeScriptGeneratorConfig {
            package_name,
            base_path,
            template_dir,
        },
    )?;

    write_generated_files(&out, into_generated_files(package.files))?;
    println!("generated TypeScript client package: {}", out.display());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_generate_studio(
    schema: Vec<PathBuf>,
    out: PathBuf,
    name: String,
    service_url: Vec<String>,
    context: Vec<String>,
    mount_path: String,
    profile: StudioProfileArg,
    template_dir: Option<PathBuf>,
) -> Result<()> {
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
    let generation_contexts =
        build_studio_contexts(&parsed_schemas, &schema, &service_url, &context_keys, &name);
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

    write_generated_files(&out, into_generated_files(package.files))?;
    println!("generated Studio app: {}", out.display());
    Ok(())
}

fn handle_print_ir(schema: PathBuf) -> Result<()> {
    let parsed = parse_schema_or_render(&schema)?;
    println!("{parsed:#?}");
    Ok(())
}

fn build_studio_contexts<'a>(
    parsed_schemas: &'a [cratestack_core::Schema],
    schema_paths: &[PathBuf],
    service_urls: &[String],
    context_keys: &[String],
    studio_name: &str,
) -> Vec<cratestack_studio_generator::StudioGeneratorContext<'a>> {
    parsed_schemas
        .iter()
        .zip(schema_paths.iter())
        .zip(service_urls.iter())
        .zip(context_keys.iter())
        .map(|(((parsed, schema_path), upstream_url), context_key)| {
            let service_name = crate::cli_support::derive_service_name(schema_path, studio_name);
            cratestack_studio_generator::StudioGeneratorContext {
                key: context_key.clone(),
                display_name: service_name.clone(),
                service_name,
                schema_path: schema_path.clone(),
                service_url: upstream_url.clone(),
                schema: parsed,
            }
        })
        .collect()
}
