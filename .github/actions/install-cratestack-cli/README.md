# install-cratestack-cli

Installs a prebuilt `cratestack-cli` binary into any GitHub Actions job without a Rust toolchain —
downloads the release asset matching the runner's OS/arch, verifies its SHA-256 checksum against the
published `.sha256` sidecar, and adds it to `PATH`.

## Usage

```yaml
- uses: cratestack/cratestack/.github/actions/install-cratestack-cli@main
  with:
    version: "0.7.15" # optional, defaults to "latest"
- run: cratestack --help
```

Pin to a specific ref (a released tag, or a commit SHA) instead of `@main` for reproducible CI —
`@main` always tracks the newest version of this action itself, independent of the `version` input
above (which pins the *binary* you get).

## Inputs

| Name | Required | Default | Description |
|---|---|---|---|
| `version` | no | `latest` | `cratestack-cli` version to install (no leading `v`). `latest` resolves the newest GitHub Release via the API. |
| `github-token` | no | `${{ github.token }}` | Token used only to resolve `latest` — avoids unauthenticated rate limiting on public runners. |

## Outputs

| Name | Description |
|---|---|
| `version` | The resolved version that was actually installed. |
| `cratestack-cli-path` | Absolute path to the installed binary. |

## Supported platforms

Same 5 targets `release-cli.yml` publishes binaries for: `x86_64-apple-darwin`,
`aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-pc-windows-msvc`. Any other OS/arch combination fails fast with a clear error rather than
silently falling through to a build-from-source path — install via `cargo install cratestack-cli`
or `cargo binstall cratestack-cli` instead on unsupported runners.

## Keeping this in sync

This action's asset-naming and target-triple logic is duplicated (by necessity — a composite
action's `run:` step can't `require()` a sibling JS file) from two other places that must all agree:

- `packages/cratestack-cli-npm/scripts/install.js` — the npm installer, same target mapping.
- `crates/cratestack-cli/Cargo.toml`'s `[package.metadata.binstall]` — the `cargo binstall` pkg-url
  template.

If the release asset layout in `.github/workflows/release-cli.yml`'s `Package archive` step ever
changes, update all three.
