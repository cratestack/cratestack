# CrateStack Examples

Runnable, end-to-end examples covering the three deployment shapes CrateStack supports as of 0.3.0. Each example is a self-contained Cargo workspace member with its own README, schema, and tests.

Two homes for examples in this repository:

- **`crates/cratestack/examples/`** — cargo-native examples that live inside the `cratestack` facade crate. Run via `cargo run --example <name> -p cratestack`. Use these when the example is small enough to fit one file and only needs the facade's dev-dependencies.
- **`examples/`** (this directory) — standalone workspace members with their own `Cargo.toml`, dependencies, tests, and binary entry. Use these when the example needs its own dependency surface (`clap` for a CLI, dev-dependencies for mock servers, etc.) or when the example is itself a multi-file template.

All examples build and run under `cargo build --workspace` / `cargo test --workspace`.

## Phase A — Pure Rust (shipped in this release)

| Example | Macro(s) | Shape |
|---|---|---|
| [`crates/cratestack/examples/sqlite_quickstart.rs`](../crates/cratestack/examples/sqlite_quickstart.rs) | `include_embedded_schema!` | Smallest embedded program — in-memory DB, one model, CRUD |
| [`crates/cratestack/examples/sqlite_offline_first.rs`](../crates/cratestack/examples/sqlite_offline_first.rs) | `include_embedded_schema!` | File-backed DB, two models, exact-precision `Decimal` |
| [`crates/cratestack/examples/sqlite_ffi_dispatch.rs`](../crates/cratestack/examples/sqlite_ffi_dispatch.rs) | `include_embedded_schema!` | JSON FFI envelope dispatcher you'd wrap with `flutter_rust_bridge` |
| [`crates/cratestack/examples/server_basic.rs`](../crates/cratestack/examples/server_basic.rs) | `include_server_schema!` | Postgres + axum router + procedure registry + host auth provider |
| [`embedded-cli/`](embedded-cli) | `include_embedded_schema!` | `clap`-driven note-taking CLI against a file-backed SQLite database |
| [`client-stub-rust/`](client-stub-rust) | `include_client_schema!` | Standalone HTTP client; the "Rust service that calls another Rust service" shape |
| [`client-multi-service/`](client-multi-service) | Two `include_client_schema!` calls | BFF / orchestrator that fans out to two upstream services concurrently |
| [`microservice-pair/`](microservice-pair) | `include_server_schema!` + `include_client_schema!` | Service that owns its own database AND calls an upstream — the canonical microservice shape |

## Phase B — Browser / wasm32 + desktop shell

| Example | Macro(s) | Shape |
|---|---|---|
| [`embedded-browser-vite/`](embedded-browser-vite) | `include_embedded_schema!` | `wasm32-unknown-unknown` + Vite + TypeScript, OPFS persistence inside a Dedicated Worker |
| [`embedded-browser-webpack/`](embedded-browser-webpack) | `include_embedded_schema!` | Same Rust crate as Vite, Webpack 5 + ts-loader config delta |
| [`embedded-browser-vite-pwa/`](embedded-browser-vite-pwa) | `include_embedded_schema!` | Same Rust crate, Vite + `vite-plugin-pwa` — installable PWA with Workbox-generated service worker precaching the wasm bundle |
| [`react-vite-daisyui/`](react-vite-daisyui) | `include_embedded_schema!` | React 19 + Vite 8 + Tailwind 4 + DaisyUI 5 — same wasm/OPFS shape with a real component library on top |
| [`react-nextjs-daisyui/`](react-nextjs-daisyui) | `include_embedded_schema!` (×2) **and** `include_client_schema!` | Next.js 16 App Router with three CrateStack surfaces: wasm/OPFS in the browser, napi-rs `.node` addon on the Node side, typed HTTP client to upstream services. Serwist PWA + offline-first sync engine reconciling OPFS ↔ napi over a delta protocol. |
| [`tauri-web/`](tauri-web) | `include_embedded_schema!` **and** `include_client_schema!` | Tauri 2 desktop shell. Webview hosts the embedded wasm (OPFS); native shell hosts the typed HTTP client called via Tauri commands. |

Build prerequisites for all four:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
brew install llvm                    # macOS — sqlite-wasm-rs needs wasm-capable clang
# (Linux: distro clang 14+ works directly)
```

`tauri-web` additionally needs the Tauri 2 platform deps — see [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/) (macOS: Xcode CLI; Linux: GTK + WebKit; Windows: MSVC + WebView2).

Run any browser example:

```bash
cd examples/embedded-browser-vite/web      # or -webpack/web, -vite-pwa/web, or react-vite-daisyui/web
pnpm install
pnpm run dev                                # auto-runs wasm-pack first
```

Run the Tauri example:

```bash
cd examples/tauri-web                       # project root — tauri-cli walks down for the conf
pnpm install
pnpm tauri dev                              # spawns Vite + the Tauri shell
```

Run the Next.js example (pnpm workspace with napi-rs addon):

```bash
cd examples/react-nextjs-daisyui
pnpm install                                # installs both web/ and napi/
pnpm --filter react-nextjs-daisyui-example run dev
                                            # builds wasm + napi, then next dev
```

The bundled `examples/scripts/wasm-build.mjs` helper detects Homebrew LLVM at `/opt/homebrew/opt/llvm/bin/clang` (or the Intel-Mac equivalent) and points `cc-rs` at it so `pnpm run dev` works out of the box on macOS.

## Phase C — Mobile + native desktop

| Example | Macro(s) | Shape |
|---|---|---|
| [`tauri-native/`](tauri-native) | `include_embedded_schema!` **and** `include_client_schema!` | Tauri 2 desktop shell with **everything CrateStack-shaped in native Rust**. Renderer is a pure view layer — every data op (local SQLite + remote HTTP) goes through Tauri commands. Compare with `tauri-web` to see the wasm-in-webview vs. native-Rust split. |
| [`embedded-flutter/`](embedded-flutter) | `include_embedded_schema!` | Flutter app bridged via [`flutter_rust_bridge`](https://cjycode.com/flutter_rust_bridge/) 2.x. Material 3 UI over a Dart-generated API surface; same `ModelDelegate` shape as the CLI and browser examples. |
| [`embedded-expo/`](embedded-expo) | `include_embedded_schema!` (via FFI dispatch) | React Native (Expo SDK 55) calling into a Rust cdylib through a local Expo native module. Uses `cratestack_rusqlite::ffi::{OperationRequest, OperationResponse}` as the JSON envelope across the JS↔native boundary. |

The Rust side of all three is workspace-tested in CI:

```bash
cargo test -p tauri-native-shell-example
cargo test -p embedded-flutter-native
cargo test -p embedded-expo-native
```

The mobile front-ends (Flutter / Expo) require platform SDKs (Flutter SDK, Xcode, Android NDK + `cargo-ndk`) that are scoped per-example — see each README for the bootstrap. The native-side scaffolding (Flutter platform dirs, Expo native module Swift/Kotlin) is **generated by the host tooling on first checkout** rather than checked in.

## How to run every example at once

```bash
cargo test --workspace        # tests for every example
cargo build --workspace       # builds every example binary

# Run a specific cargo example:
cargo run --example sqlite_quickstart -p cratestack
cargo run --example server_basic       -p cratestack

# Run a specific standalone example:
cargo run -p embedded-cli-example -- --db /tmp/notes.db add "First"
cargo run -p client-stub-rust-example
cargo run -p client-multi-service-example
cargo run -p microservice-pair-example
```

## Picking an example

| If you want to… | Read this |
|---|---|
| Stand up a CrateStack server quickly | [`server_basic`](../crates/cratestack/examples/server_basic.rs) |
| Build an offline-first mobile/desktop app | [`embedded-cli`](embedded-cli) (start here) → `sqlite_offline_first` → `sqlite_ffi_dispatch` |
| Call another CrateStack service from Rust | [`client-stub-rust`](client-stub-rust) |
| Aggregate calls to multiple services | [`client-multi-service`](client-multi-service) |
| Build a microservice that talks to other microservices | [`microservice-pair`](microservice-pair) |
| Run the schema in a browser tab (OPFS) | [`embedded-browser-vite`](embedded-browser-vite) — or `embedded-browser-webpack` if your shop uses Webpack |
| Run the schema in React + a real component library | [`react-vite-daisyui`](react-vite-daisyui) |
| Run all three CrateStack surfaces in one app with offline-first sync | [`react-nextjs-daisyui`](react-nextjs-daisyui) |
| Build a thick desktop app with everything in native Rust | [`tauri-native`](tauri-native) |
| Drive the schema from Flutter (iOS + Android + desktop) | [`embedded-flutter`](embedded-flutter) |
| Drive the schema from React Native + Expo | [`embedded-expo`](embedded-expo) |
