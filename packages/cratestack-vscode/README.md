# cratestack-vscode

This extension registers the `.cstack` language in VS Code and starts the standalone `cratestack-lsp` binary.

When the extension package includes a staged server binary under `server/<platform>/`, the extension prefers that bundled binary automatically and falls back to `cratestack.lsp.path` or `cratestack-lsp` on `PATH`.

Current editor features come from the language server plus the bundled grammar:

* diagnostics
* hover
* completion
* go-to-definition
* find all references
* document highlight
* document symbols
* semantic tokens
* basic syntax highlighting

Go-to-definition (Ctrl+Click / F12) resolves every reference site in a schema:
a field's type to its `model`, `type`, `enum` or `mixin` declaration; a
procedure argument or return type, including inside `Page<T>`; `@use(Mixin)` to
the mixin; and both halves of `@relation(fields: [...], references: [...])` —
`fields` to the local column, `references` to the column on the related model.

Find all references (Shift+F12) works from either end of a relation: asking for
references of a model's `id` surfaces the `references: [id]` sites that point at
it. Field references are qualified by their owning declaration, so `User.id` and
`Post.id` are tracked as distinct symbols rather than matched by name.

Semantic tokens layer on top of the TextMate grammar rather than replacing it.
The grammar keeps colouring keywords, strings and comments — instantly, before
the server starts — and the server then re-colours identifiers using the
resolved schema, which is the part regexes cannot do: `String` (builtin), `User`
(a model), `Role` (an enum) and `Timestamps` (a mixin) are all bare capitalised
words to a grammar, and four different things to the server.

Current limitations:

* no rename support yet
* no formatting support yet
* one diagnostic per file — the parser stops at the first error
* navigation goes quiet while a file has a syntax error, since the schema fails
  to parse and there is no last-known-good fallback

## Settings

* `cratestack.lsp.path`: path to the `cratestack-lsp` binary. Defaults to `cratestack-lsp` on `PATH`.
* `cratestack.lsp.args`: additional arguments passed to the language server.

## Local Development

1. Build the language server from `cratestack/`:
   `cargo build -p cratestack-lsp`
2. Install extension dependencies in this folder:
   `pnpm install`
3. Point `cratestack.lsp.path` at the built binary if it is not already on `PATH`.
4. Bundle the extension and run its tests:
   `pnpm test`

`pnpm test` bundles first because the entry point that actually ships is the
bundle, not `extension.js` — see below. `pnpm run test:vscode-smoke` additionally
drives a real VS Code instance and needs a built `cratestack-lsp`; it is not part
of the CI run.

## Bundle The Server For Release

1. Build the release server binary from `cratestack/`:
   `cargo build --release -p cratestack-lsp`
2. Stage it into the extension package:
   `pnpm run stage-server`
3. Package the VSIX:
   `pnpm run package:vsix`

`main` points at `dist/extension.js`, an esbuild bundle produced by
`scripts/build.mjs`. vsce runs that build itself through the `vscode:prepublish`
script, so packaging and publishing need no separate build step.

Bundling is load-bearing rather than a size optimization. The packaging commands
pass `vsce --no-dependencies` (pnpm's symlinked `node_modules` defeats vsce's
npm-style production dependency discovery) and `.vscodeignore` excludes
`node_modules/**`, so an unbundled entry point resolves nothing at activation
time: every VSIX released before this bundling step existed failed to activate
with `Cannot find module 'vscode-languageclient/node'`, leaving users with the
TextMate grammar and no language server. `test/bundle.test.js` loads the built
entry point from a directory containing nothing but a `vscode` stub, which is the
packaged environment, and is what keeps that from regressing.

See `cratestack-docs/docs/tooling/editor-tooling.md` in the standalone docs repository for the fuller current-state writeup, testing coverage, and future improvements roadmap.
