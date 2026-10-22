// `.cstack` field names are camelCase; the generated structs mirror them.
#![allow(non_snake_case)]

//! Device half of sync-notes: `schema.cstack` through
//! `include_embedded_schema!`, wrapped in a `cratestack-sync` engine.
//!
//! [`app::NotesApp`] is plain Rust and is what the tests drive natively.
//! [`wasm`] is a thin `wasm-bindgen` skin over it for the demo page's
//! Dedicated Workers, which own all networking: the engine is sans-IO.

use cratestack_macros::include_embedded_schema;

include_embedded_schema!("../schema.cstack");

pub mod app;
#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(target_arch = "wasm32")]
pub use wasm::*;
