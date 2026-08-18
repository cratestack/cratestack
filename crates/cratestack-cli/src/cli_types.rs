use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "cratestack")]
#[command(about = "CrateStack schema tooling")]
#[command(version)]
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
        /// Opt-in (issue #303): after generation, shell out to `dart run
        /// build_runner build --delete-conflicting-outputs` in `--out` so
        /// a `--preset riverpod` package's `@riverpod` annotations are
        /// actually expanded — the annotated Dart alone does not
        /// compile/analyze until `build_runner` runs. Off by default: a
        /// Rust CLI invoking another toolchain unprompted would be a
        /// surprising behaviour change for existing/scripted callers. No
        /// effect together with `--check` (drift-detection mode never
        /// writes files to run `build_runner` against). Requires a Dart
        /// SDK on `PATH` — see `crate::build_runner::BuildRunnerError`
        /// for the failure modes when it's missing or `build_runner`
        /// itself fails.
        #[arg(long)]
        run_build_runner: bool,
        /// Also emit `pubspec.yaml`/runtime dependencies on the published
        /// `cratestack_cbor` package (flutter_rust_bridge natively,
        /// wasm-bindgen on web — issue #563) instead of pure-Dart
        /// `package:cbor`.
        ///
        /// Opt-in, not the default: `cratestack_cbor` only ships prebuilt
        /// binaries for Linux x86_64, Android and web today —
        /// `createCborCodec()` throws `UnsupportedError` on iOS, macOS,
        /// Windows and Linux arm64 (see
        /// `dart-packages/cratestack_cbor/lib/src/native/native_cbor_codec.dart`).
        /// Defaulting to it would crash every generated Flutter client on
        /// iOS, the most common Flutter target. `package:cbor` is pure
        /// Dart and works everywhere, so it stays the default.
        ///
        /// Purely additive: every other emitted file is byte-identical
        /// with and without it.
        #[arg(long)]
        native_cbor: bool,
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
        /// Additionally emit the file-per-model + SWR-hooks layout under
        /// `src/swr/` (issues #304/#305, made additive by #591 — see
        /// `cratestack_client_typescript::swr`'s module doc for the full
        /// rationale). One `src/swr/models/<model>.ts` per model (types +
        /// plain framework-free async functions) plus a sibling
        /// `<model>.hooks.ts` of `useSWR`/`useSWRMutation` hooks, and a
        /// `src/swr/procedures.ts` (+ `.hooks.ts`) for procedures.
        /// Reachable from a consumer as `<package-name>/swr` (plus
        /// `/swr/models/*`, `/swr/procedures`, `/swr/procedures.hooks`)
        /// via a `package.json` `exports` subpath.
        ///
        /// Purely additive: the default layout at `src/` is always
        /// emitted regardless of this flag; `--swr` adds the `src/swr/`
        /// subtree alongside it rather than replacing it. Does not
        /// support `transport grpc` schemas yet.
        #[arg(long)]
        swr: bool,
        /// Also emit `src/refine.ts` (issue #571): the
        /// `@cratestack/refine` resource manifest for this schema — one
        /// entry per model, carrying the `@id` field name, `@@paged`, and
        /// `@version` facts the refine DataProvider needs and the
        /// generated client encodes only in its TypeScript types.
        ///
        /// Purely additive: every other emitted file is byte-identical
        /// with and without it. REST and RPC schemas only — the manifest
        /// is typed `ResourceMap` for REST and `RpcResourceMap` for RPC;
        /// gRPC-Web has no `@cratestack/refine` provider to bind to.
        /// Composes freely with `--swr`: the manifest binds to the
        /// default layout's client class, which is always emitted
        /// regardless of `--swr`.
        #[arg(long)]
        refine: bool,
        /// Also emit `src/react-query.ts` (issue #617): TanStack Query
        /// (`useQuery`/`useMutation`) hooks over the default layout's
        /// client class, re-exported from `src/index.ts`, plus the
        /// `@tanstack/react-query` peer + dev dependency in
        /// `package.json`. Before this flag existed, all three were
        /// emitted unconditionally, for every schema and transport (REST,
        /// RPC, gRPC-Web alike) — `--tanstack` finishes the convergence
        /// `--swr` (#589) and `--refine` (#571) already went through.
        ///
        /// Purely additive: every other emitted file is byte-identical
        /// with and without it. Unlike `--refine`, this composes with
        /// EVERY transport including gRPC-Web — `--tanstack` gates the
        /// same `src/react-query.ts` that used to be unconditional there
        /// too, it doesn't add support for a transport that lacked it
        /// before. Composes freely with `--swr`/`--refine`.
        #[arg(long)]
        tanstack: bool,
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
    /// Emit WireMock stub mappings (one per procedure, five per model —
    /// `list`/`get`/`create`/`update`/`delete`) derived from the
    /// schema's own `procedure`/`mutation procedure`/`model`
    /// declarations, so integration/e2e tests can run against a mock
    /// backend whose wire contract can't drift from the real one
    /// without regenerating. `transport rest` model CRUD is stateful
    /// (a create is visible on a later list/get, a delete 404s) but
    /// needs more than a plain WireMock — see
    /// `cratestack_mock_wiremock`'s crate docs, its `README.md`, and
    /// `docs/design/wiremock-stubs.md` for what's covered and what
    /// running the stateful stubs costs.
    #[command(name = "generate-wiremock")]
    GenerateWiremock {
        #[arg(long)]
        schema: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Prefix prepended to every stub's `urlPath`, matching the
        /// same-named flag on `generate-dart`/`generate-typescript` —
        /// must agree with whatever prefix the deployed server (and any
        /// generated client being tested against this mock) are
        /// configured with.
        #[arg(long, default_value = "/api")]
        base_path: String,
        /// Drift-detection mode: generate in memory and diff against
        /// `--out` instead of writing. Exits non-zero and lists the
        /// files that differ if the two don't match.
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
    /// Adopt an already-existing database as the starting point for
    /// `migrate diff` (issue #205, design doc
    /// `docs/design/migrate-baseline.md`). Introspects `--database-url`,
    /// prints a drift report against `--schema` grouped by table, and
    /// writes the snapshot from the introspected shape — plus a
    /// synthetic row in `cratestack_migrations` recording the
    /// adoption, so `apply_pending()` doesn't try to recreate what
    /// baseline already accounted for. Refuses to run if a snapshot
    /// already exists at the target path.
    Baseline {
        #[arg(long)]
        schema: PathBuf,
        /// Postgres connection string to introspect. Required (unlike
        /// `migrate diff`, baseline has nothing to do without a live
        /// database) — also the database the synthetic baseline row
        /// is recorded into.
        #[arg(long)]
        database_url: String,
        /// Root directory for per-backend migration trees, matching
        /// `migrate diff`'s default and flag.
        #[arg(long, default_value = "migrations")]
        out_dir: PathBuf,
        /// Baseline is Postgres-only for v1 (design doc §6, open
        /// question 2 — no long-lived "existing production database"
        /// story exists for embedded/SQLite targets today). The flag
        /// exists so the surface matches `migrate diff`'s and a future
        /// backend doesn't need a breaking CLI change; `postgres` is
        /// the only accepted value right now.
        #[arg(long, value_enum, default_value_t = BaselineBackendArg::Postgres)]
        backend: BaselineBackendArg,
        /// Fail (non-zero exit, no writes) if any drift is found
        /// between the live database and `--schema`, instead of the
        /// default report-and-succeed behavior. For teams that want
        /// baselining to double as a "prove the schema already
        /// matches" CI gate.
        #[arg(long)]
        strict: bool,
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

/// `migrate baseline`'s `--backend` value set — deliberately just
/// `Postgres` (not [`MigrateBackendArg`]'s three variants) so
/// "baseline is Postgres-only for v1" is enforced at the type level
/// rather than by rejecting `Sqlite`/`Both` at runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum BaselineBackendArg {
    Postgres,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum OutputFormat {
    Human,
    Json,
}
