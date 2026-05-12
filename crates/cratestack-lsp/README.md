# cratestack-lsp

Language Server Protocol implementation for `.cstack` schema files.

## Overview

`cratestack-lsp` is a `tower-lsp` binary that surfaces parse and semantic diagnostics from `cratestack-parser`, plus a few editor conveniences. It is the language server bundled by `packages/cratestack-vscode`.

## Installation

```bash
cargo install cratestack-lsp --version 0.2.2
```

Or build from the workspace:

```bash
cargo build --release -p cratestack-lsp
```

The binary speaks LSP over stdio.

## Capabilities

| Capability                | Status              |
|---------------------------|---------------------|
| Text document sync        | Full                |
| `textDocument/hover`      | Supported           |
| `textDocument/completion` | Supported (defaults)|
| `textDocument/definition` | Supported           |
| `textDocument/documentSymbol` | Supported       |
| `textDocument/publishDiagnostics` | Supported (on open and change) |

Find-references, rename, and code-action capabilities are not implemented today.

## Editor Integration

### VS Code

Use `packages/cratestack-vscode`, which bundles this binary.

### Neovim

```lua
local lspconfig = require('lspconfig')
local configs = require('lspconfig.configs')

if not configs.cratestack then
  configs.cratestack = {
    default_config = {
      cmd = { 'cratestack-lsp' },
      filetypes = { 'cstack' },
      root_dir = lspconfig.util.root_pattern('.git', '*.cstack'),
    },
  }
end

lspconfig.cratestack.setup{}
```

### Emacs

```elisp
(use-package lsp-mode
  :config
  (add-to-list 'lsp-language-id-configuration '(cstack-mode . "cstack"))
  (lsp-register-client
   (make-lsp-client
    :new-connection (lsp-stdio-connection '("cratestack-lsp"))
    :major-modes '(cstack-mode)
    :server-id 'cratestack)))
```

## See Also

- [Editor Tooling](https://cratestack.dev/tooling/editor-tooling)
- `cratestack-parser` — underlying parser and semantic checker

## License

MIT
