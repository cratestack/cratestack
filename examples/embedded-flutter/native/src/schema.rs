//! Wrap `include_embedded_schema!` in its own module so the generated
//! `cratestack_schema` symbols don't bleed into the crate root. The
//! schema file is resolved relative to this crate's `CARGO_MANIFEST_DIR`
//! (so `../notes.cstack` from this src/ file means the `notes.cstack`
//! sitting next to `Cargo.toml`).

use cratestack_macros::include_embedded_schema;

include_embedded_schema!("notes.cstack");
