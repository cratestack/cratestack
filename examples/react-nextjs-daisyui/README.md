# react-nextjs-daisyui

The full-stack CrateStack example: **React 19 + Next.js 16 + Tailwind 4 + DaisyUI 5 + Serwist PWA**, with three CrateStack surfaces wired into one app.

| Tab | Where the data lives | Macro |
|---|---|---|
| **Local (wasm)** | OPFS inside the browser, in a Dedicated Worker | `include_embedded_schema!` (wasm32 build) |
| **Server (napi)** | SQLite file owned by the Node.js process, accessed via napi-rs `.node` addon | `include_embedded_schema!` (host build) |
| **Remote** | An upstream CrateStack service, called over the typed HTTP client | `include_client_schema!` |

Plus an **offline-first sync engine**: writes go to the wasm/OPFS store first (instant, works offline). A background sync POSTs deltas to a Next.js Route Handler, which upserts them through the napi addon and returns server-authored deltas to merge back. Last-write-wins by `updatedAt`. A `localStorage`-backed cursor means a page refresh doesn't re-pull the whole server history.

## Layout

```
react-nextjs-daisyui/
├── pnpm-workspace.yaml         # the example is itself a pnpm monorepo
├── wasm/                       # browser-side cdylib (wasm32-unknown-unknown)
│   ├── Cargo.toml
│   ├── schema.cstack           # Note model
│   └── src/lib.rs              # include_embedded_schema! + wasm-bindgen exports
├── napi/                       # server-side N-API addon (`.node` binary)
│   ├── Cargo.toml
│   ├── build.rs                # napi_build::setup()
│   ├── package.json            # @napi-rs/cli build pipeline
│   ├── notes.cstack            # local Note model (mirrors wasm/)
│   ├── articles.cstack         # REMOTE service contract (Article)
│   └── src/lib.rs              # napi-derive functions, two include_*_schema! calls
└── web/                        # Next.js app
    ├── package.json
    ├── next.config.ts          # Serwist + COOP/COEP + serverExternalPackages
    ├── postcss.config.mjs      # @tailwindcss/postcss
    ├── public/
    │   ├── manifest.json       # PWA manifest
    │   ├── icon.svg
    │   └── pkg/                # populated by `pnpm run wasm:build`
    ├── app/
    │   ├── layout.tsx          # data-theme="emerald"
    │   ├── page.tsx            # thin shell hosting <App />
    │   ├── globals.css         # @import "tailwindcss"; @plugin "daisyui"
    │   ├── sw.ts               # Serwist service worker entry
    │   └── api/
    │       ├── addon.ts        # napi loader singleton
    │       ├── notes/route.ts        # GET/POST against the napi store
    │       ├── notes/sync/route.ts   # push/pull deltas for offline-first
    │       └── remote/route.ts       # fan-out to upstream over typed client
    └── components/
        ├── App.tsx             # tabbed shell + status badges + footer
        ├── LocalTab.tsx        # wasm-backed UI
        ├── ServerTab.tsx       # napi-backed UI
        ├── RemoteTab.tsx       # upstream-service UI
        ├── protocol.ts         # worker IPC types
        ├── worker.ts           # hosts the wasm runtime in a worker
        ├── useLocalNotes.ts    # typed client hook over postMessage RPC
        └── useSync.ts          # offline-first sync engine
```

## Prerequisites

- Rust + `wasm32-unknown-unknown` target + [wasm-pack](https://rustwasm.github.io/wasm-pack/)
- A wasm-capable clang. On macOS: `brew install llvm` (auto-detected by `examples/scripts/wasm-build.mjs`).
- Node.js 20+ and pnpm 9+
- `@napi-rs/cli` (installed via the napi/ package); first build compiles the addon for your host. Cross-compilation to other targets is configured in `napi/package.json#napi.targets` for CI.

## Run

```bash
cd examples/react-nextjs-daisyui

# Install both workspace packages (web + napi)
pnpm install

# Dev: builds wasm-dev + napi-debug, then starts next dev
pnpm --filter react-nextjs-daisyui-example run dev

# Production: builds wasm-release + napi-release + next build, then next start
pnpm --filter react-nextjs-daisyui-example run build
pnpm --filter react-nextjs-daisyui-example run start
```

Open `http://localhost:3000`. The Service Worker is registered only in production builds — `next dev` skips it so HMR isn't fighting Workbox.

## How offline-first works

```
┌─ browser ────────────────────────────────────────────────────┐
│                                                              │
│  React app                                                   │
│    ↓ user action (add note, mark done, ...)                  │
│  Worker (wasm/OPFS) ← writes here FIRST                      │
│    ↓ becomes "pending"                                       │
│  useSync()                                                   │
│    ↓ POST { cursor, pushes } when online                     │
└──────────────┬───────────────────────────────────────────────┘
               │
               ▼
┌─ Next.js Route Handler (/api/notes/sync) ────────────────────┐
│                                                              │
│  loadAddon().upsertNote(...) for each pushed row             │
│  loadAddon().notesSince(cursor) → newer server-side rows     │
│  reply { cursor: new, remote: rows }                         │
└──────────────┬───────────────────────────────────────────────┘
               │
               ▼
┌─ browser (continued) ────────────────────────────────────────┐
│  for row in reply.remote: upsertRemote(row)  ← LWW by ts     │
│  store reply.cursor in localStorage                          │
│  ✓ done                                                      │
└──────────────────────────────────────────────────────────────┘
```

Conflict resolution: last-write-wins by `updatedAt`. Both the wasm side and the napi side run the same rule, so a row that's *newer locally* survives the round-trip and gets re-pushed next time.

The UI surfaces three pieces of sync state:

- A header badge: `OPFS` vs `in-memory` (wasm storage path) and `online` vs `offline` (`navigator.onLine`)
- A `N pending` badge counting locally-touched rows not yet pushed
- A footer with last sync time, any error message, and a manual **Sync now** button

Triggers for auto-sync: initial mount when online, the browser `online` event, and a 30s interval gated on `document.visibilityState === 'visible' && navigator.onLine`.

## How the PWA works

[Serwist](https://serwist.pages.dev) wraps Workbox with a Next.js-aware build plugin. `next.config.ts` wires `app/sw.ts` as the source and `public/sw.js` as the output. The build inlines a precache manifest covering HTML/CSS/JS/wasm and registers runtime caches for fonts/images/api responses.

> **Webpack, not Turbopack.** `@serwist/next` is a webpack plugin and there is no Turbopack equivalent yet. Next.js 16 turns Turbopack on by default, so under a bare `next dev` the Serwist hooks would silently no-op and you'd get no service worker. The `dev` and `build` scripts in `web/package.json` pass `--webpack` explicitly to keep Serwist in the build graph. When Serwist ships a Turbopack adapter (or when a maintained alternative does), drop the flag.

Test it: `pnpm run build && pnpm run start`, then in DevTools → Application → Service Workers, you should see the worker registered and the wasm bundle pre-cached. Throw the browser into offline mode and reload — the Local tab still works (OPFS + cached wasm). The Server and Remote tabs surface the request failure cleanly.

To install: visit the page in Chrome/Edge and use **Install app**. The manifest at `/manifest.json` and the icon at `/icon.svg` cover the rest.

## How the napi addon works

The Rust crate compiles to a native dynamic library (`*.node` on every platform via `@napi-rs/cli`). Next.js loads it through `app/api/addon.ts`:

1. `require('react-nextjs-daisyui-napi')` — the `napi/index.js` shim picks the right `.node` for the host.
2. `addon.init(dbPath)` — opens (or creates) the SQLite file. Idempotent.
3. Route Handlers call `addon.listNotes()`, `addon.upsertNote(...)`, `addon.notesSince(cursor)`, and `addon.fetchRemoteArticles(baseUrl)`.

`fetchRemoteArticles` is the trusted HTTP-side path: it uses `include_client_schema!`-generated typed methods over `cratestack-client-rust`, with the `tokio_rt` napi feature giving us a real async runtime. The browser never sees the upstream URL or any outbound headers.

`next.config.ts` lists `react-nextjs-daisyui-napi` in `serverExternalPackages` so Next doesn't try to bundle the `.node` binary — it stays on disk and is loaded by `require()` at runtime.

## Tests

```bash
cargo test -p react-nextjs-daisyui-wasm     # wasm crate, native in-memory smoke
cargo test -p react-nextjs-daisyui-napi     # napi crate compiles, addon shape
```

End-to-end (Next.js + napi + worker + sync) is exercised manually via `pnpm run dev` — see "Run" above.

## See also

- [`react-vite-daisyui`](../react-vite-daisyui) — the same React/Daisy UI without the Next.js server (and without napi/sync)
- [`tauri-web`](../tauri-web) — same "trusted Rust hosts the HTTP client" pattern, but using a Tauri shell instead of Next.js
- [`embedded-browser-vite-pwa`](../embedded-browser-vite-pwa) — minimal Vite PWA pattern for reference
