use std::path::PathBuf;

/// Whether a schema with no `--native-cbor`/`native_cbor: ...` given at all
/// uses `package:cbor` (pure Dart) or `cratestack_cbor` (native, via
/// flutter_rust_bridge/wasm — cratestack#563). The single place to flip
/// this decision later — see [`DartGeneratorConfig::native_cbor`]'s doc
/// comment for why it stays `false` today: `cratestack_cbor` only supports
/// Linux x86_64, Android and web (see
/// `dart-packages/cratestack_cbor/lib/src/native/native_cbor_codec.dart`'s
/// `Abi.current()` checks), so defaulting to it would crash every
/// generated Flutter client on iOS — the most common Flutter target —
/// along with macOS, Windows and Linux arm64.
pub const DEFAULT_NATIVE_CBOR: bool = false;

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
    /// Issue #563's opt-in seam: use the published `cratestack_cbor`
    /// package (flutter_rust_bridge natively, wasm-bindgen on web) instead
    /// of pure-Dart `package:cbor` for the generated runtime's CBOR
    /// codec — see `templates/rest-runtime.dart.j2` and
    /// `templates/rpc_runtime/{types,dio_cbor,dio_json}.dart.j2`.
    ///
    /// Deliberately opt-in, not the default (`DEFAULT_NATIVE_CBOR` is
    /// `false`) — this reverses this issue's original in-thread maintainer
    /// note ("the generator will use the package. Not opt-in"), on a
    /// concrete, verified platform-support gap that note didn't account
    /// for: `cratestack_cbor` only ships prebuilt binaries for Linux
    /// x86_64, Android and web today (`createCborCodec()` throws
    /// `UnsupportedError` everywhere else — see
    /// `dart-packages/cratestack_cbor/lib/src/native/native_cbor_codec.dart`).
    /// Defaulting to it would make every generated Flutter client crash at
    /// runtime on iOS — the most common Flutter target — plus macOS,
    /// Windows and Linux arm64. `package:cbor` is pure Dart and works
    /// everywhere, so it stays the default until the platform matrix is
    /// complete enough to reconsider (a maintainer decision, not something
    /// to flip unilaterally here — see `DEFAULT_NATIVE_CBOR`'s doc).
    ///
    /// Purely additive with respect to every other emitted file: `false`
    /// leaves output byte-identical to before this flag existed (pinned by
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
