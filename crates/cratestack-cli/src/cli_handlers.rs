use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::cli_support::{
    into_generated_files, json_check_failure, json_check_success, parse_schema_or_render,
    render_schema_error, write_generated_files,
};
use crate::cli_types::{Cli, Command, MigrateAction, OutputFormat, StudioCmd};

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
        Command::Studio { cmd } => handle_studio(cmd)?,
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

fn handle_print_ir(schema: PathBuf) -> Result<()> {
    let parsed = parse_schema_or_render(&schema)?;
    println!("{parsed:#?}");
    Ok(())
}

fn handle_studio(cmd: StudioCmd) -> Result<()> {
    match cmd {
        StudioCmd::Init { out, force } => handle_studio_init(out, force),
        StudioCmd::Run { config, bind } => handle_studio_run(config, bind),
        StudioCmd::Eject { config, out, force } => {
            handle_studio_eject(config, out, force)
        }
    }
}

fn handle_studio_init(out: PathBuf, force: bool) -> Result<()> {
    std::fs::create_dir_all(&out)
        .with_context(|| format!("failed to create output directory '{}'", out.display()))?;
    let target = out.join(cratestack_studio::DEFAULT_CONFIG_FILE);
    if target.exists() && !force {
        bail!(
            "'{}' already exists; pass --force to overwrite",
            target.display()
        );
    }
    std::fs::write(&target, cratestack_studio::STARTER_CONFIG)
        .with_context(|| format!("failed to write '{}'", target.display()))?;
    println!("wrote starter studio config: {}", target.display());
    Ok(())
}

fn handle_studio_run(config: PathBuf, bind: Option<String>) -> Result<()> {
    let bind_addr: SocketAddr = match bind {
        Some(value) => value
            .parse()
            .with_context(|| format!("invalid --bind '{value}'"))?,
        None => cratestack_studio::DEFAULT_BIND
            .parse()
            .expect("default bind is a valid socket addr"),
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start tokio runtime")?;

    runtime.block_on(async {
        cratestack_studio::run(cratestack_studio::ServerOptions {
            config_path: config,
            bind: bind_addr,
        })
        .await
        .map_err(anyhow::Error::from)
    })
}

fn handle_studio_eject(_config: PathBuf, out: PathBuf, force: bool) -> Result<()> {
    let report = cratestack_studio_generator::eject(&cratestack_studio_generator::EjectOptions {
        out: out.clone(),
        force,
    })?;
    println!(
        "ejected studio UI to '{}' ({} files written)",
        report.out.display(),
        report.written.len()
    );
    println!(
        "next steps: `cd {} && trunk serve` (with `cratestack studio run` in another terminal)",
        report.out.display(),
    );
    Ok(())
}
