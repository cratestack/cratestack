# tauri-native

Tauri 2 desktop shell where **everything CrateStack-shaped lives in native Rust** — both the embedded SQLite path and the typed HTTP client. The webview is a pure view layer; every data operation goes through a `#[tauri::command]`.

This is the Phase C sibling of [`tauri-web`](../tauri-web). The HTTP-side architecture is unchanged — what moves is the local data layer:

|            | `tauri-web`                                 | `tauri-native` (this)                       |
|------------|---------------------------------------------|---------------------------------------------|
| Local data | `include_embedded_schema!` → wasm32 in the webview, OPFS-backed | `include_embedded_schema!` → native Rust in the shell, file-backed |
| Remote     | `include_client_schema!` in the shell       | `include_client_schema!` in the shell (identical) |
| Renderer   | Vanilla TS + worker holding wasm runtime    | Vanilla TS, no worker, no wasm — IPC only   |

## Why this shape exists

Pushing the embedded path out of the webview lets you:

- Use the OS-native SQLite (or any rusqlite-supported VFS) with full filesystem semantics — no OPFS sandbox.
- Share the SQLite file with non-webview code paths (background tasks, sync workers, CLI tooling on the same install).
- Drop the wasm toolchain from the build pipeline entirely — Phase B's wasm-pack + LLVM-clang dance isn't needed here.
- Keep the renderer minimal: no `Worker`, no `init()` boot dance, no OPFS feature detection. Vite serves a static index + JS bundle that does nothing but post messages to native.

The tradeoff: the renderer always needs the shell. A `tauri-web` build can in principle run the local notes UI inside any browser; `tauri-native`'s renderer is meaningless without the Tauri runtime serving the `invoke()` calls.

## Layout

```
tauri-native/
├── package.json               # ROOT: @tauri-apps/cli + tauri scripts
├── pnpm-workspace.yaml        # web/ as the only workspace package
├── src-tauri/
│   ├── Cargo.toml             # the only Cargo crate in this example
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── icons/icon.png
│   ├── notes.cstack           # local Note model (embedded)
│   ├── articles.cstack        # remote Article model (client contract)
│   └── src/
│       ├── lib.rs             # AppState + the six Tauri commands
│       └── main.rs            # thin entry: tauri_native_shell_example_lib::run()
├── web/
│   ├── package.json           # vite + @tauri-apps/api (no cli)
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── index.html             # two stacked panels: local notes + remote articles
│   └── src/
│       ├── main.ts            # form handlers + render functions; only invoke() ops
│       └── protocol.ts        # Note / Article TS types mirroring the Rust JsNote / JsArticle
└── README.md
```

## Prerequisites

- Rust + cargo.
- **Tauri 2 system deps** — see [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/). On macOS, Xcode CLI tools cover it; on Linux, GTK + WebKit GTK; on Windows, MSVC + WebView2.
- Node.js 20+ and pnpm.

Notably **not** required, in contrast to Phase B: `wasm32-unknown-unknown`, `wasm-pack`, Homebrew LLVM, or any wasm bundler configuration.

## Run

```bash
cd examples/tauri-native     # project root, NOT web/ — tauri-cli walks down for the conf
pnpm install
pnpm tauri dev
# or:
pnpm run dev
```

The data lives at the OS-appropriate app data dir:

- macOS: `~/Library/Application Support/dev.cratestack.examples.tauri-native/notes.db`
- Linux: `~/.local/share/dev.cratestack.examples.tauri-native/notes.db`
- Windows: `%APPDATA%\dev.cratestack.examples.tauri-native\notes.db`

Tauri's `app_handle.path().app_data_dir()` resolves this and the example creates the parent directory + bootstraps the `Note` table on first launch.

## The six Tauri commands

`src-tauri/src/lib.rs` exposes these to the renderer:

| Command                    | Purpose                                                  |
|----------------------------|----------------------------------------------------------|
| `list_notes(onlyOpen)`     | Find-many on the Note model, optionally filtering completed |
| `add_note(input)`          | Create a Note with a fresh UUID + current timestamps     |
| `mark_done(id)`            | Update the Note's `completed = true` + bump `updatedAt`  |
| `delete_note(id)`          | Delete by id                                             |
| `fetch_remote_articles(baseUrl)` | Call an upstream CrateStack service for Article rows via the typed client |
| `surface_summary()`        | Returns metadata about both schemas (debug helper)       |

All of these (except `fetch_remote_articles`, which is `async`) are synchronous — the inner `rusqlite` call serializes through the runtime's mutex, and Tauri's command runtime runs them on a thread pool so the UI stays responsive.

## Why two `include_*_schema!` macros in one crate

Both macros emit a module named `cratestack_schema`. To avoid the collision we wrap each call in its own module:

```rust
mod notes_schema {
    use cratestack_macros::include_embedded_schema;
    include_embedded_schema!("notes.cstack");
}

mod articles_schema {
    use cratestack_macros::include_client_schema;
    include_client_schema!("articles.cstack");
}
```

Same pattern as [`react-nextjs-daisyui/napi`](../react-nextjs-daisyui/napi). The schema paths resolve relative to `CARGO_MANIFEST_DIR`, so the two `.cstack` files sit alongside `src/` in `src-tauri/`.

## Tests

```bash
cargo test -p tauri-native-shell-example       # in-memory CRUD round-trip
```

Two tests:

- `note_crud_round_trip` — opens an in-memory SQLite, exercises create + find_many through `ModelDelegate`, asserts the JS-facing `JsNote` view matches.
- `macro_metadata_surface_is_distinct_per_module` — sanity check that the two `cratestack_schema` modules don't shadow each other.

## Verification status

| Layer | Status | Method |
|-------|--------|--------|
| Rust shell crate | ✅ | `cargo test` passes; `cargo check --workspace` clean |
| Frontend production build | ✅ | `pnpm --filter ./web run build` → 4 KB JS bundle (no wasm) |
| Tauri config discovery | ✅ | `pnpm exec tauri info` finds `src-tauri/tauri.conf.json` and reports correct `frontendDist` + `devUrl` |
| Live `pnpm tauri dev` window | ⚠ **build-only** — full developer-machine smoke deferred. The Rust commands and the renderer have been individually tested; only the full Tauri shell launch (which lands a window on your screen) hasn't been exercised here. Run it on your machine when you want the end-to-end signal |

## See also

- [`tauri-web`](../tauri-web) — the wasm-in-webview sibling. Use that when you want the renderer to be portable to a regular browser, or when filesystem access has to stay sandboxed.
- [`embedded-cli`](../embedded-cli) — the same `include_embedded_schema!` path, in a `clap`-driven CLI instead of a desktop window.
- [`react-nextjs-daisyui/napi`](../react-nextjs-daisyui/napi) — the Node-side equivalent: same dual-macro shape, exposed via N-API to a Next.js app.
