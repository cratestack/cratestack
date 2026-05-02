# Contributing

CrateStack is early public-release software. Keep changes small, tested, and aligned with the schema-first framework boundary described in `README.md`.

Before opening a pull request:

1. Run `cargo fmt`.
2. Run `cargo check --workspace --all-targets --all-features`.
3. Run `cargo test --workspace --all-features`.
4. Run package-specific checks for editor or generated-client changes when applicable.

Do not commit generated build output, local database state, or registry tokens.
