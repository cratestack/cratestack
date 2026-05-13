# tauri-web

Desktop app built with **Tauri 2** that wires up two CrateStack surfaces under one shell:

- **Webview side** — `include_embedded_schema!` compiled to `wasm32-unknown-unknown` and loaded inside the Tauri webview's Dedicated Worker. Local `Note` CRUD persists via OPFS, exactly like [`embedded-browser-vite`](../embedded-browser-vite).
- **Native shell side** — `include_client_schema!` against a remote CrateStack service. The shell exposes Tauri commands; the webview calls them via `@tauri-apps/api/core`. The renderer never makes a `fetch` itself.

This is the "thick desktop client" pattern: trusted operations (HTTP with cert pinning, secret reads, OS-keychain access) stay in native Rust; the renderer handles UI and the local data layer. Phase C's `tauri-native` example will swap the embedded webview wasm for direct Tauri IPC-driven Rust delegates, but the HTTP-side architecture remains the same.

## Layout

```
tauri-web/
├── Cargo.toml                 # WASM cdylib for the webview (embedded ORM)
├── schema.cstack              # Note schema (local data)
├── src/lib.rs                 # include_embedded_schema! + wasm-bindgen exports
├── src-tauri/
│   ├── Cargo.toml             # native Tauri binary
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── icons/icon.png
│   ├── schema.cstack          # Article schema (REMOTE service contract)
│   └── src/
│       ├── lib.rs             # include_client_schema! + Tauri commands
│       └── main.rs            # thin entry: tauri_web_shell_example_lib::run()
├── web/
│   ├── package.json           # vite + @tauri-apps/{api,cli}
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── index.html             # two panels: local notes + remote articles
│   └── src/
│       ├── main.ts            # spawns worker, calls invoke() for remote
│       ├── worker.ts          # hosts the wasm runtime
│       └── protocol.ts        # worker IPC types
└── README.md
```

## Prerequisites

- Everything from [`embedded-browser-vite`](../embedded-browser-vite#prerequisites) — Rust + `wasm32-unknown-unknown` target + `wasm-pack` + a wasm-capable clang.
- **Tauri 2 system deps** for your platform — see [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/). On macOS the Xcode CLI tools cover it; on Linux you need GTK + WebKit GTK; on Windows, MSVC + WebView2.
- **Node.js 18+ and pnpm**.

## Run

```bash
cd examples/tauri-web/web
pnpm install

# Run the desktop app (Vite dev server + Tauri shell, hot-reloading both):
pnpm tauri dev

# Or package a release build:
pnpm tauri build
```

Internally `pnpm tauri dev` runs:

1. `pnpm run dev` in `web/` (Vite + the `wasm:build:dev` prelude)
2. `cargo run -p tauri-web-shell-example` once the dev server is up
3. Opens a native window pointed at `http://localhost:5173`

The window shows two stacked panels:

- **Local notes** — exactly the Vite/OPFS demo, full offline CRUD inside the webview.
- **Remote articles** — paste a CrateStack service URL into the input; the **Tauri Rust side** issues the HTTP request via the typed `include_client_schema!`-generated client and returns the rows back to the renderer.

## How the two halves talk

```
┌─ webview (wasm + Vite) ────────────────────────────┐
│ main.ts                                            │
│   new Worker(./worker.ts)              ┐           │
│   worker.postMessage({ kind: 'add' })  │ wasm CRUD │
│   worker.onmessage → NoteView          ┘           │
│                                                    │
│   invoke('fetch_remote_articles', { baseUrl })     │
│       │                                            │
└───────┼────────────────────────────────────────────┘
        │  Tauri IPC (postMessage-shaped)
        ▼
┌─ native Tauri shell (cratestack + reqwest) ────────┐
│ #[tauri::command] async fn fetch_remote_articles { │
│     let client = cratestack_schema::client::Client │
│         ::new(CratestackClient::new(url, CborCodec));
│     client.articles().list(...).await              │
│ }                                                  │
└────────────────────────────────────────────────────┘
```

## Why split it this way

The webview can't run `cratestack-client-rust` today — it depends on `tokio` features that don't compile to `wasm32-unknown-unknown` (and reqwest's wasm path uses browser `fetch` anyway, losing native HTTP capabilities). Pushing the HTTP client to the Rust shell:

- Keeps cookies, mTLS, cert pinning, proxy config, and OS-keychain reads on the trusted side.
- Lets the shell hold long-lived secrets that the renderer never sees.
- Avoids CORS — the Rust process makes direct HTTP requests.
- Sets up the natural seam for Phase C's `tauri-native` example, where the **embedded** side also moves to native Rust delegates over Tauri IPC.

## Tests

```bash
cargo test -p tauri-web-wasm-example       # wasm crate in-memory smoke test
cargo test -p tauri-web-shell-example      # native shell crate (offline checks)
```

Full end-to-end (Vite dev server + Tauri window) is exercised manually via `pnpm tauri dev` — see "Run" above.

## See Also

- [`embedded-browser-vite`](../embedded-browser-vite) — the wasm side without the Tauri shell
- [`client-stub-rust`](../client-stub-rust) — the native `include_client_schema!` shape used in the shell, standalone
- [Phase C — Mobile (Flutter, Expo, tauri-native)](../README.md) — coming up next
