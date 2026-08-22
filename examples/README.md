# CrateStack Examples

Runnable, end-to-end examples covering the three deployment shapes CrateStack supports as of 0.3.0. Each example is a self-contained Cargo workspace member with its own README, schema, and tests.

Two homes for examples in this repository:

- **`crates/cratestack-sqlite/examples/` and `crates/cratestack-pg/examples/`** — cargo-native examples that live inside the backend crate they exercise. Run via `cargo run --example <name> -p cratestack-sqlite` (or `-p cratestack-pg`). Use these when the example is small enough to fit one file and only needs that crate's dev-dependencies. Note the package is the **backend** crate, never `-p cratestack`: that name selects the documentation-only vitrine crate, which has no examples and no tests, so targeting it returns a false green rather than an error you'd notice.
- **`examples/`** (this directory) — standalone workspace members with their own `Cargo.toml`, dependencies, tests, and binary entry. Use these when the example needs its own dependency surface (`clap` for a CLI, dev-dependencies for mock servers, etc.) or when the example is itself a multi-file template.

All examples build and run under `cargo build --workspace` / `cargo test --workspace`.

## Phase A — Pure Rust (shipped in this release)

| Example | Macro(s) | Shape |
|---|---|---|
| [`crates/cratestack-sqlite/examples/sqlite_quickstart.rs`](../crates/cratestack-sqlite/examples/sqlite_quickstart.rs) | `include_embedded_schema!` | Smallest embedded program — in-memory DB, one model, CRUD |
| [`crates/cratestack-sqlite/examples/sqlite_offline_first.rs`](../crates/cratestack-sqlite/examples/sqlite_offline_first.rs) | `include_embedded_schema!` | File-backed DB, two models, exact-precision `Decimal` |
| [`crates/cratestack-sqlite/examples/sqlite_ffi_dispatch.rs`](../crates/cratestack-sqlite/examples/sqlite_ffi_dispatch.rs) | `include_embedded_schema!` | JSON FFI envelope dispatcher you'd wrap with `flutter_rust_bridge` |
| [`crates/cratestack-pg/examples/server_basic.rs`](../crates/cratestack-pg/examples/server_basic.rs) | `include_server_schema!` | Postgres + axum router + procedure registry + host auth provider |
| [`embedded-cli/`](embedded-cli) | `include_embedded_schema!` | `clap`-driven note-taking CLI against a file-backed SQLite database |
| [`embedded-daemon/`](embedded-daemon) | `include_embedded_schema!` | Long-running tokio + `notify` daemon: debounces filesystem events, persists through `spawn_blocking`. The canonical "async I/O on the outside, sync `ModelDelegate` on the inside" example |
| [`embedded-webhook/`](embedded-webhook) | `include_embedded_schema!` | Single-binary axum HTTP webhook receiver with its own SQLite — the inverted twin of `server_basic`'s Postgres setup, for edge / single-tenant deployments |
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

The Rust side of all three is *checked* in CI, not tested — CI's `check`/`msrv` jobs run
`just check`, which `cargo check --workspace`s everything except `embedded_flutter_native`
(that crate needs `flutter_rust_bridge_codegen`-generated glue that isn't checked in; see
`embedded-flutter/README.md`), so `tauri-native-shell-example` and `embedded-expo-native`
get a compile check but `embedded_flutter_native` doesn't even do that.

Their `cargo test` coverage differs per crate, as of #597:

- **`embedded-expo-native` runs in CI.** Its three tests (dispatcher round-trip and two error
  paths) execute in `test-ci-host` — they need no platform SDK, only the host toolchain.
- **`tauri-native-shell-example` does not.** Still excluded: its 2 tests need the GTK/WebKit
  dev packages no test job installs.
- **`embedded_flutter_native` does not**, and can't — it is excluded from `--workspace`
  entirely, since its `mod frb_generated;` source is generated and gitignored.

To run their Rust tests locally:

```bash
cargo test -p tauri-native-shell-example
just frb-generate examples/embedded-flutter && cargo test -p embedded_flutter_native
cargo test -p embedded-expo-native
```

The mobile front-ends (Flutter / Expo) require platform SDKs (Flutter SDK, Xcode, Android NDK + `cargo-ndk`) that are scoped per-example — see each README for the bootstrap. The native-side scaffolding is a mix: Flutter's platform directories (`ios/`, `android/`, …) and Expo's `app/ios/`, `app/android/` prebuild output are generated by the host tooling on first checkout and gitignored, but the Expo *native module* source (`CratestackNotesModule.kt`/`.swift`, its podspec, `build.gradle`) is checked in and hand-completed — see `embedded-expo/README.md` before running its bootstrap step.

## Phase D — Generated TypeScript client presets

| Example | Macro(s) | Shape |
|---|---|---|
| [`react-vite-swr/`](react-vite-swr) | `include_server_schema!` (`transport rest`) | Postgres + axum REST server, plus a `cratestack generate-typescript --swr` client (checked in under `client/`) consumed by a real React + Vite app with **zero hand-written data-fetching code** — every `useSWR`/`useSWRMutation` call, cache key, and fetcher is generated. Also calls a plain generated function from a Node script, outside React, proving the swr layout's two-layer (plain functions + hooks) design is real. |
| [`react-vite-refine/`](react-vite-refine) | No server macro — schema only (`transport rest`) | No database, no hand-written server: a `cratestack generate-typescript --refine` client + `cratestack generate-wiremock` stubs drive a real [refine.dev](https://refine.dev) admin app (`@cratestack/refine`'s `createCratestackDataProvider`) against a live, stateful WireMock container built from `crates/cratestack-mock-wiremock`. Three models exercise plain CRUD, `@@paged` + `@version` optimistic locking, and a non-`id` primary key. |

```bash
docker compose up -d postgres
DATABASE_URL=postgres://cratestack:cratestack@localhost:55432/cratestack_test cargo run -p react-vite-swr-example
cd examples/react-vite-swr && pnpm install && pnpm --filter react-vite-swr-web run dev
```

See [`react-vite-swr/README.md`](react-vite-swr/README.md) for the full schema → generate → run path.

```bash
just react-vite-refine-fixture
docker build -t cratestack-wiremock-stateful -f crates/cratestack-mock-wiremock/docker/Dockerfile crates/cratestack-mock-wiremock/docker
docker run -d --name cratestack-refine-mock -p 8080:8080 -v "$(pwd)/examples/react-vite-refine/wiremock/mappings:/home/wiremock/mappings:ro" cratestack-wiremock-stateful
cd examples/react-vite-refine/web && pnpm install && pnpm run dev
```

See [`react-vite-refine/README.md`](react-vite-refine/README.md) for the full run path, the
deliberate "no sort/filter/pagination controls" decision, and the confirmed gaps in what this mock
backend can prove (`If-Match` is sent but not enforced; `create` never honors a client-submitted
primary key).

## Phase E — Generated Dart/Flutter client presets

| Example | Macro(s) | Shape |
|---|---|---|
| [`flutter-riverpod/`](flutter-riverpod) | `include_server_schema!` (`transport rest`, reused from `react-vite-swr`) | A `cratestack generate-dart --preset riverpod` client (checked in under `client/`) consumed by a real Flutter app with **zero hand-written providers** — every `ref.watch(boardListProvider)` / controller call is generated. Also the end-to-end proof for `generate-dart --run-build-runner` (issue #303): the CLI flag that runs `dart run build_runner build --delete-conflicting-outputs` for you after generation. |

```bash
docker compose up -d postgres
DATABASE_URL=postgres://cratestack:cratestack@localhost:55432/cratestack_test cargo run -p react-vite-swr-example
cd examples/flutter-riverpod/app && flutter create . --org dev.cratestack.examples --platforms=macos,ios,android
flutter run -d macos
```

See [`flutter-riverpod/README.md`](flutter-riverpod/README.md) for the full schema → generate →
build_runner → run path.

## Phase F — RPC transport (framework-native)

`transport rpc` is CrateStack's own JSON/CBOR RPC binding — two endpoints (`POST /rpc/{op_id}`,
`POST /rpc/batch`) dispatched by a generated string match. See
[`docs/design/rpc-transport.md`](../docs/design/rpc-transport.md).

| Example | Macro(s) | Shape |
|---|---|---|
| [`rpc-procedures/`](rpc-procedures) | `include_server_schema!` (`transport rpc`, `db = None`) | Smallest possible RPC server — one query procedure and one mutation procedure, no database, no models. Proves the RPC binding's unary route shape in isolation |
| [`rpc-batch/`](rpc-batch) | `include_server_schema!` (`transport rpc`, `db = None`) | RPC server demonstrating `POST /rpc/batch` — multiple op calls in one round-trip, per-frame error isolation, request order preserved on the response |
| [`rpc-batch-debounce/`](rpc-batch-debounce) | `include_server_schema!` (`transport rpc`, `db = None`) | Client-side `BatchDebouncer` that coalesces independent RPC calls into a single `POST /rpc/batch`, wrapping the in-process `rpc-batch` router |
| [`rpc-streaming/`](rpc-streaming) | `include_server_schema!` (`transport rpc`, `db = None`) | RPC server streaming a list-return procedure via `Accept: application/cbor-seq` — the same route serves a single CBOR `Vec` or a stream of CBOR chunks depending on content negotiation |
| [`rpc-streaming-client-rust/`](rpc-streaming-client-rust) | `include_client_schema!` | Rust client consuming the `rpc-streaming` server's cbor-seq stream through a typed, generated streaming method (`RpcStream<Tick>`) — companion to `rpc-streaming` |

```bash
cargo run -p rpc-procedures-example
cargo run -p rpc-batch-example
cargo run -p rpc-batch-debounce-example
cargo run -p rpc-streaming-example

# rpc-streaming-client-rust needs rpc-streaming running in another terminal:
REMOTE_URL=http://localhost:3001 cargo run -p rpc-streaming-client-rust-example
```

## Standalone verification crates (not workspace members)

These two prove a dependency-graph property that only holds outside Cargo's workspace-wide feature
unification, so each is deliberately listed in the root `Cargo.toml`'s `[workspace] exclude` array
instead of `members` (see the comments there and their own READMEs for why) — run their checks from
inside their own directory rather than via `-p` like the rest of this directory.

| Example | Macro(s) | Shape |
|---|---|---|
| [`no-database-verification/`](no-database-verification) | `include_server_schema!` (`db = None`) | Standalone crate with its own `Cargo.lock` proving via `cargo tree` that `sqlx`/`cratestack-sqlx` are absent from a `cratestack-pg`, `default-features = false` consumer by default, and present once the `postgres` feature is enabled |
| [`no-database-verification-api/`](no-database-verification-api) | `include_server_schema!` (`db = None`) | Same `cargo tree` proof for a `cratestack-api` consumer, which never depends on `cratestack-sqlx` under any feature |

```bash
cd examples/no-database-verification && cargo tree | grep -i sqlx   # -> no output
cd examples/no-database-verification-api && cargo tree | grep -i sqlx   # -> no output
```

## Verification matrix

Snapshot of what's been actually exercised end-to-end against a real runtime, vs. what's only been built and unit-tested. Point-in-time as of the linked commit; rerun whenever the matrix drifts.

| Example | `cargo test` | End-to-end | Verified surface |
|---|---|---|---|
| `embedded-cli` | ✅ | ✅ | `cargo run`: add / list / count / mark-done / delete persist to a file-backed SQLite |
| `embedded-daemon` | ✅ | ✅ | Watched `/tmp/cratestack-daemon-test` while bursting writes; rows persisted with `bursts > 1` confirming the debouncer collapsed them |
| `embedded-webhook` | ✅ | ✅ | `curl` POST / GET / list / mark-processed round-trips against a real `127.0.0.1` bind, rows persisted to the file-backed SQLite |
| `client-stub-rust` | ✅ | ✅ | `cargo run` prints the typed client surface (live HTTP call requires a remote service) |
| `client-multi-service` | ✅ | ✅ | `cargo run` prints both upstream surfaces |
| `microservice-pair` | ✅ | ✅ | `cargo run` prints server + client surfaces |
| `embedded-browser-vite` | ✅ | ✅ | `pnpm dev` → headless browser: add / mark-done via wasm worker (in-memory fallback on this preview browser; OPFS path covered by webpack variant) |
| `embedded-browser-webpack` | ✅ | ✅ | `pnpm dev` → headless browser: add / list with **OPFS persistence** |
| `embedded-browser-vite-pwa` | ✅ | ✅ | `pnpm dev` → headless browser: add / list with OPFS + service worker registered |
| `react-vite-daisyui` | ✅ | ✅ | `pnpm dev` → React + DaisyUI render + wasm/OPFS CRUD |
| `react-nextjs-daisyui` | ✅ | ✅ | `pnpm dev` → all 3 tabs: local OPFS write, sync push to napi-rs SQLite, server tab reads back, remote tab errors cleanly |
| `tauri-web` | ✅ | ⚠ build-only | `cargo test` + `pnpm exec tauri info` + `vite build` clean. `pnpm tauri dev` (opens a desktop window) deferred to the developer |
| `tauri-native` | ✅ | ⚠ build-only | Same as above |
| `embedded-flutter` | ✅ | ✅ macOS desktop + ✅ Android APK | `flutter run -d macos`: 6 CRUD rows via the Material 3 UI, persisted to the sandboxed app-data SQLite. `flutter build apk` for arm64-v8a / armeabi-v7a / x86_64 lands `libembedded_flutter_native.so` in the APK. **iOS: not tested — out of scope for now.** |
| `embedded-expo` | ✅ | ✅ Android emulator | `npx expo run:android` on Pixel_10_Pro: 6 CRUD rows via React Native UI, persisted to `/data/data/.../files/cratestack-notes.db` (SQLite + WAL). **iOS: not tested — out of scope for now.** Build path is set up (podspec vendors `libembedded_expo_native.a`, Swift uses `@_silgen_name` against the same C ABI), but full `expo run:ios` needs an installed iOS Simulator runtime (Xcode → Settings → Components) that wasn't on the verification host. |
| `react-vite-swr` | ✅ | ✅ | Real server booted against Postgres; `pnpm --filter react-vite-swr-web run dev` in a real browser (Claude Preview) — created a board via `useCreateBoard` and watched the list refresh with no manual refetch, opened its detail screen, toggled/deleted tasks via `useUpdateTask`/`useDeleteTask` (both invalidated the list live), watched the `estimateFocusMinutes` procedure hook's estimate recompute automatically each time. `pnpm run seed` (`tsx`, plain generated functions, no React) separately created data and called the procedure outside any component. |
| `flutter-riverpod` | n/a (no Rust crate — reuses `react-vite-swr`'s server; `client/`'s own `flutter test` is the analog) | ✅ macOS desktop | Real `react-vite-swr` server booted against Postgres; `flutter run -d macos` — created a board via the generated `BoardCreateController` and watched the list refresh (`ref.invalidate(boardListProvider)`, no manual refetch), opened its detail screen, toggled/deleted tasks via the generated `TaskUpdateController`/`TaskDeleteController` (both invalidated the list live), watched the `estimateFocusMinutes` procedure provider's estimate recompute automatically each time. Two real bugs found and fixed live during this run (a missing `Accept` header in `CratestackDioAdapter`, and a controller auto-dispose race) — see `flutter-riverpod/README.md`. |
| `react-vite-refine` | ✅ (5 offline tests, no Docker) | ✅ | Real stateful WireMock container (built from `crates/cratestack-mock-wiremock/docker/Dockerfile`) — `pnpm run verify` (`web/scripts/verify.ts`, outside React) drove create → list → update → delete → 404 live for all three models, confirmed the falsy `published: false` round trip, and confirmed (asserted, not assumed) that a stale `If-Match` gets `200` not `412` against this mock. Also driven by hand in a real browser (Claude Preview): added/edited/deleted rows on all three tabs, confirmed against the container's own state via `fetch`. See `react-vite-refine/README.md`'s "What this demo can't prove" for the two confirmed gaps this surfaced in `cratestack-mock-wiremock`. |

What "end-to-end" means here:

- For native binaries: launched the process and observed correct CRUD against the persisted SQLite file.
- For browser examples: started the dev server, loaded the page in a headless browser via Claude Preview, filled the form, clicked Save, snapshotted the DOM to confirm rows landed.
- For full-stack examples (Next.js): exercised all routes / tabs and confirmed the data flow across all surfaces.
- For mobile: launched the app on a real emulator, added rows through the on-device UI, and read the underlying SQLite file out via `adb` / Finder.

What "build-only" means: `cargo test`, `cargo check --workspace`, `vite build`, `tauri info` (config discovery) are clean — meaningful compile-time / config-time signals — but no human-or-bot interaction with the running app was performed.

## How to run every example at once

```bash
cargo test --workspace        # tests for every example
cargo build --workspace       # builds every example binary

# Run a specific cargo example — the package is the BACKEND crate that owns it,
# not `-p cratestack` (that is the empty vitrine crate; see "Two homes" above).
cargo run --example sqlite_quickstart -p cratestack-sqlite
cargo run --example server_basic      -p cratestack-pg

# Run a specific standalone example:
cargo run -p embedded-cli-example -- --db /tmp/notes.db add "First"
cargo run -p client-stub-rust-example
cargo run -p client-multi-service-example
cargo run -p microservice-pair-example
```

## Picking an example

| If you want to… | Read this |
|---|---|
| Stand up a CrateStack server quickly | [`server_basic`](../crates/cratestack-pg/examples/server_basic.rs) |
| Build an offline-first mobile/desktop app | [`embedded-cli`](embedded-cli) (start here) → `sqlite_offline_first` → `sqlite_ffi_dispatch` |
| Call another CrateStack service from Rust | [`client-stub-rust`](client-stub-rust) |
| Aggregate calls to multiple services | [`client-multi-service`](client-multi-service) |
| Build a microservice that talks to other microservices | [`microservice-pair`](microservice-pair) |
| Run the schema in a browser tab (OPFS) | [`embedded-browser-vite`](embedded-browser-vite) — or `embedded-browser-webpack` if your shop uses Webpack |
| Run the schema in React + a real component library | [`react-vite-daisyui`](react-vite-daisyui) |
| Run all three CrateStack surfaces in one app with offline-first sync | [`react-nextjs-daisyui`](react-nextjs-daisyui) |
| Build a thick desktop app with everything in native Rust | [`tauri-native`](tauri-native) |
| Run a long-running async daemon that persists locally | [`embedded-daemon`](embedded-daemon) |
| Stand up a small HTTP service with its own SQLite | [`embedded-webhook`](embedded-webhook) |
| Drive the schema from Flutter (iOS + Android + desktop) | [`embedded-flutter`](embedded-flutter) |
| Drive the schema from React Native + Expo | [`embedded-expo`](embedded-expo) |
| Consume a generated REST/RPC TypeScript client with zero hand-written data-fetching code (SWR hooks) | [`react-vite-swr`](react-vite-swr) |
| Consume a generated Dart/Flutter client with zero hand-written Riverpod providers | [`flutter-riverpod`](flutter-riverpod) |
