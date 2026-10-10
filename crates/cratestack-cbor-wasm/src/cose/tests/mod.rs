//! `wasm-bindgen-test`s of `ClientEnvelope` against the shared vectors
//! (`cratestack-cose/tests/vectors`, read in place at compile time, so no
//! vector file is copied). Run with `wasm-pack test --headless --chrome --
//! --features cose`.

// The crate's other tests run in Node by default; `wasm-pack test --headless
// --chrome` skips a suite that is configured for it only. These run in the
// browser the Dart package's web backend targets, and so does the rest of
// the crate's suite whenever `cose` is on.
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

mod misuse;
mod support;
mod vectors;
