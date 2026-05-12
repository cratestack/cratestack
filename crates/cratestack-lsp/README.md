# cratestack-lsp

Language Server Protocol implementation for `.cstack` schema files.

## Overview

`cratestack-lsp` provides IDE support for CrateStack schema files, including diagnostics, completions, hover documentation, and navigation.

## Installation

The LSP is typically bundled with editor extensions:

```bash
cargo install cratestack-lsp
```

## Features

### Diagnostics

Real-time error detection for:
- Syntax errors
- Undefined references
- Invalid attribute values
- Missing required fields
- Circular dependencies

### Completions

Context-aware suggestions for:
- Model and type names
- Field names
- Attribute names and values
- Relation references
- Enum values

### Hover

Documentation on hover for:
- Model definitions
- Field types
- Attributes and their meanings

### Navigation

- Go to definition
- Find references
- Document symbols outline

## Editor Integration

### VS Code

Install the `cratestack-vscode` extension which bundles this LSP.

### Neovim

```lua
local configs = require('lspconfig.configs')

configs.cratestack = {
  default_config = {
    cmd = {'cratestack-lsp'},
    filetypes = {'cstack'},
    root_dir = lspconfig.util.root_pattern('.git', '*.cstack'),
  },
}

require('lspconfig').cratestack.setup{}
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

## Protocol Support

| Capability | Status |
|------------|--------|
| Diagnostics | ✓ |
| Completion | ✓ |
| Hover | ✓ |
| Go to Definition | ✓ |
| Find References | ✓ |
| Document Symbols | ✓ |
| Rename | Partial |

## File Type

The LSP handles `.cstack` files:

```
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model User {
  id String @id
  email String @unique
  name String?
}
```

## See Also

- [Editor Tooling](https://cratestack.dev/tooling/editor-tooling)
- `cratestack-parser` - Underlying parser

## License

MIT