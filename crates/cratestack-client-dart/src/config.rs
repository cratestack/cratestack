use std::path::PathBuf;

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
    /// The `riverpod` preset, sending through fespalier's `HttpTransport`
    /// instead of Dio (cratestack#1204): no `dio` in the pubspec, no
    /// `dio_transport.dart`, a generated `CratestackFespalierTransport`
    /// over `package:fespalier_transport`, and a `<prefix>TransportProvider`
    /// the app overrides with its transport chain. The `fespalier_transport`
    /// git dependency is pinned at [`DartGeneratorConfig::fespalier_ref`].
    Fespalier,
}

impl DartPreset {
    /// Whether this preset emits the riverpod layout (one file per model,
    /// `@riverpod` providers). `Fespalier` is `Riverpod` plus a transport.
    pub fn is_riverpod_layout(self) -> bool {
        matches!(self, Self::Riverpod | Self::Fespalier)
    }
}

/// The fespalier release `--preset fespalier` pins `fespalier_transport` at
/// unless `--fespalier-ref` says otherwise (cratestack#1204). This is the
/// release whose `fespalier_transport` interface the generated
/// `CratestackFespalierTransport` was written against; bump it together with
/// that file when fespalier changes the interface. fespalier packages are git
/// dependencies by design, so this is a git tag, not a pub.dev version.
pub const DEFAULT_FESPALIER_REF: &str = "v0.21.0";

/// Options for [`crate::generate_package`].
///
/// There is no codec option: every generated package encodes and decodes
/// CBOR through `cratestack_cbor` (the Rust codec, over flutter_rust_bridge
/// natively and wasm-bindgen on web, cratestack#563). That package is a
/// Flutter FFI plugin, so a generated client is Flutter-only, and it ships
/// no Linux arm64 library. The pure-Dart `package:cbor` mode, and the
/// `native_cbor` field that selected it, were removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGeneratorConfig {
    pub library_name: String,
    pub base_path: String,
    pub template_dir: Option<PathBuf>,
    pub preset: DartPreset,
    /// Hex-encoded canonical schema identity (issue #178, cratestack#1065) —
    /// `cratestack_core::schema_digest_hex` of the parsed schema, computed
    /// once by the CLI, the same function `cratestack-macros` calls for
    /// `include_*_schema!`; comments and whitespace do not change it,
    /// and baked into the generated client as `Client.schemaSha256`, sent
    /// as `x-cratestack-schema-sha` on every request so a drifted Dart
    /// client shows up as a server-side `tracing::warn!`, not a silent
    /// wire mismatch. Empty string when not supplied (e.g. this crate
    /// used as a library directly, or in tests) — the generated client
    /// simply omits the header in that case.
    pub schema_sha256: String,
    /// Git ref (tag, branch or commit) of `github.com/fespalier/fespalier`
    /// the `fespalier` preset pins `fespalier_transport` at in the emitted
    /// pubspec (cratestack#1204). Ignored by every other preset. Defaults to
    /// [`DEFAULT_FESPALIER_REF`]; the CLI exposes it as `--fespalier-ref`.
    pub fespalier_ref: String,
}

impl Default for DartGeneratorConfig {
    fn default() -> Self {
        Self {
            library_name: "cratestack_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Default,
            schema_sha256: String::new(),
            fespalier_ref: DEFAULT_FESPALIER_REF.to_owned(),
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
    /// `--fespalier-ref` is empty or holds whitespace or control characters,
    /// none of which a git ref can contain.
    #[error("invalid fespalier ref {0:?}: expected a git tag, branch or commit, e.g. v0.21.0")]
    InvalidFespalierRef(String),
}
