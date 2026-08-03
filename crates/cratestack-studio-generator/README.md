# cratestack-studio-generator

**Transitional shim.** The 0.3 line shipped a multi-crate Leptos+Axum studio
scaffold rendered from Jinja templates. That has been removed.

The replacement is [`cratestack-studio`](../cratestack-studio): a single
binary served from a `studio.toml` workspace file. Browse with
`cratestack studio run`.

This crate now only re-exports `cratestack-studio`'s `eject()` (plus
`EjectOptions`/`EjectError`/`EjectReport`) so the CLI's existing import
surface keeps working. `eject()` scaffolds a standalone, runnable Studio
binary crate; pass `with_ui: true` in `EjectOptions` to also unpack the
Leptos+Trunk UI sources for front-end customization. New code should
depend on `cratestack-studio` directly — see its
[README](../cratestack-studio/README.md#eject).

If you depended on `generate_package`, `StudioGeneratorConfig`,
`StudioGeneratorContext`, `StudioProfile`, `GeneratedStudioFile`, or
`GeneratedStudioPackage` in 0.3.x — these are gone. Migration: use
`cratestack studio init` to seed a `studio.toml`, then `cratestack studio
run`.

## License

MIT
