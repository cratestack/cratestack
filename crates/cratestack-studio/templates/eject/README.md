# cratestack-studio UI — ejected

This directory was produced by `cratestack studio eject` and contains
a writable copy of CrateStack Studio's Leptos+Trunk UI. You can fork
it freely: customize the styling, swap components, wire in
organization-specific actions ahead of the upstream's schedule.

## Build prerequisites

```bash
cargo install trunk
rustup target add wasm32-unknown-unknown
```

## Local development

You'll need two terminals.

```bash
# Terminal 1 — Studio backend (run from your studio.toml directory).
cratestack studio run            # binds 127.0.0.1:7878

# Terminal 2 — UI dev server.
trunk serve                      # serves the SPA on 127.0.0.1:8080
```

`Trunk.toml` already proxies `/api/*` to the backend, so the browser
sees a single origin.

## Release build

```bash
trunk build --release
```

This produces `dist/` — a static `index.html` plus the WASM bundle
and assets. Drop it behind any static-file server.

## Relationship to upstream

This eject is a **point-in-time snapshot** of the framework's UI
sources. There is no automated upgrade path back to upstream: when a
new framework version ships meaningful UI changes, you'll need to
merge or re-eject manually. Most users only need to eject when they
want fine-grained control over the visual style or want to wire
proprietary actions; if your changes are upstream-friendly, please
open a PR to `cratestack/cratestack` instead.

## API contract

The ejected UI consumes Studio's stable read API: targets list,
schema introspection, paginated records, single record by PK,
relation follow, and "copy Rust query" snippet. See the framework's
docs at <https://cratestack.dev/studio/read-api> for the endpoint
shapes and error envelope.
