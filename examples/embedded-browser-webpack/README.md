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

## Why this example pins `typescript: ^6.0.3`

Every other manifest in this repo declares `typescript: ^7.0.2`. This one is the deliberate exception,
and it is load-bearing — do not "align" it without replacing `ts-loader` first.

TypeScript 7 is the Go-native compiler (`tsgo`). Its npm package no longer exports the classic
JS compiler API: `ts.sys`, `ts.findConfigFile`, and `ts.createProgram` are all `undefined` at runtime.
`ts-loader` (9.6.2, the latest release) is built on exactly those entry points, so under TS 7 the
webpack build dies in `ts-loader`'s config resolution with:

```
TypeError: Cannot read properties of undefined (reading 'fileExists')
    at findConfigFile (node_modules/ts-loader/dist/config.js:105:30)
```

Note that `pnpm run typecheck` (`tsc --noEmit`) **passes** under TS 7 — the sources themselves are
TS 7-clean. It is only the bundler integration that breaks, so a green typecheck is not sufficient
evidence that the bump is safe; run `pnpm run build` too. `ts-loader` also declares
`peerDependencies.typescript: "*"`, so pnpm emits no peer warning to catch this for you.

Lifting the pin needs one of: a `ts-loader` release that targets the TS 7 API, or switching this
example to a TS 7-compatible transform (e.g. `swc-loader`/`babel-loader` for emit plus `tsc` for
typechecking). Until then `^6.0.3` stays.

## Tests

`cargo test -p embedded-browser-webpack-example` runs the same in-memory smoke tests as the Vite version, on the native target.

## See Also

- [`embedded-browser-vite`](../embedded-browser-vite) — sibling example, same Rust crate, Vite instead of Webpack
- [Offline-First with Embedded SQLite](https://cratestack.dev/guides/offline-first-sqlite) — full guide on `cratestack-rusqlite` across native + browser
