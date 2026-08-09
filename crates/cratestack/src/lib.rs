#![doc = include_str!("../README.md")]

// This crate intentionally exports no items. The framework's
// public surface lives in `cratestack-pg` (server), `cratestack-api`
// (procedures-only, no-database server), `cratestack-sqlite`
// (embedded), and `cratestack-client` (pure HTTP-client SDKs,
// cratestack#490), all four of which expose their library as
// `cratestack` via Cargo's `package =` rename:
//
// ```toml
// # Backend service
// cratestack = { package = "cratestack-pg", version = "0.7" }
//
// # Procedures-only, no-database backend service
// cratestack = { package = "cratestack-api", version = "0.7" }
//
// # Embedded (mobile / desktop / wasm)
// cratestack = { package = "cratestack-sqlite", version = "0.7" }
//
// # Pure HTTP-client SDK (no cratestack-axum in the dependency graph)
// cratestack = { package = "cratestack-client", version = "0.7" }
// ```
//
// See the README rendered above for the full picture.
