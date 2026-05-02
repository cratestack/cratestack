use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "cratestack")]
#[command(about = "CrateStack schema tooling")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
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
pub(crate) enum OutputFormat {
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum StudioProfileArg {
    Dev,
    Prod,
}
