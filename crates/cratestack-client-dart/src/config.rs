use std::path::PathBuf;

use cratestack_proto::PbLock;

/// Selects the generated package's file layout. See issue #301 — the
/// `riverpod` preset is a strict superset of `default`'s content,
/// repartitioned into one file per model (types + `XApi` client +
/// relocated `Provider<XApi>`), never a redesign of what's generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DartPreset {
    /// Today's monolithic `lib/src/models.dart`/`lib/src/apis.dart`
    /// layout. Byte-identical output is a hard contract — see
    /// `tests/snapshot.rs`.
    #[default]
    Default,
    /// One file per model (`lib/src/models/<model>.dart`), a shared
    /// file for types referenced by more than one model, procedures in
    /// their own file, and the package-wide DI providers
    /// (`xAdapterProvider`/`xClientProvider`) in a shared `client.dart`.
    Riverpod,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGeneratorConfig {
    pub library_name: String,
    pub base_path: String,
    pub template_dir: Option<PathBuf>,
    pub preset: DartPreset,
    /// `transport grpc` schemas only: the parsed `<schema>.pb.lock` — the
    /// gRPC wire codec needs the *real* field numbers `cratestack-proto`
    /// assigned (ticket #168) to encode/decode protobuf correctly, the
    /// same numbers the Rust server's mirror structs and `.proto` artifact
    /// use. `None` for REST/RPC schemas (unused there) and is a hard error
    /// for a `transport grpc` schema — see `DartGeneratorError::MissingPbLock`.
    pub pb_lock: Option<PbLock>,
    /// Hex-encoded SHA-256 of the schema file's raw bytes (issue #178) —
    /// computed once by the CLI (`cli_support::hash_schema_source`, the
    /// same computation `cratestack-macros` does for `include_*_schema!`)
    /// and baked into the generated client as `Client.schemaSha256`, sent
    /// as `x-cratestack-schema-sha` on every request so a drifted Dart
    /// client shows up as a server-side `tracing::warn!`, not a silent
    /// wire mismatch. Empty string when not supplied (e.g. this crate
    /// used as a library directly, or in tests) — the generated client
    /// simply omits the header in that case. REST and RPC only for this
    /// pass, matching the TypeScript client's scope — the gRPC transport
    /// doesn't send it yet (tracked, not attempted).
    pub schema_sha256: String,
}

impl Default for DartGeneratorConfig {
    fn default() -> Self {
        Self {
            library_name: "cratestack_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Default,
            pb_lock: None,
            schema_sha256: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedDartFile {
    pub file_name: String,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedDartPackage {
    pub files: Vec<GeneratedDartFile>,
}

#[derive(Debug, thiserror::Error)]
pub enum DartGeneratorError {
    #[error("failed to read template '{template_name}' from {path}: {source}")]
    TemplateRead {
        path: String,
        template_name: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to register template '{0}': {1}")]
    TemplateRegistration(&'static str, #[source] minijinja::Error),
    #[error("failed to render template '{0}': {1}")]
    TemplateRender(&'static str, #[source] minijinja::Error),
    /// The schema declares a composite primary key (`@@id([...])`) on at
    /// least one model. `include_*_schema!` has rejected these since the
    /// gap was found (see `cratestack_core::composite_id`), but this
    /// generator had no equivalent guard and instead panicked in
    /// `builders_model.rs`'s `primary_key_field(model).expect(...)` — a
    /// panic, not an error, with a message (`validated schemas always
    /// have an id field`) that is simply untrue: the parser accepts such
    /// a schema. Same rejection, same wording, as the macro path.
    #[error("{0}")]
    CompositePrimaryKeyUnsupported(String),
    /// `transport grpc` schema, but `DartGeneratorConfig::pb_lock` was
    /// `None`. The gRPC wire codec needs the real field numbers
    /// `cratestack generate-proto` assigns — run that first (or pass its
    /// output through) before generating a `transport grpc` client.
    #[error(
        "schema declares `transport grpc`, which needs the schema's `.pb.lock` to generate a \
         gRPC client — run `cratestack generate-proto` first and pass its lock via \
         `DartGeneratorConfig::pb_lock`"
    )]
    MissingPbLock,
    /// The lock parsed, but has no `package` — shouldn't happen for a lock
    /// `generate-proto` produced (`--package` is required on first run,
    /// `docs/design/protobuf.md` §4.6), but a hand-edited or pre-package
    /// lock is possible, so this is a real error rather than a panic.
    #[error(
        "the schema's `.pb.lock` has no `package` set — gRPC method paths need it \
         (`/<package>.Api/<Method>`); re-run `cratestack generate-proto --package <name>`"
    )]
    MissingPbLockPackage,
    /// The lock is missing an entry (message or field) the schema expects
    /// — a stale lock relative to the schema. `cratestack generate-proto
    /// --check` is the tool that catches and reports this drift in detail;
    /// this generator just refuses to guess a field number. `field` is
    /// pre-formatted by the call site (` field \`x\`` or empty) rather than
    /// interpolated here, to keep the `#[error(...)]` format string a
    /// plain literal.
    #[error(
        "`.pb.lock` is missing an entry for message `{message}`{field}: re-run `cratestack generate-proto` to refresh it"
    )]
    MissingPbLockEntry { message: String, field: String },
    /// Epic #297's `riverpod` preset targets REST and RPC only for its
    /// first pass (see the epic's Scope/Out section) — `transport grpc`
    /// schemas keep using `DartPreset::Default`.
    #[error(
        "the `riverpod` preset does not support `transport grpc` schemas yet — use `DartPreset::Default` \
         for this schema, or drop `transport grpc`"
    )]
    RiverpodPresetGrpcUnsupported,
}
