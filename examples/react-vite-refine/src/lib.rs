//! `examples/react-vite-refine` has no server crate — see `schema.cstack`
//! for why (a refine.dev admin app driven end-to-end by CrateStack
//! codegen against a *generated WireMock backend*, not a database). This
//! crate exists only so `schema.cstack` sits inside a real workspace
//! member with an offline `cargo test` (`tests/smoke.rs`) proving the
//! schema parses and the generated TypeScript/WireMock output has the
//! expected shape — no Postgres, no Docker, no network. See
//! `README.md` for the full run path (`just react-vite-refine-fixture`,
//! the WireMock container, `web/`).

/// Relative to this crate's own directory (`CARGO_MANIFEST_DIR`), not the
/// workspace root — matches every other example's `schema.cstack`
/// convention.
pub const SCHEMA_PATH: &str = "schema.cstack";
