# @cratestack/cli

npm wrapper around [`cratestack-cli`](https://crates.io/crates/cratestack-cli) — the CrateStack
`.cstack` schema validator and client/Studio code generator. Installing this package downloads the
prebuilt `cratestack` binary matching your platform from the matching
[GitHub Release](https://github.com/cratestack/cratestack/releases); no Rust toolchain required.

## Install

```bash
npm install --global @cratestack/cli
```

Or run without installing:

```bash
npx @cratestack/cli --help
```

## Supported platforms

macOS (x64, arm64), Linux (x64, arm64), Windows (x64). See
[cratestack-cli](https://github.com/cratestack/cratestack/tree/main/crates/cratestack-cli) for the
full command reference (`check`, `generate-dart`, `generate-typescript`, `studio`, `print-ir`).

## How it works

The `postinstall` script (`scripts/install.js`) detects your OS/architecture, downloads the
matching release archive plus its `.sha256` checksum from GitHub Releases, verifies the checksum,
and extracts the `cratestack` binary into `bin/`. The `cratestack` command then execs that binary.

Environment variables:

- `CRATESTACK_CLI_SKIP_DOWNLOAD=1` — skip the postinstall download (e.g. offline/vendored installs
  that provide the binary another way).
- `CRATESTACK_CLI_BINARY_PATH=/path/to/cratestack` — run a specific binary instead of the one
  downloaded at install time.

If the download fails or your platform isn't yet supported, fall back to:

```bash
cargo binstall cratestack-cli
# or
cargo install cratestack-cli
```

## License

MIT
