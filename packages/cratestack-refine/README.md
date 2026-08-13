# @cratestack/refine

A [refine.dev](https://refine.dev) `DataProvider` over CrateStack's generated TypeScript **REST**
client. Wires refine's fixed `getList`/`getOne`/`getMany`/`create`/`update`/`deleteOne` surface to
a generated model class (`client.widgets`, `client.ledgers`, …), mapping refine's filter operators,
pagination, primary keys, and `@version` optimistic-locking conflicts onto the server's real
contract — the same one [cratestack-studio](https://cratestack.dev) deliberately bypasses.
`cratestack-studio` talks to `[target.db]` directly and skips `@@allow`; a refine app built on this
package goes through the generated API and inherits policy, validation, `@version` concurrency, and
audit. That's the reason this package exists (cratestack#571): refine is the safe end-user admin
surface, Studio is the sysadmin one.

This package covers REST-transport schemas only (`generate-typescript`'s default transport, no
`transport rpc`) — see [Scope](#scope) below.

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

`--refine` requires a REST schema and the default preset — see [Scope](#scope).

## Usage

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

## Filters

refine's `CrudFilters` (`{ field, operator, value }`) map onto the generated list route's
`field__operator=value` query convention — the same operator set the generated client's shared
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

refine's `{ current, pageSize }` maps to `limit`/`offset` directly. **`totalCount` is only ever
emitted for a `@@paged` model's list route.** Set `paged: false` in a resource's config honestly —
a non-`@@paged` resource's `getList` still returns real data, but `total` degrades to
"how many rows this one response returned" rather than a true count. Either add `@@paged` to the
model, or configure refine's `pagination: { mode: "off" }` for that resource and treat the result as
one (capped) page.

## Primary keys

refine assumes every record has `id: BaseKey`. cratestack's `@id` can be on any field
(`primaryKey` in the resource config above). Every returned record gets a synthetic `id` field
alongside its real primary key so refine's row-selection machinery has something to key off;
writes (`get`/`update`/`deleteOne`) receive that same value back and pass it straight through as
the real primary key — no translation needed, since the *value space* is identical. The one place
this bites: a `<Create>` form's fields must use the schema's real primary-key field name (e.g.
`sku`), not `id` — a create payload has no record yet to synthesize an `id` from.

## Optimistic locking (`@version` / `If-Match`)

**The single most important correctness point in this package.** A `@version` model requires
`If-Match` on both update *and* delete (cratestack#493/#519/#538) — missing or stale `If-Match`
returns `412 Precondition Failed` and leaves the row untouched. refine's `update`/`deleteOne` hooks
fetch the record before editing it, so the version is known by the time a mutation fires; this
package remembers it (keyed per `createCratestackDataProvider` call, not module-global) from every
read/write that returns a fresh record, and sends it automatically. A `412` is surfaced as a
distinguishable conflict (`statusCode: 412`, a human-readable message) rather than a generic
failure — check `error.statusCode === 412` in an `onError` handler to show a "someone else changed
this" message instead of a generic one.

Pass `meta: { ifMatch: <version> }` to `update`/`deleteOne` to override the cache explicitly (e.g.
optimistic-lock-aware bulk workflows). Calling `update`/`deleteOne` on a `@version` resource with no
known version and no override throws rather than silently omitting `If-Match` — omitting it doesn't
fail on the wire the way a *stale* value does, so this package refuses to make that mistake for you.

## `createMany` / `updateMany` / `deleteMany`

Implemented, not declined — but as N sequential single-record round trips (`Promise.all` over
`create`/`update`/`deleteOne`), not a real batch. The generated REST client exposes no
`updateMany`/`deleteMany` wrapper around the server's actual `update_many`/`delete_many`, and
`/rpc/batch` is an RPC-transport-only endpoint. So these three methods work, but without atomicity
and at the cost of N requests instead of one.

## Procedures (`custom`)

A cratestack `procedure` has no other home in a `DataProvider`. Pass a `procedures` map to
`createCratestackDataProvider`'s second argument and call it through refine's `custom`:

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
  subscribe endpoint. Needs a TS SSE client first.
- **`authProvider`** — a separate, orthogonal concern from data access.
- **RPC-transport schemas** (`transport rpc`) — this package's `DataProvider` above is written
  against the REST client's `list(options)`/`CratestackFetchQuery` shape, which the RPC client
  does not share (its `list(query, options)` takes the query positionally, as a
  `CratestackRpcListQuery`), so it needs its own mapping layer, not documented here.
  The generator-side restriction this depended on has been lifted (cratestack#571's follow-up):
  `cratestack generate-typescript --refine` now emits an `src/refine.ts` for `transport rpc`
  schemas too, typed `RpcResourceMap` instead of `ResourceMap` — see
  `crates/cratestack-client-typescript/README.md`'s `--refine` section. That manifest is
  transport-agnostic (same four facts either way); it is this package's RPC-shaped provider
  that is the remaining piece, tracked as a companion change.

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
