#![doc = include_str!("../README.md")]

// This crate intentionally exports no items. The framework's
// public surface lives in `cratestack-pg` (server), `cratestack-api`
// (procedures-only, no-database server), and `cratestack-sqlite`
// (embedded), all three of which expose their library as `cratestack`
// via Cargo's `package =` rename:
//
// ```toml
// # Backend service
// cratestack = { package = "cratestack-pg", version = "0.4" }
//
// # Procedures-only, no-database backend service
// cratestack = { package = "cratestack-api", version = "0.6" }
//
// # Embedded (mobile / desktop / wasm)
// cratestack = { package = "cratestack-sqlite", version = "0.4" }
// ```
//
// See the README rendered above for the full picture.
