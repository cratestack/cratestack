#![doc = include_str!("../README.md")]

// This crate intentionally exports no items. The framework's
// public surface lives in `cratestack-pg` (server) and
// `cratestack-sqlite` (embedded), both of which expose their
// library as `cratestack` via Cargo's `package =` rename:
//
// ```toml
// # Backend service
// cratestack = { package = "cratestack-pg", version = "0.4" }
//
// # Embedded (mobile / desktop / wasm)
// cratestack = { package = "cratestack-sqlite", version = "0.4" }
// ```
//
// See the README rendered above for the full picture.
