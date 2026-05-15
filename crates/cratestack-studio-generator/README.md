# cratestack-studio-generator

**Transitional shim.** The 0.3 line shipped a multi-crate Leptos+Axum studio
scaffold rendered from Jinja templates. That has been removed.

The replacement is [`cratestack-studio`](../cratestack-studio): a single
binary served from a `studio.toml` workspace file. Browse with
`cratestack studio run`.

In Phase 2 of the rewrite this crate gains a thin `eject` path that copies
`cratestack-studio`'s own sources into an output directory, so users can
fork the UI without losing a clean upgrade story. Until then the public
`eject()` function returns `NotImplemented`.

If you depended on `generate_package`, `StudioGeneratorConfig`,
`StudioGeneratorContext`, `StudioProfile`, `GeneratedStudioFile`, or
`GeneratedStudioPackage` in 0.3.x — these are gone. Migration: use
`cratestack studio init` to seed a `studio.toml`, then `cratestack studio
run`.

## License

MIT
