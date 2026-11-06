use std::path::PathBuf;

/// Whether a schema with no `--tanstack`/`tanstack: ...` given at all emits
/// `src/react-query.ts`. **Reserved for @stephane-segning (issue #617's
/// Risks section)**: hard-break to `false` in the next release, or a
/// deprecation window where this stays `true` (with a warning) for one more
/// release first. Currently `false`, matching the issue's stated Expected
/// Behavior. This is the one place to change to flip
/// [`TypeScriptGeneratorConfig::default`]'s `tanstack` value — see that
/// field's doc comment for why the CLI's own `--tanstack` flag needs a
/// separate, larger change (not just this constant) if the decision goes
/// the other way: a plain presence/absence `bool` flag can represent
/// "off unless passed", but representing "on unless explicitly turned off"
/// needs an additional negation flag, which this constant alone can't
/// provide.
pub const DEFAULT_TANSTACK: bool = false;

/// Whether a schema with no `--rtk`/`rtk: ...` given at all emits
/// `src/rtk-api.ts`. `false` — same posture `DEFAULT_TANSTACK` documents:
/// a brand-new opt-in framework binding defaults off. See
/// [`TypeScriptGeneratorConfig::rtk`]'s doc comment for what the flag
/// emits.
pub const DEFAULT_RTK: bool = false;

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
    /// Issue #571: additionally emit `src/refine.ts`, the
    /// `@cratestack/refine` resource-manifest factory for this schema (see
    /// `crate::refine`'s module doc for what it contains and why it is
    /// generated rather than hand-written).
    ///
    /// Purely additive — `false` (the default) leaves every other emitted
    /// file byte-identical, which is what `tests/snapshot.rs` pins.
    /// Composes freely with `swr: true` — `src/refine.ts` binds to the
    /// default layout's client class, which is always emitted regardless
    /// of `swr`.
    pub refine: bool,
    /// Issue #617: additionally emit `src/react-query.ts` — TanStack Query
    /// (`useQuery`/`useMutation`) hooks over the default layout's client
    /// class — and re-export it from `src/index.ts`, and declare
    /// `@tanstack/react-query` as a peer + dev dependency in
    /// `package.json`. Before this flag existed, all three were emitted
    /// unconditionally for every schema and every transport (REST and RPC
    /// alike); `--tanstack` finishes the convergence `--swr`
    /// (#589) and `--refine` (#571) already went through, where every
    /// framework-specific binding is an additive opt-in and the core typed
    /// client stays framework-free.
    ///
    /// Purely additive with respect to every OTHER emitted file, which stays
    /// byte-identical regardless of this flag's value — that part is not in
    /// question. What IS a reserved maintainer decision (issue #617's Risks
    /// section, not implementation discretion) is which way this defaults
    /// when unset: see [`DEFAULT_TANSTACK`], the single place that decision
    /// lives.
    pub tanstack: bool,
    /// Hex-encoded canonical schema identity (issue #178, cratestack#1065) —
    /// `cratestack_core::schema_digest_hex` of the parsed schema, computed
    /// once by the CLI, the same function `cratestack-macros` calls for
    /// `include_*_schema!`; comments and whitespace do not change it,
    /// and baked into the generated client as `SCHEMA_SHA256`, sent as
    /// `x-cratestack-schema-sha` on every request so a drifted TypeScript
    /// client shows up as a server-side `tracing::warn!`, not a silent
    /// wire mismatch. Empty string when not supplied (e.g. this crate
    /// used as a library directly, or in tests) — the generated client
    /// simply omits the header in that case.
    pub schema_sha256: String,
    /// Issue #906 (epic #893's `#897`): additionally emit `src/rtk-api.ts` —
    /// an RTK Query `createApi` endpoint set, built on the existing
    /// `@cratestack/adapter-rtk` `createRpcBaseQuery` primitive for an
    /// RPC-transport schema — and re-export it from `src/index.ts`. See
    /// `crate::rtk`'s module doc for the full shape, and why REST and RPC
    /// necessarily dispatch differently even though both stay "no second
    /// transport implementation" (RPC calls the adapter's base query
    /// directly; REST's `queryFn` still calls this same generated
    /// package's own REST client methods, since no REST equivalent of
    /// `@cratestack/adapter-rtk` exists to dispatch through).
    ///
    /// Purely additive, mirroring `tanstack`/`refine`/`swr`: every other
    /// emitted file is byte-identical with and without it. Composes freely
    /// with every other flag and every transport.
    pub rtk: bool,
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
            tanstack: DEFAULT_TANSTACK,
            schema_sha256: String::new(),
            rtk: DEFAULT_RTK,
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
