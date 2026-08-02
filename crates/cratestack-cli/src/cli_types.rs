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
        /// Drift-detection mode: generate in memory and diff against
        /// `--out` instead of writing. Exits non-zero and lists the
        /// files that differ if the two don't match.
        #[arg(long)]
        check: bool,
        /// `default` (today's monolithic `lib/src/models.dart`/
        /// `lib/src/apis.dart`, byte-identical to pre-#301 output) or
        /// `riverpod` (one file per model under `lib/src/models/`, plus
        /// a shared file for cross-model types, procedures in their own
        /// file, and the package-wide DI providers in `lib/src/client.dart`).
        #[arg(long, value_enum, default_value_t = DartPresetArg::Default)]
        preset: DartPresetArg,
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
        /// Drift-detection mode: generate in memory and diff against
        /// `--out` instead of writing. Exits non-zero and lists the
        /// files that differ if the two don't match.
        #[arg(long)]
        check: bool,
        /// Emit model interfaces with every scalar field required, matching
        /// the schema's own nullability instead of forcing every field
        /// optional for partial `fields`/`include` projection. For
        /// consumers that always fetch full objects.
        #[arg(long)]
        full_selection: bool,
        /// Output layout (issue #304). `default` is today's monolithic
        /// layout (`src/models.ts`, `src/client.ts`, ...) and stays
        /// byte-identical forever. `swr` emits one `src/models/<model>.ts`
        /// per model (types + plain framework-free async functions) and a
        /// `src/procedures.ts` for procedures — the structural foundation
        /// #305 builds SWR hooks on top of. `swr` does not support
        /// `transport grpc` schemas yet.
        #[arg(long, value_enum, default_value_t = TypeScriptPresetArg::Default)]
        preset: TypeScriptPresetArg,
    },
    /// Emit a `.proto` file describing the schema's messages/enums
    /// (no `service` block — that needs `transport grpc`, ticket #170)
    /// plus its sibling field-number lockfile. See
    /// `docs/design/protobuf.md` §4.6 for why `--package` is required on
    /// first run and locked thereafter.
    #[command(name = "generate-proto")]
    GenerateProto {
        #[arg(long)]
        schema: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Protobuf package name. Required on first run (no existing
        /// `<schema>.pb.lock`); on later runs, must match what's already
        /// locked or be omitted.
        #[arg(long)]
        package: Option<String>,
        /// Drift-detection mode: rebuild the lock and `.proto` text in
        /// memory and compare against what's on disk instead of writing.
        #[arg(long)]
        check: bool,
    },
    /// Studio: admin and testing surface for `.cstack` schemas.
    Studio {
        #[command(subcommand)]
        cmd: StudioCmd,
    },
    PrintIr {
        #[arg(long)]
        schema: PathBuf,
    },
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
    /// Diff two `.cstack` schemas and classify each change by its
    /// effect on the generated wire contract (breaking / additive /
    /// internal-only). Exits non-zero if any breaking change is
    /// found, so it can gate CI on schema PRs.
    Diff {
        /// Path to the baseline schema.
        old: PathBuf,
        /// Path to the candidate schema.
        new: PathBuf,
        /// Emit machine-readable JSON instead of the human report.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum StudioCmd {
    /// Write a starter `studio.toml` in the chosen directory.
    Init {
        /// Output directory. The file is always named `studio.toml`.
        #[arg(long, default_value = ".")]
        out: PathBuf,
        /// Overwrite an existing `studio.toml` if present.
        #[arg(long)]
        force: bool,
    },
    /// Boot the studio server against a `studio.toml`.
    Run {
        #[arg(long, default_value = "studio.toml")]
        config: PathBuf,
        /// Override the bind address (default `127.0.0.1:7878`).
        #[arg(long)]
        bind: Option<String>,
    },
    /// Eject a customizable starter project that embeds the studio
    /// against your own `.cstack` schemas. The default emits a
    /// self-contained binary crate (Cargo.toml, src/main.rs,
    /// studio.toml, example schema). Pass `--with-ui` to also drop
    /// the Leptos UI sources for front-end customization.
    Eject {
        #[arg(long)]
        out: PathBuf,
        /// Optional project name written into Cargo.toml / README.
        /// Defaults to the `--out` directory's basename.
        #[arg(long)]
        name: Option<String>,
        /// Overwrite files in `--out` if the directory already exists
        /// and has contents.
        #[arg(long)]
        force: bool,
        /// Also unpack the Leptos+Trunk UI sources into `<out>/ui/`
        /// for front-end customization.
        #[arg(long)]
        with_ui: bool,
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

/// CLI-facing mirror of `cratestack_client_dart::DartPreset` — kept as a
/// separate `ValueEnum` (rather than deriving `ValueEnum` on the library
/// type itself) so the library crate doesn't need a `clap` dependency.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum DartPresetArg {
    Default,
    Riverpod,
}

impl From<DartPresetArg> for cratestack_client_dart::DartPreset {
    fn from(value: DartPresetArg) -> Self {
        match value {
            DartPresetArg::Default => cratestack_client_dart::DartPreset::Default,
            DartPresetArg::Riverpod => cratestack_client_dart::DartPreset::Riverpod,
        }
    }
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

/// CLI-facing mirror of `cratestack_client_typescript::TypeScriptPreset`
/// (issue #304) — `clap::ValueEnum` needs its own type, not the library's,
/// so `cli_handlers::handle_generate_typescript` converts one to the
/// other. Variant names and the default match the Dart CLI's sibling flag
/// (#297) so `generate-typescript --preset swr` and `generate-dart
/// --preset swr` stay consistent for anyone using both.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum TypeScriptPresetArg {
    Default,
    Swr,
}
