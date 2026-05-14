# Release Process

CrateStack publishes through the common public Rust and editor channels:

* Rust crates: crates.io and docs.rs
* CLI binaries and release notes: GitHub Releases
* VS Code extension: Visual Studio Marketplace and Open VSX
* Documentation site: Mintlify or equivalent static docs hosting from `docs-site/`

## Prerequisites

Required credentials are intentionally read from the environment:

* `CARGO_REGISTRY_TOKEN` for crates.io
* `VSCE_PAT` for Visual Studio Marketplace
* `OVSX_PAT` for Open VSX
* GitHub permissions to push tags and create releases

## Validate

Run from the repository root:

```sh
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
cargo package -p cratestack-core --allow-dirty --no-verify
```

On the first public release, sibling crates that depend on `cratestack-core` and each other cannot all complete `cargo package` against crates.io until their dependencies have been published. After the first ordered publish has populated crates.io, run package dry-runs across the full workspace before each later release:

```sh
for package in \
  cratestack-core \
  cratestack-policy \
  cratestack-parser \
  cratestack-codec-cbor \
  cratestack-codec-json \
  cratestack-axum \
  cratestack-sqlx \
  cratestack-client-rust \
  cratestack-client-dart \
  cratestack-client-typescript \
  cratestack-client-flutter \
  cratestack-client-store-sqlite \
  cratestack-client-store-redis \
  cratestack-studio-generator \
  cratestack-macros \
  cratestack \
  cratestack-lsp \
  cratestack-cli; do
  cargo package -p "$package" --allow-dirty --no-verify
done
```

Run from `packages/cratestack-vscode`:

```sh
pnpm install
pnpm run test:smoke
pnpm run package:vsix
```

## Publish Rust Crates

Publish leaf crates before crates that depend on them:

```sh
cargo publish -p cratestack-core
cargo publish -p cratestack-policy
cargo publish -p cratestack-parser
cargo publish -p cratestack-codec-cbor
cargo publish -p cratestack-codec-json
cargo publish -p cratestack-axum
cargo publish -p cratestack-sql
cargo publish -p cratestack-sqlx
cargo publish -p cratestack-client-rust
cargo publish -p cratestack-client-dart
cargo publish -p cratestack-client-typescript
cargo publish -p cratestack-client-flutter
cargo publish -p cratestack-client-store-sqlite
cargo publish -p cratestack-client-store-redis
cargo publish -p cratestack-studio-generator
cargo publish -p cratestack-migrate
cargo publish -p cratestack-macros
cargo publish -p cratestack-rusqlite
cargo publish -p cratestack
cargo publish -p cratestack-lsp
cargo publish -p cratestack-redis
cargo publish -p cratestack-cli
```

If crates.io index propagation causes a dependency lookup race, wait briefly and retry the next crate.

## Publish Editor Extension

Build and stage the language server first:

```sh
cargo build --release -p cratestack-lsp
cd packages/cratestack-vscode
pnpm run package:vsix
pnpm run publish:vscode-marketplace
pnpm run publish:open-vsx
```

## Tag

```sh
git tag v0.1.0
git push origin v0.1.0
```
