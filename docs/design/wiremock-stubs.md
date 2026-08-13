# WireMock stub generation from `.cstack` schemas

Status: **v2 implemented** (`cratestack-mock-wiremock`, `cratestack
generate-wiremock`) — happy-path stubs for procedures **and** `model`
CRUD routes (`list`/`get`/`create`/`update`/`delete`), under `transport
rest`/`transport rpc`. Not stateful — see §8. Scope and open questions
below are current, not historical: several are deliberately deferred,
not resolved.
Scope: a new generator crate (`crates/cratestack-mock-wiremock`), a new CLI
subcommand (`generate-wiremock`), no changes to `cratestack-parser` or
`cratestack-macros`.
Tracking: issue #438. Model CRUD (§8) motivated by the `refine.dev`
admin app, `packages/cratestack-refine`, and its planned no-database
example, `examples/react-vite-refine`.

## 1. The problem

CrateStack is schema-first: a `.cstack` file defines models and procedures,
and `cratestack generate-dart`/`generate-typescript`/`generate-proto`
derive a client (or `.proto` description) from it that cannot drift from
the schema without a rebuild. Nothing generates a **mock server**. Anyone
testing a client (mobile app, web frontend, another backend service)
against a CrateStack-generated API today either runs the real server (a
real database, real downstream dependencies) or hand-writes stub fixtures
— and hand-written stubs are exactly the kind of artifact this project's
own generated-client story exists to replace: a fixture that keeps
returning yesterday's shape after the schema changes, silently, with
nothing to catch it.

This is not hypothetical. `ADORSYS-GIS/webank-mobile` — a production
consumer of CrateStack-generated Dart clients (ADR 0032, the Go-to-Rust BFF
migration) — has **37 hand-maintained WireMock JSON mapping files**
(`wiremock/mappings/{fineract,customer-service,eml,keycloak}/*.json`,
36 present in the checkout inspected for this proposal, the wording keeps
this loosely stated on purpose since the count moves) stubbing downstream
services for its integration/e2e suite. None of those are generated; all
of them can (and, per the project's own migration history, already has
led to real defects tracked as separate issues) drift from the real
contract. The project's stated intent is to build a proper e2e testing
framework on CrateStack, and a generated mock is the natural next piece:
the same schema that produces the real server and the real client should
also produce the thing that stands in for the server during a test.

## 2. What the schema actually carries, and what that means for a stub

Read before designing, not assumed:

- **Wire shape.** `crates/cratestack-macros/src/transport/rest.rs` fixes
  every procedure's REST route to `POST /$procs/<name>` regardless of
  whether the schema says `transport rest` explicitly or takes it as the
  default — there is no per-procedure REST verb/path customization the way
  a hand-designed REST API would have. `transport rpc` schemas use
  `POST /rpc/{op_id}` (`cratestack_core::rpc::RPC_UNARY_PATH`) instead,
  where `{op_id}` for a procedure is **`procedure.<name>`**, not the bare
  name — `generate_procedure_rpc_dispatch_arm` in
  `crates/cratestack-macros/src/transport/rpc.rs` builds it as
  `format!("procedure.{}", procedure.name)`, matched byte-for-byte by the
  generated Dart RPC client (`'procedure.{{ procedure.name }}'` in
  `templates/rpc-apis.dart.j2`) and exercised end-to-end by
  `crates/cratestack-pg/tests/rpc_canonical_request.rs` and
  `.../tests/include_schema.rs`, both of which hit
  `/rpc/procedure.ping`, never `/rpc/ping`. A first version of this
  generator got this wrong (bare `/rpc/<name>`, caught in review before
  merge, see the PR history) — worth calling out explicitly here because
  it's exactly the failure mode this crate exists to prevent: a stub that
  never matches fails a test for a reason that looks like the code under
  test rather than the fixture. Either way (REST or RPC), one route per
  procedure, always `POST`, body in and body out with no envelope.
- **Status codes.** Every procedure's success response is a literal
  `axum::http::StatusCode::OK` — `crates/cratestack-macros/src/axum/
  procedure.rs`'s `generate_procedure_axum_handler` hardcodes it; there is
  no schema-level way to declare a different 2xx status today (open
  upstream as **#407**, filed against a real webank-services case: a
  ported KYC-submit route whose original Go handler returned `202
  Accepted` had to become a plain `200` carrying `{"status":"processing"}`
  instead, because CrateStack had nothing better to offer). A v1 stub
  generator should match this reality — `200` on every happy-path
  response — not invent status semantics the real server doesn't have.
  Error responses, by contrast, *do* carry real, varied status codes via
  `CoolError::status_code()` (400/401/403/404/409/422/503/500/…) — out of
  scope for v1 (see §7), but real for a future error-stub slice to target.
- **Model CRUD routes are a different, richer shape.** `model` blocks get
  five REST routes each (`generate_model_axum_routes` in
  `crates/cratestack-macros/src/axum/model/routes.rs`): `GET`/`POST
  /<plural>`, `GET`/`PATCH`/`DELETE /<plural>/{id}`, `POST` returning
  `201` instead of `200`. Out of scope for v1 — procedures were both the
  simpler case and webank's actual motivating case (`crates/bff`'s
  schema is `provider = "none"`: procedures only, no models, by
  construction — see `docs/design/no-database-mode.md`). Covered as of
  v2 (§8), motivated by `packages/cratestack-refine`, a model-CRUD-only
  consumer with no procedures of its own.
- **Nothing adjacent already does this.** `cratestack generate-proto`
  emits a `.proto` *description*, not a mock; no OpenAPI emitter or
  test-harness generator exists in this repo today (verified by reading
  every generator crate under `crates/` and every `cratestack generate-*`
  subcommand in `crates/cratestack-cli/src/cli_types.rs`). This is new
  ground, not an extension of an existing generator.

## 3. Design

A new library crate, `cratestack-mock-wiremock`, mirroring the existing
generator crates' shape (`generate_package(&Schema, &Config) ->
Result<Package, Error>`, one `GeneratedFile { file_name, contents }` per
output file) and a new CLI subcommand, `generate-wiremock`, mirroring
`generate-dart`/`generate-typescript`'s `--schema`/`--out`/`--check` flags
plus their shared `--base-path` (default `/api`) — the same prefix a
generated client is configured with, so a WireMock instance and a
generated client pointed at the same `--base-path` agree on the exact URL.

Unlike the Dart/TypeScript generators, this crate does **not** use
`minijinja` templates — the output is JSON, not source code, and building
`serde_json::Value`s directly with `serde_json::to_string_pretty` is both
simpler and safer (no risk of a template producing invalid JSON via
unescaped interpolation).

One file per procedure, `mappings/<procedureName>.json`, and (as of v2,
§8) five files per model, `mappings/model.<ModelName>.<verb>.json` for
`verb` in `list`/`get`/`create`/`update`/`delete` — the
`model.<Name>.<verb>` naming echoes the RPC op-id convention
(`model.<Name>.<verb>`, `crates/cratestack-macros/src/transport/rpc.rs`)
so it reads the same way whether the schema is REST or RPC, and can
never collide with a procedure file (procedure names cannot contain a
literal `.`). `mappings/` is the directory name a WireMock instance
scans by convention, so `--out` can point directly at a project's
existing WireMock root.

### 3.1 Example

Given:

```cstack
datasource db {
  provider = "none"
}

type Greeting {
  message String
}

procedure hello(): Greeting
```

`cratestack generate-wiremock --schema schema.cstack --out wiremock`
writes `wiremock/mappings/hello.json`:

```json
{
  "metadata": {
    "cratestack": { "generated": true, "kind": "query", "procedure": "hello" }
  },
  "request": { "method": "POST", "urlPath": "/api/$procs/hello" },
  "response": {
    "headers": { "Content-Type": "application/json" },
    "jsonBody": { "message": "string" },
    "status": 200
  }
}
```

### 3.2 Real output against webank-mobile's actual schema

Run against `ADORSYS-GIS/webank-mobile`'s committed
`packages/rust_api.schema.cstack` (1900 lines, the real `crates/bff`
schema, `provider = "none"`) as an end-to-end check, not just the toy
example above: `generate-wiremock` produces 40 mapping files, one per
declared procedure, covering the full route surface — `paymentProviders`,
`getMyReferral`, `enrolDevice`/`confirmEnrolDevice`, `kycStatus`,
`submitKycDocument`, `sendP2P`, `cashoutScan`/`cashoutSettle`, etc. — with
no manual intervention. `paymentProviders.json`'s synthesized body, for a
reply type with a nested list of a nested type:

```json
{
  "jsonBody": {
    "aggregator": "string",
    "providerType": "string",
    "providers": [
      { "available": true, "code": "string", "icon": "string", "name": "string" }
    ]
  }
}
```

`submitKycDocument.json` — the exact procedure `#407` was filed about —
correctly reflects today's reality (`200`, not a synthesized `202`):

```json
{ "response": { "status": 200, "jsonBody": { "status": "string" } } }
```

`--check` (drift detection, mirroring `generate-dart --check`) also works
end-to-end against this schema: a second run reports no drift.

### 3.3 What the real-schema smoke test does and doesn't cover

§3.2's run against webank-mobile's actual `crates/bff` schema is real,
useful evidence — but it is evidence about *that* schema's shapes only,
not a substitute for exercising every shape this generator claims to
handle. Two review findings on the PR that introduced this crate landed
on exactly that gap, both in code paths the webank schema structurally
cannot reach:

- The RPC-transport route (`/rpc/procedure.<name>`, §2) — webank's
  schema is `transport rest`, so the RPC branch was never executed by
  the smoke test at all.
- The mutual-cycle-broken-by-a-`List`-step case in `values.rs`'s cycle
  guard (`type A { b: B[] } type B { a: A }`) — webank's schema has no
  mutually-recursive `type`s, so this shape never came up either.

Both are now covered by targeted unit tests in
`crates/cratestack-mock-wiremock/tests/procedures.rs` instead
(`pins_the_exact_url_path_for_both_transports`,
`mutual_cycle_broken_by_a_list_step_on_only_one_side_still_terminates`).
The lesson generalizes: a smoke test against one real, organically-grown
schema is a strong signal for the shapes that schema happens to use, and
no signal at all for the ones it doesn't — this generator's actual test
coverage for "does every schema construct synthesize correctly" has to
come from the unit test suite's deliberately-constructed shapes
(self-reference, mutual reference, `Page<T>`, enums, both transports,
…), not from any single real-world schema, however large.

## 4. Design questions, answered for v1

- **What does a stub return? Deterministic defaults? Schema-declared
  examples? A seed?** Deterministic, fixed-per-scalar-type defaults
  (`String` -> `"string"`, `Int` -> `0`, `Boolean` -> `true`, `DateTime` ->
  a fixed epoch timestamp, `Uuid` -> a nil UUID, enums -> the first
  declared variant, `Page<T>` -> one synthesized item plus a real
  `PageInfo` envelope, …), not random values or a seed. Two runs against
  an unchanged schema are byte-identical — the same property that makes
  `--check` a meaningful CI gate and makes it safe to gitignore generated
  stubs (§6). Schema-declared examples (a hypothetical `@example(...)`
  attribute) would be strictly better for stub *readability* and are left
  as a clean follow-up — nothing in this design forecloses it; it would
  slot in as an extra lookup ahead of the type-name fallback in
  `cratestack-mock-wiremock/src/values.rs`.
- **How do callers vary responses? Real tests need error cases.** v1
  deliberately doesn't attempt this: every stub matches on request method
  + path only (no body assertion) and always answers the happy path.
  WireMock's own scenario/priority/request-matcher features are the right
  tool for error variants, but layering them on top of a *generated*
  response needs its own design (where does the "return a 404 for this
  specific input" case get declared — in the schema? in a hand-authored
  overlay file alongside the generated ones? see #407's per-procedure
  status question, which is a prerequisite for a generated 4xx/5xx variant
  to even have a status to use). Left as an open follow-up rather than
  guessed at here.
- **Auth. Every procedure sits behind an auth chokepoint. Skip it, or
  emulate rejection?** v1 skips it entirely — stubs match on method+path
  only, with no header/auth assertion, so any request (authenticated or
  not) gets the happy-path response. This is a real limitation: it means
  a WireMock-backed test suite can't exercise "what does an unauthenticated
  call see" against a generated stub. It's also the only honest choice for
  v1, because the auth chokepoint isn't a schema-level concept in the same
  sense a return type is — `@allow(...)` policies are evaluated
  server-side against an authenticated context the schema doesn't fully
  describe (see `crates/cratestack-policy`), and in at least one real
  deployment (webank's Rust BFF) the actual auth chokepoint is a
  *hand-written* axum middleware in front of the generated router, not
  something CrateStack itself emits. Emulating rejection convincingly
  needs its own design pass once there's a concrete second use case to
  design against, not a guess bolted onto this PR.
- **Where do generated stubs live, and how are they refreshed? If
  committed, they go stale exactly like hand-written ones.** Not
  committed — gitignored and regenerated from a pinned schema (hash or
  commit pin), the same pattern `ADORSYS-GIS/webank-mobile`'s own
  `mobile/CLAUDE.md` already documents for its generated Dart client
  (`packages/rust_api/` is entirely gitignored; only the schema pin and a
  copy of the schema itself are committed, and a bootstrap script
  regenerates the client from those on every build). Applying the same
  shape here: a project would commit its `.cstack` schema (or a pinned
  copy of it, if the schema itself lives in a different repo) and a small
  script that runs `cratestack generate-wiremock`, not the generated
  `mappings/*.json` files themselves. `--check` exists specifically to let
  CI enforce "the committed schema and the WireMock stubs a test run
  against actually agree," the same job `generate-dart --check` already
  does for the Dart client.
- **Is WireMock the right target at all, or a general mock-server
  emitter with WireMock first?** WireMock is the right *first* target —
  it's the tool the motivating case (webank-mobile) already uses, it
  needs no runtime beyond a JVM/Docker image, and its stub-mapping format
  is a stable, external, documented JSON shape this crate can target
  without inventing its own IR. But the internal shape of this generator
  (`Schema` in, one synthesized example value per return type, one
  request/response pair per procedure) has nothing WireMock-specific
  about it until `mapping.rs` assembles the final JSON — a second backend
  (a plain JSON-fixture-per-route directory for a hand-rolled mock server,
  or `msw` handlers for a JS test suite) could reuse `values.rs` entirely.
  Not built speculatively here (no second backend exists yet to prove the
  abstraction against), but the crate is structured so it wouldn't be a
  rewrite.

## 5. Cycle safety

A schema can be self-referential (`type Node { next Node? }`,
`type A { b B } type B { a A }`). `values.rs` tracks which composite type
names are currently being expanded on the active recursion path; a
repeated name at an `Optional` or `List` step terminates that branch
(`null` / `[]`) instead of recursing forever, while a repeated name
reachable only through `Required` fields — a cycle with no finite
instance — is a real, immediate error
(`WireMockGeneratorError::UnbreakableCycle`), not a stack overflow.

## 6. Committed vs. generated

See §4's answer above — the recommendation is the same "commit the pin,
gitignore the output, regenerate in CI" shape webank-mobile already uses
for its Dart client, applied to WireMock stubs. This crate itself is
agnostic to where `--out` points; the recommendation is a usage pattern
for consumers, not something `generate_package` enforces.

## 7. Explicitly out of scope (tracked, not forgotten)

- **`transport grpc` schemas.** Rejected up front
  (`WireMockGeneratorError::UnsupportedTransport`) — WireMock stubs a
  JSON/HTTP wire shape, not protobuf-over-HTTP/2; grpc needs a different
  mock target (a gRPC mock server, or `grpcurl`-style fixtures), not an
  extension of this crate.
- **Error-case stubs, request-body matching, auth emulation.** See §4.
  Also applies to model CRUD (§8): a `list` route's `field__operator=`
  filters, `sort`/`orderBy`, `limit`/`offset`, and `fields`/`include`
  selection (`crates/cratestack-axum/src/query.rs`) are not asserted or
  varied on — every request that matches a stub's method + path gets the
  same synthesized response regardless of query string.
- **`FindMany<T>` return types.** Schema validation already forbids
  `FindMany<T>` anywhere outside a procedure *argument* position, so this
  is defense-in-depth in `values.rs`, not a real gap in practice — kept as
  a real error rather than an `unreachable!()` only because this crate's
  public API takes `&Schema` directly, so an unvalidated or hand-built
  schema could still reach it.
- **Model CRUD statefulness beyond what §8 ships.** See §9 — a real
  per-record store needs a third-party WireMock Java extension, a
  dependency decision left to the maintainer rather than picked here.

## 8. Model CRUD (v2)

`model` blocks get the same treatment procedures got in v1: one stub per
route, happy-path only, deterministic synthesized bodies. Five verbs per
model — `list`, `get`, `create`, `update`, `delete` — derived the same
way the real server derives them, not re-implemented by hand:

- **Route paths.** The REST plural segment is
  `cratestack_core::route_naming::model_route_segment(&model.name)` —
  imported directly from `cratestack-core`, not re-derived (see this
  repo's #345 history: two independent reimplementations of this exact
  function drifted apart before it was centralized). `list`/`create`
  share `{base}/{plural}` (`GET`/`POST`); `get`/`update`/`delete` share
  `{base}/{plural}/{id}` (`GET`/`PATCH`/`DELETE`). The `{id}` segment has
  no fixed value to match — this generator has no record store (§9) — so
  those three routes are a WireMock `urlPathPattern` regex
  (`^{base}/{plural}/[^/]+$`, with `base` regex-escaped) instead of an
  exact `urlPath`, matching *any* id. Verified against a real WireMock
  container (§8.3): `GET /api/posts/42` matches the `get` stub for a
  schema that never mentions `42` anywhere.
- **RPC routes fall out for free.** `transport rpc` schemas dispatch
  model verbs to `POST /rpc/model.<ModelName>.<verb>`
  (`generate_model_rpc_dispatch_arms` in
  `crates/cratestack-macros/src/transport/rpc.rs`) — five distinct exact
  paths, no `{id}` in the URL at all (it travels in the request body
  instead), so RPC stubs need no pattern matching. Because REST and RPC
  dispatch to the *same* handler body per verb (the doc comments on
  every handler in `handlers_crud.rs`/`handlers_update.rs`/
  `handlers_list.rs` say so explicitly), the response bodies and status
  codes are identical between the two transports — only the route shape
  differs. This is why RPC model support "fell out cheaply": the
  transport-specific piece is ~25 lines
  (`crates/cratestack-mock-wiremock/src/model_mapping/rpc.rs`).
- **Status codes.** `create` is `201`, matching
  `build_create_handler`'s literal `StatusCode::CREATED` — the one place
  a model stub's status differs from every procedure stub's `200`.
  `list`/`get`/`update`/`delete` are all `200`.
- **Response bodies mirror the default projection, not a full model
  dump.** A `get`/`list`/`create`/`update`/`delete` response body
  excludes two field categories the real server's default projection
  (`crates/cratestack-macros/src/axum/model/serializers/
  projection_fields.rs`) also excludes: relation fields (fields whose
  type names another declared model — populated only via
  `include=<relation>`) and `@server_only` fields (never serialized to a
  client). Every other field is synthesized with the same deterministic
  per-scalar-type defaults procedures already use
  (`crates/cratestack-mock-wiremock/src/values.rs`, reused as-is).
- **`list` reuses the `Page<T>` envelope shape.** An `@@paged` model's
  `list` response is `{items, totalCount, pageInfo}` — the identical
  shape `values.rs` already synthesizes for a procedure returning
  `Page<T>`; a non-`@@paged` model's `list` is a bare JSON array.

### 8.1 Real output against a real fixture

Run against `crates/cratestack-client-dart/tests/fixtures/ci_rest.cstack`
(two models, `Author` and `@@paged Post`, related to each other) instead
of a synthetic toy schema:

`mappings/model.Post.get.json`'s body — `author` (the relation back to
`Author`) is absent, every scalar field is present:

```json
{ "authorId": 0, "id": 0, "published": true, "status": "draft", "title": "string" }
```

`mappings/model.Author.list.json` — not `@@paged`, so a bare array, and
`posts` (the relation to `Post`) is absent:

```json
[{ "id": 0, "name": "string" }]
```

`mappings/model.Post.list.json` — `@@paged`, so the envelope:

```json
{
  "items": [{ "authorId": 0, "id": 0, "published": true, "status": "draft", "title": "string" }],
  "pageInfo": { "hasNextPage": false, "hasPreviousPage": false, "limit": null, "offset": null },
  "totalCount": 1
}
```

`--check` round-trips cleanly against this fixture, and correctly flags
drift: adding a field to `Author` and re-running `--check` without
regenerating reports 6 stale files (all 5 `model.Author.*.json` plus,
because `listPosts()`'s procedure return type `Post[]` transitively
expands the `author: Author` relation the way procedure synthesis always
has — see §2's "richer shape" note — `listPosts.json` too).

### 8.2 What was deliberately *not* built

- **No query-string assertion or variation.** See §7 — a `list` stub
  answers the same body no matter what `field__operator=`/`sort`/
  `limit`/`offset`/`fields`/`include` values a request sends.
- **No request-body assertion on `create`/`update`.** Same v1 precedent
  as procedures (§4) — any body is accepted.
- **No per-record statefulness.** The single largest piece of scope this
  design doc considered and did not build — see §9.

### 8.3 Real-WireMock verification

Unlike v1's JSON-shape-only verification, this slice was checked against
an actual `wiremock/wiremock:3.9.1` container (`docker run`, mappings
volume-mounted read-only), not just asserted in Rust unit tests. Against
`ci_rest.cstack`'s generated mappings:

| Request | Status | Body |
|---|---|---|
| `GET /api/posts` | 200 | `{"items":[{"authorId":0,"id":0,"published":true,"status":"draft","title":"string"}],"pageInfo":{...},"totalCount":1}` |
| `GET /api/posts/42` | 200 | `{"authorId":0,"id":0,"published":true,"status":"draft","title":"string"}` |
| `POST /api/posts` (body `{"title":"hello",...}`) | 201 | same synthesized body — **not** `"hello"` (§9) |
| `PATCH /api/posts/42` (body `{"title":"updated"}`) | 200 | same synthesized body — **not** `"updated"` (§9) |
| `DELETE /api/posts/42` | 200 | same synthesized body |
| `GET /api/posts/42` again, after the `DELETE` above | 200 | identical to the first `GET` — nothing was actually deleted (§9) |
| `GET /api/authors` | 200 | `[{"id":0,"name":"string"}]` (bare array — not `@@paged`) |
| `GET /api/widgets` (no such model) | 404 | WireMock's own "Request was not matched" page |
| `POST /rpc/model.Widget.list` (separate `transport rpc` schema) | 200 | `[{"id":0,"name":"string"}]` |
| `POST /rpc/model.Widget.get` / `.create` / `.update` / `.delete` | 200/201/200/200 | analogous synthesized bodies |

The `GET /api/posts/42` twice, before and after a `DELETE`, is the
concrete evidence for §9's central finding: these stubs are not
stateful.

## 9. Model CRUD statefulness — investigated, not built

The motivating consumer for model CRUD stubs is `packages/
cratestack-refine`, a refine.dev admin app driven entirely by model
CRUD, and a planned example
(`examples/react-vite-refine`) that runs a Vite + refine app against
these stubs with **no database**. For that example to be a convincing
demo rather than "a static JSON blob wearing a mock server's clothes",
a record created through the app should appear in the next `list`, and
an update should be visible on the next `get`. This section is the
investigation into whether that's achievable, done *before* building
anything, per this feature's own instructions — concluding "not without
a dependency decision the maintainer should make" is the actual
deliverable here, not a consolation prize.

### 9.1 What vanilla WireMock (no extension) can and cannot do

- **Scenarios** (`scenarioName`/`requiredScenarioState`/
  `newScenarioState`) are a finite state machine: each *scenario name*
  holds exactly one current state string, starting at `Scenario.STARTED`.
  A stub can match on "scenario X is in state Y" and transition it to
  state Z. This can express a fixed *sequence* of canned responses for
  one named scenario (first call returns A, second call returns B) — it
  cannot express "one of N records, addressed by id, each independently
  mutable," because there is nowhere to put N records' worth of data. A
  scenario is a label, not a store.
- **Response templating** (Handlebars, bundled in the standalone jar —
  `"transformers": ["response-template"]` or `--global-response-
  templating`, no extra dependency) can render a response body from
  *that same request's* method/URL/headers/body
  (`{{request.path.[1]}}`, `{{request.body}}`, …). It has no access to
  data submitted by an *earlier, different* request. This means
  templating alone could make a single `create`'s response echo back
  what was just posted (cosmetically nicer, still not "appears in the
  next list") — not attempted here, because doing it for `create` only
  and not `list` risks reading as partial statefulness rather than the
  honest "none" this crate actually ships (see this feature's explicit
  instruction not to ship that under a stateful-sounding name).
- **Conclusion:** vanilla WireMock cannot make a `create` visible in a
  later `list`, or an `update` visible in a later `get`, full stop. This
  isn't a gap in this generator — it's a ceiling in the tool.

### 9.2 What a real per-record store needs: `wiremock-state-extension`

The official-adjacent (`github.com/wiremock` org) `wiremock-state-
extension` adds exactly the missing piece: a context-scoped store (one
`state` record or an append-only `list` of records per context key),
wired declaratively into stub JSON via `serveEventListeners`
(`recordState`, `deleteState`, list `addLast`/`deleteFirst`/
`deleteLast`/`deleteIndex`/`deleteWhere`) and a `state` Handlebars helper
to read stored properties back into a later response — genuinely
capable of "`POST /widgets` appends to a list; `GET /widgets` renders
that list; `GET /widgets/{id}` looks the id up in it." This is real,
evaluated capability, not a rejected-on-paper option — and it comes with
real costs:

- **A JVM classpath addition, not a config flag.** It ships as a JAR
  (`org.wiremock.extensions:wiremock-state-extension` via Gradle/Maven,
  or a standalone JAR alongside `wiremock-standalone`, or mounted into
  `/var/wiremock/extensions` in a container) — "generated WireMock
  stubs" stops meaning "drop JSON files next to any `wiremock/wiremock`
  image" and starts meaning "run *this* extended image/classpath." Every
  consumer of this generator's output (webank-mobile's e2e suite,
  `examples/react-vite-refine`, any future one) inherits that
  requirement.
- **No built-in pagination or filter/sort query language.** Both would
  have to be hand-rolled in Handlebars conditionals per model per field
  — for a *generated* stub, that's either an explosion of per-field
  template logic this crate would have to emit, or an honest admission
  that filtering/sorting/pagination stay unimplemented even with the
  extension (§7 already scopes those out regardless).
- **Concurrency: "the lock-level is basically the whole context store"**
  (the extension's own documented limitation) — fine for a single-writer
  dev/test loop (the refine.dev example's actual use case), a real
  constraint for anything higher-concurrency.
- **Instance-local, not distributed** — a restart or a second WireMock
  instance loses/splits the store; irrelevant for a local example, real
  for a shared CI mock.

### 9.3 Decision

This PR does **not** wire in `wiremock-state-extension`. Building the
per-model Handlebars templates it needs (`serveEventListeners` for
`create`, `state`-helper-driven bodies for `list`/`get`, delete
handling, all keyed correctly per model and per id) is a substantial
second generator inside this crate, not an incremental addition to the
happy-path static one — and it commits every consumer to a non-vanilla
WireMock deployment. Per this feature's own instructions, that's a
dependency-and-complexity tradeoff for the maintainer to weigh, not a
default to pick silently:

- **Go all-in on `wiremock-state-extension`.** Real create-then-list
  statefulness, at the cost of a Java extension dependency for every
  consumer and meaningfully more generator complexity (a second,
  stateful mapping shape alongside the static one, or a full
  replacement of it).
- **Stay static (what this PR ships).** No dependency change, works
  against any vanilla WireMock, honestly documented as non-stateful
  (§8.2, §9.1) — `examples/react-vite-refine` would need to design
  around that (e.g. a fixed seed set of records the UI reads/edits
  in-memory client-side, not a real create-then-list loop against the
  mock).
- **A bounded middle ground was considered and rejected as not
  actually solving the problem.** Pre-seeding a fixed set of `scenario`-
  backed "slots" that transition state on `update` could make *one*
  fixed record's `get` return a second canned body after a matching
  `PATCH` — but the second body still can't contain whatever the caller
  actually sent (§9.1's templating limitation), so it demonstrates "the
  mock changed", not "the mock reflects what you typed." Not worth the
  added generator complexity for that little payoff.

If the maintainer wants the extension-backed path, it's a follow-up on
top of this PR's static baseline, not a rewrite of it — the route
derivation, projection rules, and REST/RPC split in §8 stay valid; only
the response body/state-wiring layer would change.
