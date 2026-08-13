use std::path::PathBuf;

use cratestack_proto::PbLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeScriptGeneratorConfig {
    pub package_name: String,
    pub base_path: String,
    pub template_dir: Option<PathBuf>,
    /// Issue #591 (originally #304/#305 as `--preset swr`, now a
    /// composable flag rather than a mutually-exclusive preset — see
    /// `crate::swr`'s module doc for the full rationale): additionally
    /// emit the file-per-model + SWR-hooks layout under `src/swr/`,
    /// reachable by consumers as `<package_name>/swr` (and
    /// `<package_name>/swr/models/*`, `/swr/procedures`,
    /// `/swr/procedures.hooks`) via a `package.json` `exports` subpath.
    ///
    /// Purely additive — `false` (the default) leaves every other emitted
    /// file byte-identical to before this flag existed, which is what
    /// `tests/snapshot.rs` pins. The default layout at `src/` is always
    /// emitted regardless of this flag; `swr: true` adds the `src/swr/`
    /// subtree alongside it rather than replacing it, so a consumer who
    /// used to run this generator twice (once per preset, into two
    /// directories/packages) gets both layouts from one run into one
    /// package instead.
    pub swr: bool,
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
    /// Issue #571: additionally emit `src/refine.ts`, the
    /// `@cratestack/refine` resource-manifest factory for this schema (see
    /// `crate::refine`'s module doc for what it contains and why it is
    /// generated rather than hand-written).
    ///
    /// Purely additive — `false` (the default) leaves every other emitted
    /// file byte-identical, which is what `tests/snapshot.rs` pins.
    /// REST or RPC schemas only; `generate_package` rejects a `transport
    /// grpc` schema rather than emitting a file that cannot type-check.
    /// Composes freely with `swr: true` — `src/refine.ts` binds to the
    /// default layout's client class, which is always emitted regardless
    /// of `swr`.
    pub refine: bool,
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
            swr: false,
            full_selection: false,
            refine: false,
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
