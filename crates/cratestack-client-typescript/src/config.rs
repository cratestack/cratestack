use std::path::PathBuf;

use cratestack_proto::PbLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeScriptGeneratorConfig {
    pub package_name: String,
    pub base_path: String,
    pub template_dir: Option<PathBuf>,
    /// Emit model interfaces with every scalar field required (matching the
    /// schema's own nullability) instead of forcing every field optional to
    /// account for partial `fields`/`include` projection. For consumers that
    /// never do partial selection and always fetch full objects.
    pub full_selection: bool,
    /// `transport grpc` schemas only: the parsed `<schema>.pb.lock` — the
    /// gRPC-Web wire client needs the *real* field numbers `cratestack-proto`
    /// assigned (ticket #168) to encode/decode protobuf correctly, the same
    /// numbers the Rust server's mirror structs and `.proto` artifact use.
    /// `None` for REST/RPC schemas (unused there) and is a hard error for a
    /// `transport grpc` schema — see `TypeScriptGeneratorError::MissingPbLock`.
    pub pb_lock: Option<PbLock>,
    /// Hex-encoded SHA-256 of the schema file's raw bytes (issue #178) —
    /// computed once by the CLI (`cli_support::hash_schema_source`, the
    /// same computation `cratestack-macros` does for `include_*_schema!`)
    /// and baked into the generated client as `SCHEMA_SHA256`, sent as
    /// `x-cratestack-schema-sha` on every request so a drifted TypeScript
    /// client shows up as a server-side `tracing::warn!`, not a silent
    /// wire mismatch. Empty string when not supplied (e.g. this crate
    /// used as a library directly, or in tests) — the generated client
    /// simply omits the header in that case. REST and RPC only for this
    /// pass, matching the Rust client's scope — the gRPC-Web transport
    /// doesn't send it yet (tracked, not attempted).
    pub schema_sha256: String,
}

impl Default for TypeScriptGeneratorConfig {
    fn default() -> Self {
        Self {
            package_name: "cratestack-client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            full_selection: false,
            pb_lock: None,
            schema_sha256: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTypeScriptFile {
    pub file_name: String,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTypeScriptPackage {
    pub files: Vec<GeneratedTypeScriptFile>,
}
