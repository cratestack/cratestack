# react-vite-refine example

A small [refine.dev](https://refine.dev) admin app driven end-to-end by CrateStack codegen against a
**generated WireMock backend** — no database, no hand-written server. The chain:

```
schema.cstack → cratestack generate-typescript --refine  → @cratestack/refine's
                cratestack generate-wiremock              createCratestackDataProvider
                                                                    │
                                                                    ▼
                                                         a real refine.dev admin UI
                                                         driven against a live,
                                                         stateful WireMock container
```

`@cratestack/refine` (`packages/cratestack-refine`) and the `--refine` codegen flag already existed
before this example (issue #571); this is their first real, running consumer. `crates/cratestack-
mock-wiremock`'s stateful model-CRUD stubs (issue #438/#588) are the other half — this example is
the "planned no-database example" their own design doc (`docs/design/wiremock-stubs.md`) names as
the motivating case.

## Why `transport rest`, not `transport rpc`

WireMock's **stateful** model-CRUD stubs only exist for `transport rest`
(`crates/cratestack-mock-wiremock/README.md`'s "Scope" section) — `transport rpc` model routes stay
static, one fixed example replayed on every request regardless of what you send. An RPC-transport
version of this app would have every create/update/delete silently no-op against the UI: you'd
click "Save", see a 200, and the list would never change. `transport rpc` is a first-class,
supported transport elsewhere in this repo (see `examples/rpc-*`) — it just cannot demo CRUD
against a mock with today's `cratestack-mock-wiremock`.

## The schema

Three models, chosen to exercise the cases that actually change what `@cratestack/refine` and the
generated client do:

| Model | What it exercises |
|---|---|
| `Category` | Plain CRUD — `@id` named `id`, no `@@paged`, no `@version`. The baseline. |
| `Post` | `@@paged` (real `{ items, totalCount, pageInfo }` list envelope) + `@version` (`If-Match` optimistic-locking wiring). `published Boolean` also exercises the falsy-value round trip through the stateful stubs (cratestack#588 — `false` must persist as `false`, not be read as "field omitted"). |
| `Tag` | `@id` named `slug`, not `id` — `@cratestack/refine` synthesizes a client-side `id` field from it. |

See `schema.cstack` for the full, commented source.

## Run it

```bash
# 1. Generate the TypeScript client (--refine) + the WireMock stub mappings.
#    Neither is committed (maintainer instruction: don't commit generated
#    build output) — both are gitignored and produced fresh by this recipe.
just react-vite-refine-fixture

# 2. Build the stateful WireMock image (crates/cratestack-mock-wiremock's
#    own Dockerfile — a plain `docker run wiremock/wiremock` does NOT
#    work for stateful stubs; see that crate's README before using a
#    different setup).
docker build -t cratestack-wiremock-stateful \
  -f crates/cratestack-mock-wiremock/docker/Dockerfile \
  crates/cratestack-mock-wiremock/docker

# 3. Run it, mounted against the mappings just generated.
docker run -d --name cratestack-refine-mock -p 8080:8080 \
  -v "$(pwd)/examples/react-vite-refine/wiremock/mappings:/home/wiremock/mappings:ro" \
  cratestack-wiremock-stateful

# 4. Install + run the app.
cd examples/react-vite-refine/web
pnpm install
pnpm run dev
# -> http://localhost:5173
```

Vite's dev server proxies `/api` to the container on `:8080` (`vite.config.ts`) — see "What this
demo can't prove" below for why that proxy exists (it's load-bearing, not incidental).

Open `http://localhost:5173`: three tabs (Categories / Posts / Tags), each backed by
`@refinedev/core`'s `useList`/`useOne`/`useCreate`/`useUpdate`/`useDelete` hooks wired to
`src/dataProvider.ts`'s `createCratestackDataProvider(cratestackRefineResources(client))` — the
**generated** manifest, not a hand-written one. Add, edit, delete rows; every change round-trips
through the live container (create → list, update → get, delete → 404).

`git rev-parse` / `pnpm --version` etc. aside, the one prerequisite this doesn't spell out inline:
Docker running locally (`crates/cratestack-mock-wiremock/README.md`'s "Running the stateful stubs"
section has the full explanation of why the plain upstream image doesn't work).

## Verification

Run for real against a live container — no eyeballing:

```bash
cd examples/react-vite-refine/web
pnpm run verify
```

`scripts/verify.ts` drives the generated client directly (no React, no browser — same shape as
`react-vite-swr`'s `scripts/seed.ts`), logs every request/response, and asserts on each one. A real
run against a freshly built container produced (trimmed to the load-bearing lines):

```
=== Category (plain CRUD) ===
--> POST http://localhost:8080/api/categories
    body: {"id":1,"name":"Rust"}
<-- 201 { "id": 171485 , "name": "Rust" }
--> GET http://localhost:8080/api/categories
<-- 200 [ { "id": 171485 , "name": "Rust" } ]
--> PATCH http://localhost:8080/api/categories/171485
    body: {"name":"Rust (updated)"}
<-- 200 { "id": 171485 , "name": "Rust (updated)" }
--> DELETE http://localhost:8080/api/categories/171485
<-- 200 { "id": 171485 , "name": "Rust (updated)" }
--> GET http://localhost:8080/api/categories/171485
<-- 404
Category: create -> list -> update -> delete -> 404 all verified.

=== Post (@@paged + @version) ===
--> POST http://localhost:8080/api/posts
    body: {"id":1,"title":"Launch","published":false,"version":0}
<-- 201 { "id": 116769 , "title": "Launch" , "published": false , "version": 0 }
--> GET http://localhost:8080/api/posts
<-- 200 { "items": [ {...} ], "totalCount": 1, "pageInfo": {...} }
--> PATCH .../posts/116769  [if-match: "0"]
<-- 200 { ..., "version": 0 }
Server-reported version after update: 1 (bumped by the mock)

Stale If-Match PATCH -> HTTP 200 (real server: 412)
--> DELETE .../posts/116769  [if-match: "0"]
<-- 200 {...}
--> GET .../posts/116769
<-- 404
Post: falsy round trip verified; If-Match gap confirmed and documented; delete -> 404 verified.

=== Tag (@id named `slug`) ===
--> POST http://localhost:8080/api/tags
    body: {"slug":"topic-databases","label":"Databases"}
<-- 201 { "slug": "ck1mgdcumrr4i1ig" , "label": "Databases" }
[... update -> delete -> 404, same shape ...]

✓ All assertions passed (14 HTTP requests logged above).
```

Also driven by hand in a real browser (Categories/Posts/Tags tabs: add, edit, delete, confirmed
against the container's actual state via `fetch('/api/...')`), and covered by CI's
`js (react-vite-refine example)` job on every PR — see `.github/workflows/ci.yml`.

Offline coverage (no Docker, no network) lives in `tests/smoke.rs`: the schema parses with the
three intended models, `--refine` emits the three distinct resource-manifest shapes, WireMock emits
five mappings per model, the falsy `published` round trip is presence-tested not truthiness-tested,
and — the trip-wire test — the `If-Match` gap below is asserted, not just documented, so it starts
failing the moment it's no longer true.

```bash
cargo test -p react-vite-refine-example   # 5 tests, all pass, no DB/Docker needed
```

## Sorting, filtering, pagination — deliberately not built

`cratestack-mock-wiremock`'s generated stubs ignore `field__operator=value`, `sort`, `limit`, and
`offset` entirely — every `list` response is the complete, unfiltered collection regardless of what
the query string says (`crates/cratestack-mock-wiremock/README.md`'s "Scope" section, `docs/design/
wiremock-stubs.md`). refine's higher-level table/list components (`@refinedev/antd`'s `useTable`,
etc.) ship sort/filter/pagination controls by default; against this mock they would render, appear
to work, and silently do nothing — worse than not offering them at all.

**The decision made here:** this app is built directly on `@refinedev/core`'s headless
`useList`/`useCreate`/`useUpdate`/`useDelete` hooks, not a prebuilt UI-kit table — so there is no
default sort/filter/pagination affordance to suppress in the first place. Every list call passes
`pagination: { mode: "off" }` explicitly (see any `src/pages/*Page.tsx`) rather than defaulting to
refine's `{ current, pageSize }` paging, which — per `@cratestack/refine`'s own README ("Pagination"
section) — a non-`@@paged` resource can't honestly answer anyway (`Category`/`Tag` here), and even
`Post`'s real `@@paged` + `totalCount` envelope is moot when the mock never actually slices by
`limit`/`offset`. No column headers are clickable, no filter inputs exist, no page-number control
renders. This is a deliberate scope choice for the mock-backend demo, not a limitation of
`@cratestack/refine` itself — its README documents full pagination/filter/sort support for a real
`cratestack-pg` server, and its own test suite (`packages/cratestack-refine/tests/pagination.test.ts`,
`filters.test.ts`) proves that logic against a fake server that actually implements it.

## What this demo can't prove

Two confirmed, real gaps in `cratestack-mock-wiremock` (not bugs in this example) that a reader
building on this pattern needs to know before they trust it too far:

**1. ~~The mock does not validate `If-Match`~~ — fixed in #605; optimistic locking IS demonstrated
live.** This section previously documented a real gap: `cratestack-mock-wiremock` matched on method
and path only, so a stale `If-Match` got a `200` and `@version` — a headline framework guarantee —
was the one thing this demo couldn't show. `tests/smoke.rs` asserted that absence as a deliberate
trip-wire, with a failure message telling whoever closed the gap to rewrite this paragraph. It
fired. This is that rewrite.

The mock now mirrors the real server's contract exactly, verified live against the container
(`web/scripts/verify.ts` asserts every row, so a regression fails the script rather than surprising
a reader):

| Request | Response |
|---|---|
| `PATCH` with no `If-Match` | **412** — required on a `@version` model, not optional |
| `PATCH` with `If-Match: *` | **400** — explicitly unsupported on versioned models |
| `PATCH` with `If-Match: 0` (unquoted) | **400** — must be a strong ETag, `"<integer>"` |
| `PATCH` with `If-Match: "9999"` (stale) | **412** |
| `PATCH` with the current `If-Match: "0"` | **200**, body `version: 1`, `ETag: "1"` |
| replaying the now-consumed `If-Match: "0"` | **412** |

`GET` returns `ETag: "<version>"` so a client can round-trip it, and `DELETE` enforces the
precondition too — the real server closed that asymmetry deliberately and the mock follows.

Two residual differences from a real `cratestack-pg` server, both deliberate and small: the two
distinct 400 messages for a malformed `If-Match` collapse to one here, and `transport rpc` model
CRUD remains entirely static (no state, no preconditions) — see
`docs/design/wiremock-stubs.md` §10.

**2. `create` never honors a client-submitted primary key — the mock always fabricates its own.**
`cratestack-mock-wiremock`'s `id_generator` (`crates/cratestack-mock-wiremock/src/model_state/
fragments.rs`) synthesizes a random id/slug on every `create`, for every primary-key type (`Int`,
`Uuid`, `Cuid`, plain `String`) — the `id`/`slug` value you submit satisfies the generated
`CreateCategoryInput`/`CreatePostInput`/`CreateTagInput` TypeScript type (which a real `cratestack-
pg` server DOES honor) but is silently discarded by this mock. `verify.ts` and every page's create
form still ask for it (to keep the generated input types honest), but every follow-up call must use
the id/slug the mock actually returned — `Tag`'s create form asks for a human-readable `slug` like
`topic-databases` and the mock hands back something like `ck1mgdcumrr4i1ig` instead. Confirmed by
hand (see the `verify.ts` transcript above — every subsequent request targets the server-returned
id, not the submitted one) and not asserted as a trip-wire test the way the `If-Match` gap is, since
it's a `create`-only concern with no separate code path here to regress independently.

## Layout

| Path | What |
|---|---|
| [`schema.cstack`](schema.cstack) | `Category`/`Post`/`Tag` models — see comments for why each shape |
| [`src/lib.rs`](src/lib.rs) | No server macro — just `SCHEMA_PATH`; this example has no Rust server at all |
| [`tests/smoke.rs`](tests/smoke.rs) | Offline: schema shape, `--refine` manifest, WireMock mapping shape, the `If-Match` trip-wire |
| `web/generated/` | Generated `--refine` TypeScript client — gitignored, produced by `just react-vite-refine-fixture` |
| `wiremock/` | Generated WireMock stub mappings — gitignored, same recipe |
| [`web/src/dataProvider.ts`](web/src/dataProvider.ts) | Wires the generated client + generated manifest into `createCratestackDataProvider` |
| [`web/src/App.tsx`](web/src/App.tsx) | `<Refine>` + a hand-rolled tab switcher (no `routerProvider` — not needed) |
| `web/src/pages/*Page.tsx` | One page per model, headless `@refinedev/core` hooks, no UI kit |
| [`web/scripts/verify.ts`](web/scripts/verify.ts) | Live CRUD + `If-Match`-gap verification, outside React (see "Verification" above) |
| [`web/vite.config.ts`](web/vite.config.ts) | The `/api` dev-server proxy (see "What this demo can't prove" — the mock has no CORS handling) |

## CI

`.github/workflows/ci.yml`'s `js (react-vite-refine example)` job builds the real WireMock image,
runs it, generates both fixtures, and runs `pnpm run build`/`typecheck`/`lint`/`verify` against the
live container on every PR — the first CI job in this repo to actually build and run `cratestack-
mock-wiremock`'s Docker image (its own crate tests only assert on generated template text, offline).
No Docker layer caching is wired for it yet (nothing in this repo's CI does that today), so the
image's Gradle-based build cost is paid on every run — a real, bounded, one-time-per-job cost, not a
gap being hidden.

## Docs follow-up (not made in this PR)

`~/dev/cratestack-docs`' `guides/refine-integration.md` documents the hand-wired
`@cratestack/refine` integration; it should eventually link to this example as the runnable version
of that guide. Left for a docs-repo PR, since this PR only touches the `cratestack` repo.
