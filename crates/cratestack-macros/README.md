# cratestack-macros

Procedural macros for compile-time schema processing.

## Overview

`cratestack-macros` provides the `include_schema!` and `include_client_macro!` macros that process `.cstack` files at compile time and generate typed Rust code.

## Installation

The macros are re-exported through the main crate:

```toml
[dependencies]
cratestack = "0.2"
```

## include_schema!

Generate delegates, routes, and client types from a schema:

```rust
use cratestack::include_schema;

include_schema!("schema.cstack");

// Generated:
// - cratestack_schema module with Model types, CreateInput, UpdateInput
// - cratestack_schema::CrateStack runtime builder
// - ModelDelegate implementations for each model
// - cratestack_schema::axum::model_router for Axum
// - cratestack_schema::client::Client for HTTP client
```

### What gets generated

1. **Model structs**: `User`, `Post`, etc. with `Serialize`/`Deserialize`
2. **Input structs**: `CreateUserInput`, `UpdateUserInput`
3. **Selection builders**: `User::select()`, `Post::include_selection()`
4. **Filter builders**: `post::id().eq(...)`, `post::author().email().like(...)`
5. **Delegates**: `ModelDelegate` for CRUD operations
6. **Axum router**: `cratestack_schema::axum::model_router(...)`
7. **Client types**: For `cratestack-client-rust`

## include_client_macro!

Generate only the client surface (useful for separate client packages):

```rust
use cratestack::include_client_macro;

include_client_macro!("../schemas/api.cstack");

// Generates a minimal cratestack_schema module with:
// - Model types
// - Client surface for HTTP requests
// - No server-side delegates
```

## Generated Selectors

Field selectors for partial updates and projections:

```rust
// Projection
let selection = User::select()
    .id()
    .email()
    .include_posts(
        Post::include_selection()
            .id()
            .title()
    );

// Filter expression
let filter = post::published().is_true()
    .and(post::author().email().eq("owner@example.com"));

// Ordering
let order = post::createdAt().desc();
```

## Decimal Backend

The macros respect workspace features:

```toml
[features]
default = ["decimal-rust-decimal"]
decimal-bigdecimal = ["cratestack-core/decimal-bigdecimal"]
```

Generated code uses `cratestack::Decimal` which resolves to the selected backend.

## License

MIT