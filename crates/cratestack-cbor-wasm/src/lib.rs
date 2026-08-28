//! `wasm-bindgen` bindings for `cratestack-codec-cbor`'s `CborCodec`,
//! built into the `@cratestack/cbor-web` npm package (issue #287, epic
//! #285). This crate reimplements no CBOR logic of its own — it wraps
//! the existing, already-tested `CborCodec` for `wasm32-unknown-unknown`.
//!
//! All `wasm-bindgen`-specific code (and its dependencies) lives behind
//! `cfg(target_arch = "wasm32")`, mirroring `crates/cratestack-rusqlite`
//! and `examples/embedded-browser-vite`: on a plain host toolchain this
//! crate is effectively empty, so `cargo check --workspace` / `cargo test
//! --workspace` never require the wasm32 target or `wasm-bindgen-cli`.
//! Only `wasm-pack build --target web` (which needs the wasm32 target)
//! produces the real `.wasm` artifact this package ships.
mod value_bridge;

#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(target_arch = "wasm32")]
pub use wasm::*;
