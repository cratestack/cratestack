//! Thin re-export of [`cratestack_studio::eject`].
//!
//! Phase 0 emptied this crate. Phase 2 wires the real eject path
//! through `cratestack-studio` and re-exposes it here so existing
//! consumers (the CLI) keep their stable import surface. New code
//! should depend on `cratestack-studio` directly.

pub use cratestack_studio::{EjectError, EjectOptions, EjectReport, eject};
