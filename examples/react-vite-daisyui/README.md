# react-vite-daisyui

React 19 + Vite 8 + Tailwind 4 + DaisyUI 5 SPA driving a CrateStack embedded SQLite schema compiled to `wasm32-unknown-unknown` and persisted via OPFS.

The Rust crate is byte-for-byte the same shape as [`embedded-browser-vite`](../embedded-browser-vite) — only the renderer differs. This is the point: the embedded path doesn't care about your UI framework, and slotting in DaisyUI's component library on top costs you nothing on the data plane.

## Layout

```
react-vite-daisyui/
├── Cargo.toml                 # wasm cdylib crate
├── schema.cstack              # Note model
├── src/lib.rs                 # include_embedded_schema! + wasm-bindgen exports
└── web/
    ├── package.json           # vite + react + tailwind + daisyui
    ├── vite.config.ts         # tailwind plugin + COOP/COEP headers
    ├── tsconfig.json
    ├── index.html             # data-theme="emerald"
    └── src/
        ├── main.tsx           # React root
        ├── App.tsx            # DaisyUI-styled UI
        ├── useNotesWorker.ts  # typed worker client hook
        ├── worker.ts          # hosts the wasm runtime
        ├── protocol.ts        # worker IPC types
        └── styles.css         # @import "tailwindcss"; @plugin "daisyui"
```

## Prerequisites

- Rust + `wasm32-unknown-unknown` target + [wasm-pack](https://rustwasm.github.io/wasm-pack/)
- A wasm-capable clang. On macOS: `brew install llvm` (the shared `examples/scripts/wasm-build.mjs` helper auto-detects it).
- Node.js 20+ and pnpm.

## Run

```bash
cd examples/react-vite-daisyui/web
pnpm install
pnpm run dev
```

Open `http://localhost:5173`.

`pnpm run dev` chains:

1. `pnpm run wasm:build:dev` — `wasm-pack build --target web --out-dir web/pkg --dev` (with the wasm-capable `CC` exported)
2. `vite` — Vite 8 dev server with the Tailwind 4 plugin and React Fast Refresh

The Tailwind 4 + DaisyUI 5 setup uses **CSS-first config**: there's no `tailwind.config.js`. Configuration lives directly in `web/src/styles.css`:

```css
@import "tailwindcss";
@plugin "daisyui" {
  themes: emerald --default, dark;
}
```

## What's in the UI

- A title/body/pin form that writes through to OPFS via the wasm runtime
- A list with "hide completed" toggle, per-row "Done" / "Delete" actions
- A header badge showing whether OPFS persistence succeeded or whether the runtime fell back to in-memory storage

All UI components are DaisyUI primitives — `btn`, `card`, `input`, `badge`, `alert`, `navbar`, `loading`, `toggle`, `checkbox`, `textarea`.

## Tests

```bash
cargo test -p react-vite-daisyui-example
```

Native in-memory smoke test exercises the same `ModelDelegate` paths the wasm worker calls.

## See also

- [`embedded-browser-vite`](../embedded-browser-vite) — the same Rust surface with vanilla TypeScript
- [`react-nextjs-daisyui`](../react-nextjs-daisyui) — the React shape with three surfaces (wasm + napi-rs `.node` + remote) and offline-first sync
