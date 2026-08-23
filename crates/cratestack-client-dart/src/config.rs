use std::path::PathBuf;

/// Whether a schema with no `--no-native-cbor`/`native_cbor: ...` given at
/// all uses `cratestack_cbor` (native, via flutter_rust_bridge/wasm —
/// cratestack#563) or `package:cbor` (pure Dart). The single place to flip
/// this decision later — see [`DartGeneratorConfig::native_cbor`]'s doc
/// comment for the current reasoning.
///
/// Flipped to `true` (maintainer decision, cratestack#563 follow-up):
/// `cratestack_cbor` 0.8.7 is published to pub.dev with Windows, macOS and
/// iOS support verified there, so the platform-support gap and the
/// published-package lag that used to justify defaulting to `false` are
/// both closed. Linux arm64 is the one remaining unsupported target —
/// `createCborCodec()` still throws `UnsupportedError` there, so a
/// generated client built for that target needs `--no-native-cbor` (pure
/// Dart, works everywhere) to avoid a runtime crash.
pub const DEFAULT_NATIVE_CBOR: bool = true;

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
    /// Issue #563's seam: use the published `cratestack_cbor` package
    /// (flutter_rust_bridge natively, wasm-bindgen on web) instead of
    /// pure-Dart `package:cbor` for the generated runtime's CBOR codec —
    /// see `templates/rest-runtime.dart.j2` and
    /// `templates/rpc_runtime/{types,dio_cbor,dio_json}.dart.j2`.
    ///
    /// **The default as of this doc (`DEFAULT_NATIVE_CBOR` is `true`).**
    /// It was opt-in for a while, on a concrete, verified platform-support
    /// gap: `cratestack_cbor` originally shipped prebuilt binaries for
    /// Linux x86_64, Android and web only, so defaulting to it would have
    /// made every generated Flutter client crash at runtime on iOS — the
    /// most common Flutter target — plus macOS and Windows.
    ///
    /// **Both blockers are closed.** cratestack#563's platform-matrix
    /// slices added Windows x64, macOS (universal) and iOS (device +
    /// simulator), each verified by a real running app round-tripping the
    /// shared fixture, and `cratestack_cbor` 0.8.7 — carrying that matrix —
    /// is published on pub.dev, closing the published-package-lags-the-repo
    /// gap that kept the default at `false` even after the platform work
    /// landed. Only Linux arm64 remains unsupported: `createCborCodec()`
    /// still throws `UnsupportedError` there — see
    /// `dart-packages/cratestack_cbor/lib/src/native/native_cbor_codec.dart`.
    /// A client generated for that target needs `native_cbor: false`
    /// (CLI: `--no-native-cbor`) to fall back to pure-Dart `package:cbor`,
    /// which works everywhere.
    ///
    /// Purely additive with respect to every other emitted file: this flag
    /// leaves output byte-identical either way apart from the two files
    /// that legitimately depend on the codec choice (pinned by
    /// `tests/native_cbor_generator.rs`'s dedicated regression test, on
    /// top of the pre-existing `tests/snapshot.rs` pins that never pass
    /// this field at all and so exercise `Default::default()`).
    ///
    /// `cratestack_cbor`'s codec is async (`Future<CratestackCborCodec>
    /// createCborCodec()`, `Uint8List encodeJson(String json)`, `String
    /// decodeJson(List<int> bytes)`) where `package:cbor/simple.dart`'s is
    /// synchronous and operates on a dynamic Dart value directly rather
    /// than JSON text — the runtime templates bridge the two via
    /// `jsonEncode`/`jsonDecode` around a lazily-initialized, cached codec
    /// future, not a synchronous shim. See `wire_encode.rs`/`wire_decode.rs`
    /// for why this is a safe bridge: every generated `toWire()`/`fromWire`
    /// already routes fields through JSON-plain values (ISO-8601 strings
    /// for `DateTime`, `List<int>` for `Bytes`, decimal string text) rather
    /// than any CBOR-specific Dart type, so the body a runtime adapter
    /// encodes/decodes was already JSON-safe before this flag existed.
    pub native_cbor: bool,
}

impl Default for DartGeneratorConfig {
    fn default() -> Self {
        Self {
            library_name: "cratestack_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Default,
            schema_sha256: String::new(),
            native_cbor: DEFAULT_NATIVE_CBOR,
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
}
