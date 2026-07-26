use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGeneratorConfig {
    pub library_name: String,
    pub base_path: String,
    pub template_dir: Option<PathBuf>,
    /// Hex-encoded SHA-256 of the schema file's raw bytes (issue #178) —
    /// computed once by the CLI (`cli_support::hash_schema_source`, the
    /// same computation `cratestack-macros` does for `include_*_schema!`)
    /// and baked into the generated client as `Client.schemaSha256`, sent
    /// as `x-cratestack-schema-sha` on every request so a drifted Dart
    /// client shows up as a server-side `tracing::warn!`, not a silent
    /// wire mismatch. Empty string when not supplied (e.g. this crate
    /// used as a library directly, or in tests) — the generated client
    /// simply omits the header in that case.
    pub schema_sha256: String,
}

impl Default for DartGeneratorConfig {
    fn default() -> Self {
        Self {
            library_name: "cratestack_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
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
    /// `transport grpc` schemas parse (ticket #170) and `cratestack-proto`
    /// can emit their `.proto` `service` block, but no Dart client exists
    /// yet — `package:grpc` support is ticket #172
    /// (`docs/design/protobuf.md` §9). Distinct from
    /// `cratestack-macros::include::parse`'s `reject_grpc_transport_without_runtime`:
    /// that guard covers the three `include_*_schema!` proc-macros, this
    /// crate is a separate, non-macro CLI code path
    /// (`cratestack generate-dart`) with its own schema-to-transport match
    /// that needed its own exhaustive arm once `TransportStyle` grew a
    /// third variant.
    #[error(
        "schema declares `transport grpc`, which has no Dart client codegen yet \
         (tracking: https://github.com/cratestack/cratestack/issues/172)"
    )]
    UnsupportedGrpcTransport,
}
