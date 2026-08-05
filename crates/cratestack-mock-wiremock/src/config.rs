/// Options controlling how `generate_package` renders a schema's
/// procedures into WireMock stub mappings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireMockGeneratorConfig {
    /// Prefix prepended to every generated stub's `urlPath`, matching the
    /// same-named flag on `generate-dart`/`generate-typescript` (default
    /// `/api`) — cratestack itself has no opinion on where a schema's
    /// procedures are mounted (see the `RouteTransportDescriptor`s
    /// `crates/cratestack-macros/src/transport/rest.rs` emits, which are
    /// all prefix-free), so this only needs to agree with whatever
    /// prefix the deployed server and the generated client it's standing
    /// in for were both configured with.
    pub base_path: String,
}

impl Default for WireMockGeneratorConfig {
    fn default() -> Self {
        Self {
            base_path: "/api".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedWireMockFile {
    /// Relative path under `--out`, e.g. `mappings/getMyReferral.json`.
    /// Always under `mappings/` — the directory name a WireMock instance
    /// scans by convention (`WireMockServer`'s default
    /// `--root-dir`/`mappings/` layout), so `--out` can be pointed
    /// directly at a project's existing WireMock root.
    pub file_name: String,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedWireMockPackage {
    pub files: Vec<GeneratedWireMockFile>,
}
