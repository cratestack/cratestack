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
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum MigrateAction {
    /// Generate a migration from `.cstack` vs the committed snapshot.
    Diff {
        #[arg(long)]
        schema: PathBuf,
        /// Root directory for per-backend migration trees. Defaults
        /// to `migrations/`. Migrations land under
        /// `<out_dir>/<backend>/<timestamp>_<name>/`.
        #[arg(long, default_value = "migrations")]
        out_dir: PathBuf,
        /// Which backend(s) to generate for.
        #[arg(long, value_enum, default_value_t = MigrateBackendArg::Both)]
        backend: MigrateBackendArg,
        /// Human-readable slug appended to the migration directory
        /// name (e.g. `add_customer_email`). Defaults to `migration`.
        #[arg(long, default_value = "migration")]
        name: String,
        /// Allow the migration to contain lossy ops (DropColumn,
        /// DropTable, narrowing type changes). Without this flag,
        /// the command refuses to write a destructive migration.
        #[arg(long)]
        allow_destructive: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum MigrateBackendArg {
    Postgres,
    Sqlite,
    Both,
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
