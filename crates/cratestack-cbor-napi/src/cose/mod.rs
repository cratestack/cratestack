//! The COSE signed transport for Node (issue #1010, ADR 0006 §11): a
//! `ClientEnvelope` class that seals requests and opens responses with
//! `cratestack-cose`, the one implementation, byte-identical to the Rust
//! client and the web build (`crates/cratestack-cbor-wasm/src/cose`).
//!
//! ```text
//! JS binding object ──▶ BindingSpec ──▶ CallBinding ──▶ CoseEnvelope
//!   ▲                     (canonical query, AAD inputs: cratestack-cose)
//!   └── Uint8Array / { payload, kid, alg, thumbprint } / { code, message }
//! ```
//!
//! The split follows `lib.rs`: [`envelope`], [`spec`], [`external`] and
//! [`fail`] are plain Rust and run under `cargo test`; `shell` is the napi
//! class around them and is exercised by the vitest suite of
//! `packages/cratestack-cbor-node`.
//!
//! Errors are `{ code, message }`, `code` being `"rejected"` (every failed
//! verification, with an empty message) or `"misuse"`, as in the web build.

mod calls;
mod drive;
mod envelope;
mod external;
mod fail;
#[cfg(not(target_arch = "wasm32"))]
mod inputs;
#[cfg(not(target_arch = "wasm32"))]
mod pending;
#[cfg(not(target_arch = "wasm32"))]
mod shell;
mod spec;

#[cfg(test)]
mod tests;

#[cfg(not(target_arch = "wasm32"))]
pub use pending::{PendingSeal, canonical_query, contract_header_value};
#[cfg(not(target_arch = "wasm32"))]
pub use shell::ClientEnvelope;
