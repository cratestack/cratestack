fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    napi_build::setup();
}
