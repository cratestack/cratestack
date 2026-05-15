# cratestack-studio

Admin and testing surface for CrateStack schemas. A single binary that loads
one or more `.cstack` files described in a `studio.toml` workspace file, opens
the configured database and/or service connections, and serves a local web
app for browsing and (eventually) editing records.

This crate replaces the per-project `cratestack-studio-generator` scaffold.
The generator now only powers `cratestack studio eject`, which copies the
binary's own sources out as a customizable Leptos+Axum project.

## Status

Phase 0 — skeleton only. `studio init` writes a starter `studio.toml`,
`studio run` binds to `127.0.0.1` and serves a stub page. No schema
introspection, no DB connections, no UI yet.

## Quickstart

```bash
cratestack studio init           # writes ./studio.toml
cratestack studio run            # binds 127.0.0.1:7878 by default
```

See the workspace plan for the v0 / v1 / v2 roadmap.
