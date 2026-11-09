# cratestack-parser

Parser and semantic checker for `.cstack` schema files.

## Overview

`cratestack-parser` turns `.cstack` source text into a `cratestack_core::Schema` AST and runs semantic validation. The `include_server_schema!` / `include_embedded_schema!` / `include_client_schema!` proc-macros call this crate at compile time; the LSP and CLI call it at runtime.

## Installation

```toml
[dependencies]
cratestack-parser = "0.19"
```

## Usage

### Parse from a string

```rust
use cratestack_parser::parse_schema_named;

let source = r#"
auth Principal {
  id String
  role String?
}

model Post {
  id String @id
  title String
  published Boolean @default(false)
  authorId String

  @@allow("read", auth() != null)
}
"#;

let schema = parse_schema_named("schema.cstack", source)?;
```

### Parse from a file

```rust
use cratestack_parser::parse_schema_file;

let schema = parse_schema_file("schema.cstack")?;
```

### Named source for diagnostics

```rust
use cratestack_parser::parse_schema_named;

let schema = parse_schema_named("my-app/schema.cstack", source)?;
```

## Errors

`SchemaError` carries a source span, a line number, and the file it came from, plus an
ariadne-rendered report:

```rust
use cratestack_parser::{SchemaError, parse_schema_named};

match parse_schema_named("schema.cstack", source) {
    Ok(schema) => { /* ... */ }
    Err(error) => {
        eprintln!("{}", error.message());
        eprintln!("line {}", error.line());
        eprintln!("file {}", error.file());
        let span = error.span();
        eprintln!("bytes {}..{}", span.start, span.end);
        eprintln!("{}", error.render());
    }
}
```

## Supported Constructs

| Construct       | Description                                                |
|-----------------|------------------------------------------------------------|
| `datasource`    | Database config (`provider = "postgresql"` or `"sqlite"`)  |
| `auth`          | Single auth block declaring principal fields               |
| `mixin`         | Reusable field set, applied via `@use(...)` on a model     |
| `model`         | Entity with fields, relations, and policy attributes       |
| `type`          | Named record type (`@computed` resolver fields supported)  |
| `enum`          | Identifier variants; optionally explicit ordinals and `@@reserved` (since 0.17.0) |
| `procedure`     | `procedure` / `mutation procedure` / `subscription procedure` with typed args/return |
| `view`          | Read-only projection over one or more models (ADR-0003), optionally `@@materialized` |
| `query`         | Declarative parameterized custom SQL, policy-gated (since 0.11.0) |
| `topic`         | A subscription topic (publish/subscribe, no model behind it) |
| `transport`     | `transport rpc` (REST is the default)                      |
| `extension`     | Opt-in capability blocks: `pgvector`, `postgis`, `rate_limit`, `websocket`, `webtransport` |
| `mcp`           | `mcp { }` with `expose = [tools, resources]`, and `name = "..."` when resources are exposed; typed and validated |
| `part` / `part of` / `import` | Multi-file schemas: split one schema, or share `type`/`enum`/`mixin` files (since 0.19.0; `load_schema`) |

See the root README for the canonical capability matrix.

## Field and Model Attributes

See [Field Attributes](https://cratestack.dev/reference/field-attributes) for the full list. Common attributes:

- `@id`, `@unique`, `@relation(...)`, `@default(...)`
- `@readonly`, `@server_only`, `@pii`, `@sensitive`
- `@version` (optimistic locking)
- `@length`, `@range`, `@email`, `@regex`, `@uri`, `@iso4217` (validators: enforced on a model's create and
  update inputs, on the `type`s and `model`s a procedure takes as arguments, and on a `@computed`
  field's `?computedParams=`; refused on a `type` no client input reaches)
- `@db_enforce` (the validators `@range`, `@length`, `@iso4217` as a `CHECK`; refused beside none of them)
- `@@allow(action, expr)`, `@@deny(action, expr)`
- `@@audit`, `@@soft_delete`
- `@@emit(created, updated, deleted)`

A field accepts only the attributes its declaration kind lists (`model`,
`view`, `mixin`, `type`, `auth`), and any other name is an error:
`cratestack_parser::field_attribute_names(host)` returns a kind's list, and
the editor completions read the same one.

## See Also

- [Mixins reference](https://cratestack.dev/reference/mixins)
- [Field Attributes](https://cratestack.dev/reference/field-attributes)
- [Validators guide](https://cratestack.dev/guides/validators)
- `cratestack-macros` — the proc-macro that drives this parser at compile time
- `cratestack-lsp` — LSP frontend over the same parser

## License

MIT
