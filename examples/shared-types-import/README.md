# shared-types-import

Two services, each with its own schema and its own SQLite database, that `import` one shared
`.cstack` file of types, enums and mixins. This is the reuse half of
[multi-file schemas](https://cratestack.dev/reference/multi-file-schemas) *(since 0.19.0)*; for
the other half, splitting one schema with `part`, see [`react-vite-swr`](../react-vite-swr).

## Layout

```text
shared/currency.cstack    enum Currency                       (imported by both)
shared/common.cstack      type Money, mixin OrderRecord       (imports currency.cstack)
billing/schema.cstack     model Invoice                       (imports common.cstack)
shipping/schema.cstack    enum ShipmentStatus, model Shipment (imports currency.cstack and common.cstack)
src/lib.rs                one `include_embedded_schema!` per service, each in its own module
```

`shared/common.cstack`:

```cstack
// Shapes both services use. This file imports `currency.cstack` itself, so a
// schema that imports this file sees `Currency` too: imports are transitive.

import "currency.cstack"

/// An amount in the currency's minor unit (cents; whole francs for XAF).
type Money {
  amountMinor Int
  currency Currency
}

// The fields every service row carries. `@use(OrderRecord)` copies them into
// the importer's model, so each service stores them in its own table.
mixin OrderRecord {
  orderId Int
  createdAt DateTime @default(now())
}
```

`billing/schema.cstack`:

```cstack
// The billing service's schema root. Billing owns its own database and its
// one table. The shared shapes come from `../shared/common.cstack`.
// `Currency` is not imported here: `common.cstack` imports it, and imports
// are transitive.

datasource db {
  provider = "sqlite"
  url = env("DATABASE_URL")
}

import "../shared/common.cstack"

model Invoice {
  @use(OrderRecord)
  id Int @id
  customer String
  totalMinor Int
  currency Currency
  paid Boolean
}
```

## What it shows

- **`import` shares shapes, not tables.** A shared file may declare only `type`, `enum` and
  `mixin`. Each service generates its own `Currency` and `Money` from the same file: the two
  `Currency` types are different Rust types with the same variants, and a billing `Money`
  decodes as a shipping `Money`. Each service stores `Currency` and the `OrderRecord` fields in
  **its own** table. A `model` in a shared file is refused (`tests/load_graph.rs`).
- **Imports are transitive.** Billing never imports `currency.cstack`; it gets `Currency` through
  `common.cstack`.
- **A file reached twice loads once.** Shipping imports `currency.cstack` directly and again
  through `common.cstack`. The loader keys files by canonical path, so that is one file.
- **A mixin from another file.** `@use(OrderRecord)` copies `orderId` and `createdAt` into
  `Invoice` and `Shipment`. `orderId` is the key that links one order across the two databases.
- **Two embedded schemas in one crate.** Every `include_*_schema!` call emits a module named
  `cratestack_schema`, so each call sits in its own module (`billing::cratestack_schema`,
  `shipping::cratestack_schema`).
- **Rebuilds.** Each service's `SCHEMA_FILES` lists the files it was built from in load order,
  each one an `include_str!`, so editing `shared/common.cstack` rebuilds both.

## Run

```bash
cargo run -p shared-types-import-example
```

```text
billing:  invoice 1 for order 42, total Money { amountMinor: 12500, currency: XAF }
shipping: shipment 7 for order 42 (Pending), insured Money { amountMinor: 12500, currency: XAF }
```

## Tests

```bash
cargo test -p shared-types-import-example
```

No database server needed: everything runs against in-memory SQLite, in plain `cargo test`.

## See the file graph

```bash
cargo run -p cratestack-cli -- check --schema examples/shared-types-import/shipping/schema.cstack --list-files
```

```text
schema OK: examples/shared-types-import/shipping/schema.cstack
files (3):
  root        examples/shared-types-import/shipping/schema.cstack
  import      examples/shared-types-import/shared/currency.cstack
  import      examples/shared-types-import/shared/common.cstack
```

Billing reaches `currency.cstack` only through `common.cstack`:

```text
schema OK: examples/shared-types-import/billing/schema.cstack
files (3):
  root        examples/shared-types-import/billing/schema.cstack
  import      examples/shared-types-import/shared/common.cstack
  import      examples/shared-types-import/shared/currency.cstack
```

A shared file is a schema on its own too: `check --schema examples/shared-types-import/shared/common.cstack` checks it.

## Notes

- Real services would be separate crates, each pointing its macro at its own root and
  importing `../../shared/...`. That works inside a Cargo workspace, but `cargo publish`
  packages only the crate directory, so a published crate must keep its schema files inside
  it.
- There is no `import ... as`: imported names join the importer's one namespace, and a name
  declared in two files is an error that names both.
- `OrderRecord.createdAt` is a create-time default (`@default(now())`). Nothing updates it later.
