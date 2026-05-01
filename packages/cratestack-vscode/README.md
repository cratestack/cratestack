# cratestack-vscode

This extension registers the `.cstack` language in VS Code and starts the standalone `cratestack-lsp` binary.

When the extension package includes a staged server binary under `server/<platform>/`, the extension prefers that bundled binary automatically and falls back to `cratestack.lsp.path` or `cratestack-lsp` on `PATH`.

Current editor features come from the language server plus the bundled grammar:

* diagnostics
* hover
* completion
* go-to-definition
* document symbols
* basic syntax highlighting

Current limitations:

* no rename support yet
* no references provider yet
* no formatting support yet
* no semantic tokens yet

## Settings

* `cratestack.lsp.path`: path to the `cratestack-lsp` binary. Defaults to `cratestack-lsp` on `PATH`.
* `cratestack.lsp.args`: additional arguments passed to the language server.

## Local Development

1. Build the language server from `cratestack/`:
   `cargo build -p cratestack-lsp`
2. Install extension dependencies in this folder:
   `pnpm install`
3. Point `cratestack.lsp.path` at the built binary if it is not already on `PATH`.
4. Run the package smoke tests:
   `pnpm run test:smoke`

## Bundle The Server For Release

1. Build the release server binary from `cratestack/`:
   `cargo build --release -p cratestack-lsp`
2. Stage it into the extension package:
   `pnpm run stage-server`
3. Package the VSIX:
   `pnpm run package:vsix`

The package command uses `vsce --no-dependencies` because this extension ships a thin wrapper plus the staged language-server binary rather than relying on npm-style production dependency discovery.

See `../../../cratestack-docs/docs/tooling/editor-tooling.md` for the fuller current-state writeup, testing coverage, and future improvements roadmap.
