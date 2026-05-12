# cratestack-parser

Parser and semantic validator for `.cstack` schema files.

## Overview

`cratestack-parser` transforms `.cstack` source text into a typed `Schema` AST with full semantic validation. It's used internally by `include_schema!` and can be used directly for tooling.

## Installation

```toml
[dependencies]
cratestack-parser = "0.2"
```

## Usage

### Parse from string

```rust
use cratestack_parser::parse_schema;

let source = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

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

let schema = parse_schema(source)?;
```

### Parse from file

```rust
use cratestack_parser::parse_schema_file;

let schema = parse_schema_file("schema.cstack")?;
```

### Named schema (for error messages)

```rust
use cratestack_parser::parse_schema_named;

let schema = parse_schema_named("my-app/schema.cstack", source)?;
```

## Errors

`SchemaError` includes source spans for IDE integration:

```rust
use cratestack_parser::{parse_schema, SchemaError};

match parse_schema(source) {
    Ok(schema) => { /* use schema */ }
    Err(error) => {
        eprintln!("Error: {}", error.message);
        eprintln!("  at line {}", error.line);
        eprintln!("  span: {}..{}", error.span.start, error.span.end);
    }
}
```

## Supported Constructs

| Construct | Description |
|-----------|-------------|
| `datasource` | Database configuration (`provider`, `url`) |
| `auth` | Authentication context fields |
| `mixin` | Reusable field sets (`mixin AuditFields { ... }`) |
| `model` | Entity definition with fields and attributes |
| `type` | Named type declaration |
| `enum` | Enumerated values |
| `procedure` | Query or mutation operations |
| `@use(...)` | Mixin expansion on models |

## Field Attributes

See [Field Attributes](https://cratestack.dev/reference/field-attributes):

- `@id` - Primary key
- `@default(...)` - Server-side default
- `@readonly` - Excluded from inputs, visible in responses
- `@server_only` - Internal use, stripped from responses
- `@pii`, `@sensitive` - Audit redaction
- `@version` - Optimistic locking
- `@length`, `@range`, `@email`, `@regex`, `@uri`, `@iso4217` - Validators

## Model Attributes

- `@@allow(action, condition)` - Read/write policy
- `@@deny(action, condition)` - Block access
- `@@audit` - Enable audit logging
- `@@soft_delete` - Soft delete support
- `@@emit(created, updated, deleted)` - Event emission

## Integration

The parser is typically used through the `include_schema!` macro:

```rust
use cratestack::include_schema;

include_schema!("schema.cstack");
// Generates cratestack_schema module with all types and delegates
```

## License

MIT