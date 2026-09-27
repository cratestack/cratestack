// `RuntimeHandle` is the blocking FFI surface (Flutter, Swift, Kotlin): it
// owns a current-thread tokio runtime and `block_on`s each request. That
// cannot work in a browser — a `fetch` future only resolves when the JS
// event loop runs, which a blocked thread never lets it do — so the handle
// and the transport only it uses are native-only (cratestack#1104). On
// wasm32, call the async `CratestackClient` / generated client directly.
#[cfg(not(target_arch = "wasm32"))]
pub mod handle;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod transport;
pub mod wire;
