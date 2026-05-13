# embedded-browser-webpack

Same demo as [`embedded-browser-vite`](../embedded-browser-vite), bundled with **Webpack 5** instead of Vite. The Rust source is **identical** to the Vite example (`include_embedded_schema!` + `wasm-bindgen` exports for a single `Note` model); only the JavaScript-side build configuration differs.

## Why we ship both

Vite and Webpack take meaningfully different positions on:

- **Worker resolution** — Vite has first-class `new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' })`; Webpack 5 supports the same syntax but you may also see codebases using `worker-loader` for legacy reasons.
- **Wasm loading** — Vite serves the `.wasm` from the `web/pkg/` directory directly; Webpack 5 with `experiments.asyncWebAssembly` imports `.wasm` as a true ES module dependency.
- **`wasm-pack` target** — Vite consumes `--target web` (standalone ES module with an `init()` call); Webpack consumes `--target bundler` (sync ES-module imports, bundler-orchestrated wasm fetch).

If your shop already runs Webpack 5, copy this example and skip the Vite version. Same Rust crate, same runtime story, same OPFS persistence inside a Dedicated Worker.

## Prerequisites

Identical to the Vite example — see [`../embedded-browser-vite/README.md`](../embedded-browser-vite/README.md#prerequisites).

In short: Rust + `wasm32-unknown-unknown` target + `wasm-pack` + a wasm-capable clang (`brew install llvm` on macOS, or distro `clang` 14+ on Linux) + Node.js 18+ and pnpm.

## Run

```bash
cd examples/embedded-browser-webpack/web
pnpm install
pnpm run dev
# Opens at http://localhost:5174
```

Production build:

```bash
pnpm run build
# Output in web/dist/
```

## Layout

Mirrors the Vite example almost line for line:

```
embedded-browser-webpack/
├── Cargo.toml                  # identical except for the package name
├── schema.cstack               # identical
├── src/lib.rs                  # identical
├── web/
│   ├── package.json            # webpack, ts-loader, html-webpack-plugin
│   ├── webpack.config.js       # ⇐ this is the bundler-specific bit
│   ├── tsconfig.json           # identical
│   ├── index.html              # identical (script tag injected by HtmlWebpackPlugin)
│   ├── src/main.ts             # one-line import path diff (no `.ts` ext)
│   ├── src/worker.ts           # imports the `--target bundler` pkg/, no init()
│   └── src/protocol.ts         # identical
└── README.md
```

The whole config delta is in `webpack.config.js` (~50 lines) plus the import-path cleanup in the TS files.

## Tests

`cargo test -p embedded-browser-webpack-example` runs the same in-memory smoke tests as the Vite version, on the native target.

## See Also

- [`embedded-browser-vite`](../embedded-browser-vite) — sibling example, same Rust crate, Vite instead of Webpack
- [Offline-First with Embedded SQLite](https://cratestack.dev/guides/offline-first-sqlite) — full guide on `cratestack-rusqlite` across native + browser
