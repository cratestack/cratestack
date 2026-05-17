//! CrateStack Studio web UI. Leptos CSR. Served by Trunk in dev,
//! bundled into the binary by Phase 2's `studio eject` path.

mod api;
mod app;
mod editors;
mod tools;
mod types;

use leptos::prelude::*;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    mount_to_body(app::App);
}
