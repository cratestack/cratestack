# @cratestack/refine

A [refine.dev](https://refine.dev) `DataProvider` over CrateStack's generated TypeScript client —
**REST** (`createCratestackDataProvider`) or **RPC** (`createCratestackRpcDataProvider`, for
`transport rpc` schemas). Wires refine's fixed `getList`/`getOne`/`getMany`/`create`/`update`/
`deleteOne` surface to a generated model class (`client.widgets`, `client.ledgers`, …), mapping
refine's filter operators, pagination, primary keys, and `@version` optimistic-locking conflicts
onto the server's real contract — the same one [cratestack-studio](https://cratestack.dev)
deliberately bypasses. `cratestack-studio` talks to `[target.db]` directly and skips `@@allow`; a
refine app built on this package goes through the generated API and inherits policy, validation,
`@version` concurrency, and audit. That's the reason this package exists (cratestack#571): refine
is the safe end-user admin surface, Studio is the sysadmin one.

Both transports share this same README except where a section says otherwise — the two providers'
`getList`/`getOne`/`getMany`/`create`/`update`/`deleteOne`/`createMany`/`updateMany`/`deleteMany`/
`custom` behavior, filter-operator table, pagination semantics, primary-key handling, and `@version`
optimistic-locking guarantee are identical; only the wire calls underneath differ. See
[RPC transport](#rpc-transport) below for what's RPC-specific, and [Scope](#scope) for what neither
provider does.

## A runtime package with a generated manifest

A `DataProvider` needs four things per resource — the generated API object itself, the model's
primary-key field name, whether it declares `@@paged`, and whether/where it declares `@version`.
None of them has a *runtime* discovery path: the generated client carries no `$meta` object and no
introspection endpoint, nothing beyond what its TypeScript **types** already encode at compile time
(confirmed against the real generated `client.ts`/`models.ts` this package's own tests drive — see
[tests/](./tests)). So the provider itself has to be a hand-written runtime function; there is
nothing schema-shaped in its logic to generate.

The **manifest** is a different question, and cratestack#571 settled it the other way: those four
facts all live in the `.cstack` schema, so the generator emits them. Pass `--refine` to
`cratestack generate-typescript` and it writes an extra `src/refine.ts` next to the client:

```bash
cratestack generate-typescript \
  --schema schema.cstack \
  --out ./generated \
  --refine
```

```ts
// generated/src/refine.ts — generated, do not edit
export function cratestackRefineResources(client: ExampleApiClientClient): ResourceMap { … }
```

Writing that manifest by hand is still perfectly supported (it is a plain object literal, and every
example below shows one), but the generated one cannot drift: a model that gains `@version`, or one
whose `@id` is not called `id`, updates itself on the next `generate-typescript` run instead of
failing quietly at runtime.

`--refine` works for **both** REST and RPC schemas, on the default preset. The emitted function is
the same shape either way — `cratestackRefineResources(client)` — typed `ResourceMap` for REST and
`RpcResourceMap` for RPC, so consumer code is identical across transports. Only `transport grpc` is
rejected (`TypeScriptGeneratorError::RefineRequiresRestOrRpc`): its client speaks typed protobuf
with no URL-query shaping, so there is nothing for this provider to drive. See [Scope](#scope).

## Usage (REST)

With the generated manifest:

```ts
import { createCratestackDataProvider } from "@cratestack/refine";
import { ExampleApiClientClient } from "./generated/src/client.js";
import { cratestackRefineResources } from "./generated/src/refine.js";

const client = new ExampleApiClientClient("https://api.example.com", { basePath: "/api" });
const dataProvider = createCratestackDataProvider(cratestackRefineResources(client));
```

Or writing the manifest yourself — identical shape, and what the generated file contains:

```ts
const dataProvider = createCratestackDataProvider({
  widgets: { api: client.widgets, primaryKey: "id", paged: false },
  ledgers: { api: client.ledgers, primaryKey: "id", paged: true, versionField: "version" },
  products: { api: client.products, primaryKey: "sku", paged: false },
});
```

Pass this straight to refine's `<Refine dataProvider={dataProvider} .../>`.

## RPC transport

For a `transport rpc` schema, use `createCratestackRpcDataProvider` and an `RpcResourceMap` instead
— same four facts per resource (`api`, `primaryKey`, `paged`, optional `versionField`), same
`getList`/`getOne`/.../`custom` behavior, same filter-operator table and `@version` guarantee as
REST. There is no generated manifest for RPC yet (`--refine` rejects RPC schemas — see above), so
write it by hand:

```ts
import { createCratestackRpcDataProvider } from "@cratestack/refine";
import { ExampleApiClientClient } from "./generated/src/client.js";

const client = new ExampleApiClientClient("https://api.example.com", { basePath: "/api" });
const dataProvider = createCratestackRpcDataProvider({
  widgets: { api: client.widgets, primaryKey: "id", paged: false },
  ledgers: { api: client.ledgers, primaryKey: "id", paged: true, versionField: "version" },
  products: { api: client.products, primaryKey: "sku", paged: false },
});
```

What's different from REST, mechanically:

- The generated RPC model class's `list` takes its query positionally
  (`list(query, options)`) instead of nested in an options object the way REST's `list({ query,
  headers, signal })` does — `createCratestackRpcDataProvider` builds an `RpcListQuery` (RPC's typed
  list-input shape) instead of REST's `CratestackFetchQuery`. `get`/`create`/`update`/`delete` are
  positionally identical between the two transports; only their options type differs
  (`CratestackRpcCallOptions` vs `CratestackRequestConfig`).
- Filters compile to `RpcListPredicate[]` (`{ key, value }` pairs) instead of REST's
  `Record<string,string>` — same `field[__operator]` key convention either way (see
  [Filters](#filters) below), just a different container shape on the wire.
- Sort compiles to a single comma-joined string (`"-createdAt,id"`) instead of REST's `string[]`
  the runtime joins client-side — same `field`/`-field` DSL.
- **Every unary call carries `If-Match` exactly like REST does.** The RPC dispatch arms
  (`crates/cratestack-macros/src/transport/rpc.rs`) pass the real HTTP `HeaderMap` straight through
  to the identical `handle_update_*_dispatch`/`handle_delete_*_dispatch` fns REST uses
  (`crates/cratestack-macros/src/axum/model/handlers_update.rs`), which read `If-Match` via
  `parse_if_match_version` — so a stale or missing `If-Match` against a `@version` model returns the
  same `412 Precondition Failed` on both transports, verified against a real generated RPC client in
  this package's own test suite (`tests/rpc-optimistic-locking.test.ts`).
- **`createCratestackRpcDataProvider` never calls `POST /rpc/batch`.** A batch request is one HTTP
  request carrying N frames, so a per-frame `If-Match` header isn't expressible there —
  `createMany`/`updateMany`/`deleteMany` are N real unary round trips instead, same non-atomic
  strategy REST uses (see [createMany / updateMany / deleteMany](#createmany--updatemany--deletemany)
  below). A batched RPC data provider is possible in principle but out of scope here — see
  [Scope](#scope).

## Filters

Applies to both providers. refine's `CrudFilters` (`{ field, operator, value }`) map onto the same
`field[__operator]` key convention on both transports — REST's `field__operator=value` query params
(`toQueryFilters`) and RPC's `RpcListPredicate[]` (`toRpcQueryFilters`) carry an identical `key`,
just in different container shapes on the wire. Same operator set the generated client's shared
filter types (`EqualityFilter`/`ComparableFilter`/`StringFilter`) expose:

| refine operator | cratestack query key |
| --- | --- |
| `eq` | `field` (no suffix) |
| `ne` | `field__ne` |
| `in` | `field__in` (comma-separated) |
| `lt` / `lte` / `gt` / `gte` | `field__lt` / `__lte` / `__gt` / `__gte` |
| `contains` | `field__contains` |
| `startswith` | `field__startsWith` |
| `null` | `field__isNull=true` |
| `nnull` | `field__isNull=false` |

**Every other refine operator throws** — `endswith`, `between`, `nin`, `containss`, refine's
`or`/`and` filter groups, and anything else not in the table above. Dropping an unsupported filter
silently would show the wrong data as if it had been filtered; an error is strictly better. Note
also that `eq`/`ne`/`in`/comparison operators only work against **required** (non-nullable) fields
— that's a server-side codegen gate
(`crates/cratestack-macros/src/axum/filter_arms.rs::generate_query_filter_arm`), not something this
package can work around.

## Pagination

Applies to both providers. refine's `{ current, pageSize }` maps to `limit`/`offset` directly.
**`totalCount` is only ever
emitted for a `@@paged` model's list route.** Set `paged: false` in a resource's config honestly —
a non-`@@paged` resource's `getList` still returns real data, but `total` degrades to
"how many rows this one response returned" rather than a true count. Either add `@@paged` to the
model, or configure refine's `pagination: { mode: "off" }` for that resource and treat the result as
one (capped) page.

## Primary keys

Applies to both providers. refine assumes every record has `id: BaseKey`. cratestack's `@id` can be on any field
(`primaryKey` in the resource config above). Every returned record gets a synthetic `id` field
alongside its real primary key so refine's row-selection machinery has something to key off;
writes (`get`/`update`/`deleteOne`) receive that same value back and pass it straight through as
the real primary key — no translation needed, since the *value space* is identical. The one place
this bites: a `<Create>` form's fields must use the schema's real primary-key field name (e.g.
`sku`), not `id` — a create payload has no record yet to synthesize an `id` from.

## Optimistic locking (`@version` / `If-Match`)

**The single most important correctness point in this package, and it holds on both transports.**
A `@version` model requires `If-Match` on both update *and* delete (cratestack#493/#519/#538) —
missing or stale `If-Match` returns `412 Precondition Failed` and leaves the row untouched, whether
the request went out as an RPC unary call or a REST `PATCH`/`DELETE` (see
[RPC transport](#rpc-transport) for why the RPC dispatch path enforces this identically to REST).
refine's `update`/`deleteOne` hooks fetch the record before editing it, so the version is known by
the time a mutation fires; each provider remembers it (keyed per `createCratestackDataProvider`/
`createCratestackRpcDataProvider` call, not module-global) from every read/write that returns a
fresh record, and sends it automatically. A `412` is surfaced as a
distinguishable conflict (`statusCode: 412`, a human-readable message) rather than a generic
failure — check `error.statusCode === 412` in an `onError` handler to show a "someone else changed
this" message instead of a generic one.

Pass `meta: { ifMatch: <version> }` to `update`/`deleteOne` to override the cache explicitly (e.g.
optimistic-lock-aware bulk workflows). Calling `update`/`deleteOne` on a `@version` resource with no
known version and no override throws rather than silently omitting `If-Match` — omitting it doesn't
fail on the wire the way a *stale* value does, so this package refuses to make that mistake for you.

## `createMany` / `updateMany` / `deleteMany`

Implemented, not declined, on both providers — but as N sequential single-record round trips
(`Promise.all` over `create`/`update`/`deleteOne`), not a real batch. The generated REST client
exposes no `updateMany`/`deleteMany` wrapper around the server's actual `update_many`/
`delete_many`. The RPC transport does have a real batch endpoint (`POST /rpc/batch`), but
`createCratestackRpcDataProvider` deliberately doesn't use it here either — see
[RPC transport](#rpc-transport) for why (per-frame `If-Match` isn't expressible in a batch request).
So these three methods work on both transports, but without atomicity and at the cost of N requests
instead of one.

## Procedures (`custom`)

A cratestack `procedure` has no other home in a `DataProvider`. Pass a `procedures` map to
`createCratestackDataProvider`'s (or `createCratestackRpcDataProvider`'s) second argument and call
it through refine's `custom` — identical shape on both providers:

```ts
const dataProvider = createCratestackDataProvider(resources, {
  procedures: { publishPost: (args) => client.procedures.publishPost(args as never) },
});

await dataProvider.custom!({
  url: "",
  method: "post",
  meta: { procedure: "publishPost" },
  payload: { args: { postId: 1 } },
});
```

## Scope

**Out of scope, tracked as follow-ups by cratestack#571 itself:**

- **`liveProvider`** — the generated TypeScript client has no SSE consumer today (no `EventSource`
  anywhere in `crates/cratestack-client-typescript/templates/`), even though the server has a real
  subscribe endpoint. Needs a TS SSE client first. Applies to both transports.
- **`authProvider`** — a separate, orthogonal concern from data access. Applies to both transports.
- **Batched RPC writes** — `createCratestackRpcDataProvider` never calls `POST /rpc/batch`, even
  though the RPC transport has one. A per-frame `If-Match` header isn't expressible in a single
  batch request, and this package's `@version` guarantee is not something it will silently weaken
  to get atomicity — see [RPC transport](#rpc-transport).
- **RPC's `where`/`or` filter-expression DSL, and `fields`/`include`/`includeFields` projection** —
  `CratestackRpcListQuery` supports all of these on the wire, but refine's `CrudFilters`/`Sort`
  shapes have nothing that maps onto them (refine has no "raw server filter expression" or
  "relation projection" concept), so this provider never writes to them. Available to a caller who
  wants to reach past refine's abstraction: call `config.api.list({ where: "..." })` directly.

**A known, pre-existing gap this package inherits rather than causes:** route suppression
(cratestack#514) isn't implemented, so a policy-denied `create`/`update`/`delete` still generates a
working-looking client method — `config.api.create` being present only proves the model declares
*some* `@@allow("create", ...)`, not that the current caller satisfies it. A refine `<Create>`
button wired to such a resource renders and then `403`s on submit for a caller who can't actually
create one. Until #514 ships, gate that in refine's own `resources` config
(`create: false`/`edit: false`/`delete: false`) per caller role, not on this package's method
presence.

See `guides/refine-integration.md` in
[cratestack-docs](https://github.com/cratestack/cratestack-docs) for the hand-wired version of
everything this package automates, including the full reasoning behind each design choice above.
