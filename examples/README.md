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
cd examples/embedded-browser-vite/web      # or -webpack/web, or -vite-pwa/web
pnpm install
pnpm run dev                                # auto-runs wasm-pack first
```

Run the Tauri example:

```bash
cd examples/tauri-web                       # project root — tauri-cli walks down for the conf
pnpm install
pnpm tauri dev                              # spawns Vite + the Tauri shell
```

The bundled `examples/scripts/wasm-build.mjs` helper detects Homebrew LLVM at `/opt/homebrew/opt/llvm/bin/clang` (or the Intel-Mac equivalent) and points `cc-rs` at it so `pnpm run dev` works out of the box on macOS.

## Phase C — Mobile + native desktop (next release)

Coming in a follow-up PR:

- `embedded-flutter/` — `flutter_rust_bridge` glue around `include_embedded_schema!` with a minimal Flutter screen
- `embedded-expo/` — Expo native module wrapping the Rust cdylib for React Native (iOS + Android)
- `tauri-native/` — sibling to `tauri-web` where the **embedded** side also moves to native Rust delegates over Tauri IPC (no wasm in the webview; the renderer becomes a pure view layer)

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
