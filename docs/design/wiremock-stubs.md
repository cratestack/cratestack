# WireMock stub generation from `.cstack` schemas

Status: **v3 implemented** (`cratestack-mock-wiremock`, `cratestack
generate-wiremock`) — happy-path stubs for procedures **and** `model`
CRUD routes (`list`/`get`/`create`/`update`/`delete`). `transport rest`
model CRUD is **stateful** (§9) — real create-then-list/update-then-get/
delete-then-404 behavior, backed by `wiremock-state-extension`, verified
against a real WireMock instance built from `crates/cratestack-mock-
wiremock/docker/Dockerfile`. An `@version` model's `update`/`delete`
also enforce `If-Match` and `get`/`update` carry an `ETag` (§10) — the
same real-container verification standard. `transport rpc` model CRUD
and every procedure stay static/deterministic (§8, §9.4). Scope and open
questions below are current, not historical: several are deliberately
deferred, not resolved.
Scope: a new generator crate (`crates/cratestack-mock-wiremock`), a new CLI
subcommand (`generate-wiremock`), no changes to `cratestack-parser` or
`cratestack-macros`.
Tracking: issue #438; the `If-Match` gap (§10) surfaced as a trip-wire in
`examples/react-vite-refine` (PR #604, `feat/react-vite-refine-example`,
unmerged as of this section — see that PR's `tests/smoke.rs::
wiremock_stubs_do_not_validate_if_match_or_any_request_header` for what
needs reconciling once both land). Model CRUD (§8) motivated by the
`refine.dev` admin app, `packages/cratestack-refine`, and its planned
no-database example, `examples/react-vite-refine`.

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
- **`transport rpc` model CRUD statefulness, and list filtering/sorting/
  pagination under either transport.** See §9.7 for why each is still
  out of scope even now that `transport rest` model CRUD (§9) is
  stateful.

## 8. Model CRUD: route derivation and the static baseline

`model` blocks get five verbs per model — `list`, `get`, `create`,
`update`, `delete` — derived the same way the real server derives them,
not re-implemented by hand. Route derivation is identical for both
transports and both stateful and static bodies; what differs (§9) is
`transport rest`'s response is now backed by a real per-record store,
while `transport rpc` and every procedure stay on the deterministic,
static shape this section describes.

- **Route paths.** The REST plural segment is
  `cratestack_core::route_naming::model_route_segment(&model.name)` —
  imported directly from `cratestack-core`, not re-derived (see this
  repo's #345 history: two independent reimplementations of this exact
  function drifted apart before it was centralized). `list`/`create`
  share `{base}/{plural}` (`GET`/`POST`); `get`/`update`/`delete` share
  `{base}/{plural}/{id}` (`GET`/`PATCH`/`DELETE`). The `{id}` segment has
  no fixed value known at generation time, so those three routes are a
  WireMock `urlPathPattern` regex (`^{base}/{plural}/[^/]+$`, with
  `base` regex-escaped) instead of an exact `urlPath`, matching *any*
  id-shaped segment — under `transport rest` (§9), whether a request
  actually gets a `200` or falls through to WireMock's own `404` is then
  decided by a `customMatcher` against the state store, not by the route
  pattern alone.
- **RPC routes.** `transport rpc` schemas dispatch model verbs to
  `POST /rpc/model.<ModelName>.<verb>` (`generate_model_rpc_dispatch_arms`
  in `crates/cratestack-macros/src/transport/rpc.rs`) — five distinct
  exact paths, no `{id}` in the URL at all (it travels in the request
  body instead), so RPC stubs need no pattern matching. This is also
  *why* RPC model CRUD stayed on the static baseline rather than getting
  the stateful treatment — see §9.4.
- **Status codes.** `create` is `201`, matching
  `build_create_handler`'s literal `StatusCode::CREATED` — the one place
  a model stub's status differs from every procedure stub's `200`.
  `list`/`get`/`update`/`delete` are all `200`.
- **The static baseline's response bodies mirror the default
  projection, not a full model dump.** A `get`/`list`/`create`/
  `update`/`delete` response body excludes two field categories the real
  server's default projection (`crates/cratestack-macros/src/axum/model/
  serializers/projection_fields.rs`) also excludes: relation fields
  (fields whose type names another declared model — populated only via
  `include=<relation>`) and `@server_only` fields (never serialized to a
  client). Every other field is synthesized with the same deterministic
  per-scalar-type defaults procedures already use
  (`crates/cratestack-mock-wiremock/src/values.rs`, reused as-is). The
  stateful REST generator (§9.5) applies the identical relation/
  `@server_only` exclusion, on top of its own per-field statefulness
  rules.
- **`list` reuses the `Page<T>` envelope shape.** An `@@paged` model's
  `list` response is `{items, totalCount, pageInfo}` — the identical
  shape `values.rs` already synthesizes for a procedure returning
  `Page<T>`; a non-`@@paged` model's `list` is a bare JSON array. True
  for both the static baseline and the stateful generator.

### 8.1 The static baseline's real output (`transport rpc`)

Run against a `transport rpc` copy of
`crates/cratestack-client-dart/tests/fixtures/ci_rest.cstack`'s models —
this is the shape every RPC model route, and every procedure regardless
of transport, still uses:

`mappings/model.Post.get.json`'s body — `author` (the relation back to
`Author`) is absent, every scalar field is present:

```json
{ "authorId": 0, "id": 0, "published": true, "status": "draft", "title": "string" }
```

`mappings/model.Post.list.json` — `@@paged`, so the envelope:

```json
{
  "items": [{ "authorId": 0, "id": 0, "published": true, "status": "draft", "title": "string" }],
  "pageInfo": { "hasNextPage": false, "hasPreviousPage": false, "limit": null, "offset": null },
  "totalCount": 1
}
```

`--check` round-trips cleanly, and correctly flags drift: adding a field
to a model and re-running `--check` without regenerating reports every
affected file as modified.

### 8.2 What the static baseline deliberately doesn't do

- **No query-string assertion or variation.** See §7 — a `list` stub
  answers the same body no matter what `field__operator=`/`sort`/
  `limit`/`offset`/`fields`/`include` values a request sends. Still true
  under the stateful generator too — see §9.7.
- **No request-body assertion on `create`/`update`.** Same v1 precedent
  as procedures (§4) — any body is accepted (the stateful generator
  *reads* the body to decide what to echo/store, but still never
  rejects a request for its shape).
- **No per-record statefulness.** `transport rpc` model CRUD, and every
  procedure, always answers the same synthesized example regardless of
  what was previously created/updated/deleted through it — by design
  (§9.4), not because nobody got around to it.

## 9. Model CRUD statefulness — investigated, decided, and built

The motivating consumer for model CRUD stubs is `packages/
cratestack-refine`, a refine.dev admin app driven entirely by model
CRUD, and a planned example
(`examples/react-vite-refine`) that runs a Vite + refine app against
these stubs with **no database**. For that example to be a convincing
demo rather than "a static JSON blob wearing a mock server's clothes",
a record created through the app should appear in the next `list`, and
an update should be visible on the next `get`. §9.1–§9.2 are the
investigation into whether that's achievable, done *before* building
anything. §9.3 on is what happened after: the maintainer read that
investigation and chose to go ahead with the dependency it surfaces;
§9.4–§9.6 are the build, a real (and initially unpublished-anywhere)
packaging bug found along the way, and the evidence it now actually
works, verified against a real WireMock instance.

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

### 9.3 The maintainer's decision

§9.1–§9.2 were reported before anything was built, with three options
laid out (go all-in on the extension; stay static; a bounded
scenario-based middle ground, evaluated and rejected as not actually
solving the problem — its "second canned body" still can't contain
whatever the caller sent, same templating ceiling as §9.1). The
maintainer's call: **go all-in on `wiremock-state-extension`** for
`transport rest` model CRUD. §9.4 onward is that build.

### 9.4 A real compatibility bug — and what actually works

Building the stateful generator surfaced a second, more concrete
question §9.1–§9.2 couldn't answer from documentation alone: does the
extension's *published* artifact even work against a real
`wiremock/wiremock` deployment? Tested by hand, in this order, each
against a real container:

1. `wiremock-state-extension:0.10.1` (Maven Central) + `wiremock/
   wiremock:3.9.1` — `AbstractMethodError` on the very first request
   that renders a `{{state ...}}` helper: `StateHandlerbarHelper does
   not define or inherit an implementation of ... Helper`.
2. The same extension version + `wiremock/wiremock:3.13.2` — identical
   error.
3. `wiremock-state-extension:0.7.0` + `wiremock/wiremock:3.7.0` — the
   *extension's own* `compatibility_test` module's declared "known
   good" pairing. A **different** error this time
   (`NoSuchMethodError` in the extension's own `recordState` listener,
   before a single `state` helper is even rendered) — worse, not
   better.

This is a real, independently-corroborated upstream defect, not a
version-pinning mistake here: `wiremock/wiremock-state-extension`
issue #36 is the identical `AbstractMethodError`, filed by another user
via the identical deployment (`docker run wiremock/wiremock` +
volume-mounted extension jar), confirmed by the extension's own
maintainer as "the package relocation was wrong" — and, per a later
comment on that same issue, it recurred for someone else after a
supposed fix. Root cause: every `wiremock/wiremock` distribution
*relocates* its bundled Handlebars (`com.github.jknack.handlebars` →
`wiremock.com.github.jknack.handlebars`); the extension's plain Maven
Central jar is compiled against the *unrelocated* package, so its
Handlebars `Helper` implementations don't match the ABI the relocated
runtime expects.

**The extension's own `build.gradle` already has the fix** — a
`shadowJar` Gradle task that relocates `com.github.ben-manes.caffeine`
and `com.github.jknack` the same way WireMock's own distribution does.
Building it from the pinned source commit
(`0d9fff0554319bc5e62310137a6b225a9760e002`, tag `0.10.1`) and loading
*that* jar into `wiremock/wiremock:3.13.2` (the WireMock version that
exact commit's `build.gradle` targets) works — verified end to end,
§9.6. The catch: **that `shadowJar` artifact is never published
anywhere.** Its own release workflow (`.github/workflows/release.yml`)
runs `gradle publish`, which ships only the plain unshaded `jar` to
Maven Central — the `shadowJar` only exists as a same-run, unauthenticated,
90-day-expiring CI build artifact (`build-and-test.yml`'s
`actions/upload-artifact`), not something a downstream consumer (or a
code generator) could fetch and pin.

So "wire in the extension" turned out to mean something more specific
than "add a Maven dependency": **build the correctly-shaded jar from
pinned source as part of standing the mock up.**
`crates/cratestack-mock-wiremock/docker/Dockerfile` does exactly that —
a multi-stage build cloning the extension at the pinned commit, running
`./gradlew shadowJar`, and layering the result into a
`wiremock/wiremock:3.13.2` base image. See that file's own header
comment for the full pinning rationale. This is real, reproducible cost
this design doc's own instructions asked to be written down plainly: a
consumer of the stateful stubs needs to build (or be handed a
pre-built) custom image, not `docker run wiremock/wiremock`.

### 9.5 Design

- **One shared-list context per model, keyed by the plural route
  segment** (e.g. `posts`) — `wiremock-state-extension`'s `list`
  operations (`addLast`, `deleteWhere`) live here; `list` renders it via
  `{{#each (state context='posts' property='list' default='[]')}}`. A
  list entry stores a **pointer** to its record's per-record context
  (`__cratestack_record_context`), not a denormalized copy of every
  field — `list` follows the pointer back with the same `context=`
  lookup `get`/`delete` use, per record, on every render. §9.8 explains
  why: an earlier version stored a full copy in the list and kept it in
  sync on every `update`, which is exactly what corrupted under
  concurrent writes to one record.
- **One per-record context per record, keyed by `request.path`** (e.g.
  `/api/posts/42`) — not a hand-built `"<plural>:<id>"` string, because
  this templating stack has no string-concatenation Handlebars helper
  (`{{concat}}` doesn't exist here; confirmed by hand) and `request.path`
  is already exactly the right unique key, for free, on every REST
  detail request. `create` doesn't have an inbound `request.path` for
  the *new* record yet, so it builds the identical string from the
  known detail-route prefix plus the id it just generated
  (`"{list_path}/{{jsonPath response.body '$.id'}}"`).
- **`get`/`update`/`delete` are gated by a `state-matcher` `customMatcher`**
  (`hasContext: "{{request.path}}"`), `priority: 1`. A request for an id
  that was never created, or was already deleted, matches no stub at
  all — WireMock's own 404, not something this generator has to emit.
- **Every write listener harvests fields from `response.body`, never
  recomputes from `request.body`.** A generated id
  (`{{randomValue ...}}`) or a merge-or-fallback expression would
  silently produce a *second, different* value if evaluated again in a
  `serveEventListeners` parameter instead of read back from what the
  response already rendered.
- **Per-field type classification decides what's stateful at all**
  (`crate::model_attrs::ScalarKind`): `Required`-arity `Int`/`Float`
  (unquoted number), `Boolean` (unquoted bool), `String`/`Cuid`/`Uuid`/
  `DateTime`/enum (quoted string) round-trip through the store.
  `Optional`/`List` arity, `Json`/`Bytes`/`Vector(n)`, and nested `type`
  references don't — those fields render the same static example
  (`values::synthesize`, unchanged) on every response, frozen, same as
  the v1 baseline. Relation and `@server_only` fields are excluded
  entirely, as before.
- **A leading-zero id would break real clients.** `{{randomValue
  length=6 type='NUMERIC'}}` can start with `0`, and a bare (unquoted)
  `084839` is not valid JSON — confirmed by hand (`json.loads` rejects
  it; roughly 1 in 10 generated ids hit this). Fixed by hard-coding the
  first digit non-zero (`1{{randomValue length=5 type='NUMERIC'}}`),
  trading uniform randomness (irrelevant for a mock) for guaranteed
  validity.
- **Handlebars won't parse `}}}`.** A bare (unquoted) numeric/boolean
  expression immediately followed by a JSON `}` produces exactly that
  three-brace sequence, which `handlebars.java`'s parser rejects as
  ambiguous — confirmed by hand. Every hand-assembled object/array in
  this generator pads a space around braces/brackets
  (`{ "a": 1 }`, not `{"a":1}`) to avoid it.

### 9.6 Real verification

Built `docker/Dockerfile` (§9.4) into a real image and ran it against
mappings generated from `crates/cratestack-client-dart/tests/fixtures/
ci_rest.cstack` by the actual CLI — not hand-written stubs, not a JSON-
shape assertion. The three required behaviors, and more:

| Step | Result |
|---|---|
| `GET /api/posts` (empty) | `200`, `{"items":[],"totalCount":0,...}` |
| `POST /api/posts` `{"title":"final check","status":"published","published":true,"authorId":3}` | `201`, echoes every field back with a generated `id` |
| `GET /api/posts` again | `200`, **contains the just-created record** |
| `PATCH /api/posts/{id}` `{"title":"patched"}` | `200`, `title` updated, every other field unchanged |
| `GET /api/posts/{id}` again | `200`, **shows the patched `title`** |
| `DELETE /api/posts/{id}` | `200`, the pre-delete snapshot |
| `GET /api/posts/{id}` again | **`404`** — not the pre-delete body |
| `GET /api/posts` again | `200`, `{"items":[],"totalCount":0,...}` — empty again |
| `PATCH /api/posts/{unknown-id}` | `404` — a customMatcher-gated write verb 404s too, not just reads |
| 25× `POST /api/posts` in a row | all 25 responses valid JSON (the leading-zero-id fix from §9.5, confirmed under repetition) |
| A model with an `Optional String`/`Json` field | those two fields render the identical frozen literal on `create`/`get`/`update`/`delete`; the model's own `Required String` field is genuinely stateful in the same responses |

`--check` still round-trips cleanly both ways: the generated *files* are
byte-identical across runs (the Handlebars *text* is fixed even though
what it renders to at request time isn't), and a schema change without
regeneration is still correctly flagged as drift.

### 9.7 What's still not stateful, even with the extension

- **`transport rpc` model CRUD.** The extension's per-record context
  needs something unique per request; REST gets that for free
  (`request.path`), RPC doesn't (the id lives in the request body, and,
  per §9.5, there's no concatenation helper to build a unique key from
  it without one). Scoped out rather than accepting a cross-model id
  collision risk on a hand-wavy "probably fine" basis — see
  `crate::model_mapping::rpc`'s module doc.
- **List filtering, sorting, and pagination.** Still not implemented,
  stateful or not (§8.2) — the extension has no built-in query language
  for its `list` operations, and hand-rolling per-field/per-operator
  Handlebars conditionals for a *generated* stub was judged not worth
  the complexity for what §7 already scoped out. A `list` response is
  always the complete, unfiltered collection.
- **Fields outside `ScalarKind`'s four cases** (§9.5) — `Optional`/
  `List` arity, `Json`/`Bytes`/`Vector(n)`, nested `type` references —
  render a fixed example on every response, same as the pre-stateful
  baseline, never reflecting what was actually created/patched.

### 9.8 cratestack#588 follow-up: three correctness defects, found and fixed

An adversarial review of the shipped §9.5–§9.7 design found three real
defects — each reproduced by hand against the real
`docker/Dockerfile` image before being fixed, not inferred from reading
the templates.

**1. Falsy values were silently dropped on `create`/`update`
(HIGH).** `merge_or_fallback`'s original form was
`{{#if (jsonPath request.body '$.field')}}<new>{{else}}<fallback>{{/if}}`
— Handlebars' `#if` is a truthiness test, and JSON `false`/`0`/`""` are
all falsy, so `#if` treated a present-but-falsy field the same as an
absent one. Reproduced: `PATCH {"count":0}` on a stored `count:5` left
`5`; `{"active":false}` left `true`; `{"name":""}` left the prior name.
**A mock consumer could never zero a counter, toggle a boolean off, or
clear a string** — worse than a static stub, because it looks like it
worked.

Fixed by presence-testing instead of truthiness-testing:
`jsonPath ... default=SENTINEL` (a distinctive constant no real field is
expected to send) returns the field's real value when present and the
sentinel only when the key is absent; `eq` (handlebars.java's bundled
`ConditionalHelpers`, confirmed present in the real extension image)
compares the two without erroring regardless of the returned value's
JSON type. See `src/model_state/fragments.rs`'s `merge_or_fallback` doc
comment for the exact expression and the decisive cases confirmed by
hand: `0`, `false`, `""`, an absent key, and explicit JSON `null`.
**Explicit `null` is deliberately treated the same as "absent"** (falls
back to the prior value, not stored literally) — every field this
helper wraps is `Required` arity by definition (`Optional` fields are
never stateful, they're frozen), so there is no valid `null` state for
a `Required` field to move into; this also falls out of the fix for
free, since the extension's `jsonPath ... default=` fires for an
explicit `null` leaf the same as a missing path, confirmed by hand.

**2. Concurrent updates to one record corrupted the shared list
(HIGH).** `update_listeners` performed
`recordState(record) → deleteState(list, deleteWhere id) →
recordState(list, addLast)` against the model's one shared list
context. `wiremock-state-extension`'s own README states plainly:
"single updates to contexts... are atomic on instance level" but
"concurrent requests are currently allowed to change the same
context... the context can change while a request is performed" — i.e.
no transaction spans a multi-step sequence — and list entries "cannot
be modified (only read/deleted)", so there is no atomic "replace this
one entry" primitive available at all (checked in the extension's
source before concluding this, not assumed).

Reproduced by hand, three separate runs against the real container:
300/400/500 concurrent `PATCH`es to **one** record, all `200`s, left
**15 / 9 / 8 duplicate stale rows** for that same id in
`GET /api/widgets`. No `500`s were reproduced in this environment (the
originally reported repro also saw ~11
`ConcurrentModificationException` `500`s at similar concurrency — timing-
and load-dependent, and not required to establish the defect: duplicate
corrupted rows are damning on their own).

Since no atomic multi-step primitive exists, the fix removes the shared
list from `update`'s write path entirely rather than trying to make the
non-atomic sequence "safer": a list entry now stores a pointer to its
record's per-record context (§9.5) instead of a denormalized copy of
every field, so `list` always reads the current, authoritative
per-record state at render time. `update` therefore only ever performs
the one write every other stateful mutation already relies on being
atomic — `recordState` against a single per-record context — and never
touches the shared list at all. Re-running the identical load test
against the fixed generator: **0 duplicates, 0 errors**, across three
runs at 300/400/500 concurrent `PATCH`es to one record (`list length`
after the run was always exactly `1` for that id). A full create → list
→ update → list → delete → list lifecycle was also re-verified against
the real container to confirm the pointer indirection didn't break
normal (non-concurrent) behavior.

`create` and `delete` still each touch the shared list exactly once (an
`addLast`, a `deleteWhere`) — the same non-atomic-multi-step category
of risk technically still applies to a `create`/`delete` race on the
*same* id, but id generation makes that collision astronomically
unlikely in practice, and it was not the reported failure mode. Load-
tested anyway rather than left as an assumption: 300 concurrently
created records, then all 300 deleted concurrently (different ids
contending for the same shared list context) — `0` errors, `0` records
remaining. A narrower same-id double-delete race (50 concurrent
`DELETE`s against one already-deleted id) also produced no `500`s or
corruption (`200`/`404` only, as expected from the `state-matcher` gate
racing the delete itself) — a residual, much smaller-window risk than
the fixed `update` case, documented here rather than further reduced,
since it was not the reported defect and the extension still offers no
stronger primitive to reduce it with.

**Bottom line for anyone relying on this mock**: single-writer or
low-concurrency dev/test use (the documented, intended use case — see
§9.2) is unaffected either way. High-concurrency *writes to the same
record* are now safe against list corruption (defect 2 is fixed); the
extension's own instance-level, non-distributed, non-transactional
design (§9.2) remains a real ceiling this generator cannot lift.

**3. Colliding pluralized route segments served silently wrong data
(HIGH).** `model Bus` and `model Buse` both route to `/api/buses`:
`to_snake_case` gives `bus`/`buse` (distinct — the pre-existing
`validate_model_name_collisions` check passes both), but `pluralize`
gives `buses` for both (`bus` -> `buses` via the "ends in `s`, append
`es`" rule; `buse` -> `buses` via the plain "append `s`" rule). No
error at generation time; `Buse`'s stub was masked by `Bus`'s matcher,
and — worse — both models shared one state pool
(`crates/cratestack-mock-wiremock/src/model_state.rs`'s per-model list
context is keyed by the plural route segment alone), so
`POST /api/buses {"driver":"Alice"}` returned `Bus`'s shape with
`driver` silently dropped. Confirmed the real Axum server hits the
identical collision — `axum::Router::route` panics at startup on an
exact path/method overlap, and the server's route registration uses the
identical `pluralize(to_snake_case(...))` composition
(`cratestack-macros/src/axum/model/routes.rs`) — so this was never a
mock-only gap.

**Fixed in the parser, not the mock generator.** Root cause was
`crates/cratestack-parser/src/validate/snake_case_collisions.rs`'s
`validate_model_name_collisions`, which only ever compared
`to_snake_case(name)`, never the pluralized route segment two distinct
snake_case forms can still collide on. Added
`validate_model_route_collisions`, comparing
`cratestack_core::route_naming::model_route_segment` (the exact
function both the real server's route registration and this generator
already call) instead. Considered the narrow alternative — reject only
inside `cratestack-mock-wiremock::generate_package` — and rejected it:
the real server hits the identical panic, so a mock-only guard would
leave the actual production bug (a server that panics at startup on a
schema its own parser accepted) unfixed for everyone except this one
generator's callers. The parser fix's blast radius was checked before
landing it, not assumed: every `.cstack` file in this repository (154
files, 205 `model` declarations) was scanned for a route-segment
collision the existing `to_snake_case`-only check would have missed —
zero found, and the full `cratestack-parser` test suite (223 tests)
still passes unmodified. `model_state.rs`'s previous comment asserting
"no two models' plurals can collide" was false (that was the bug) and
has been rewritten to state the actual invariant: the parser rejects
this schema-wide before a `Schema` value can reach this generator at
all, so this crate relies on — rather than re-derives — that guarantee.

## 10. If-Match / optimistic locking — investigated, decided, and built

Through §9.8, a model's `@version` field was just another `Required Int`
field to this generator — echoed/merged like `count` or `age`, with no
connection to `If-Match` at all. A stale `PATCH` returned `200`. This
section covers closing that gap: mirroring
`crates/cratestack-axum/src/headers/etag.rs::parse_if_match_version`,
`crates/cratestack-sqlx/src/query/write/update.rs`'s
`version = version + 1` / `WHERE version = $expected`, and
`CoolError::status_code`'s 4xx mapping, for `update`/`delete` on a
versioned model, plus an `ETag` on `get`/`update` responses.

### 10.1 The question: can `wiremock-state-extension`'s `state-matcher`
compare a *request header* against *stored per-record state*?

Every existing `customMatcher` use in this generator (§9.5) only ever
checks `hasContext` — "does a record exist at this path" — never a
property's *value*. The extension's own README documents `property`
matching (comparing a stored property against a `StringValuePattern`)
and separately documents that `hasContext`/`hasNotContext` accept
templated values, but never says whether `property`'s own matcher
*value* is templated, and never shows an example comparing a stored
value against anything from the request itself.

Read `StateRequestMatcher.java` at the pinned commit
(`docker/Dockerfile`'s `WIREMOCK_STATE_EXTENSION_COMMIT`) to settle it:
`calculateMatch` calls
`it.getKey().evaluate(context, renderTemplateRecursively(model, it.getValue()))`
— the whole `property` parameters map (including nested `equalTo`/`not`
values) is run through `renderTemplateRecursively`, which applies
WireMock's own `TemplateEngine` (the same one `response-template` and
`hasContext`/`hasNotContext` already use) to every string leaf, *before*
`ContextMatcher.property`'s evaluator ever runs `patterns.match(storedValue)`.
So a matcher value like `{{regexExtract request.headers.If-Match '[0-9]+' ...}}`
renders to the header's digits first, and *that* rendered string is what
gets compared against the stored `version` property. No extension change
needed — this was reachable the whole time, just undocumented.

Confirmed by hand against the real `docker/Dockerfile` image (not just
read from source) with a hand-built `POST`/`GET`/five-`PATCH`-case
mapping set before touching the generator at all: absent/`*`/malformed/
stale/current `If-Match` headers each hit the intended stub and only
that stub, `version` state correctly bumped via WireMock's bundled
`math` helper (`{{math (state context=... property='version') '+' 1}}`
— jknack `NumberHelpers`, part of the same `response-template`
transformer, not something the state extension itself adds), and the
resulting `ETag` round-tripped correctly across `GET` → `PATCH` →
stale-`PATCH`.

### 10.2 Design

For a model with no `@version` field: **completely unaffected** — no
code path in `crate::model_state::version_gate` is ever reached, and
`build_stateful_rest_mappings` keeps building exactly the same single
`hasContext`-only stub per verb it always did. Confirmed with a byte-
for-byte diff: the same schema generated with and without this change's
code produces an identical `mappings/` directory when the model has no
`@version` field.

For a model that declares `@version`, `update`/`delete` each fan out
from one stub into five, gated in ascending WireMock `priority` (lower
number wins first):

| # | Case | Native WireMock header match | `state-matcher` `property` check | Status |
|---|---|---|---|---|
| 1 | `If-Match` absent | `{"If-Match": {"absent": true}}` | none | `412` |
| 2 | `If-Match: *` | `{"If-Match": {"equalTo": "*"}}` | none | `400` |
| 3 | present, not a strong quoted ETag | `{"If-Match": {"doesNotMatch": "^\"-?[0-9]+\"$"}}` | none | `400` |
| 4 | well-formed, stale | `{"If-Match": {"matches": "^\"-?[0-9]+\"$"}}` | `version not equalTo <header digits>` | `412` |
| 5 | well-formed, current | same as #4 | `version equalTo <header digits>` | the real response |

Priority only has real disambiguating work to do at one seam: `*` also
satisfies case 3's `doesNotMatch` (it isn't a strong ETag either), so
case 2 must outrank case 3 — confirmed by hand, then encoded as
`priority: 2` vs `priority: 3` rather than left to declaration-order
luck. Every other pair is mutually exclusive by header content alone (a
header is present or absent; a present value matches the strong-ETag
regex or it doesn't), so priority is otherwise inert defense in depth.
`<header digits>` is `{{regexExtract request.headers.If-Match '[0-9]+' default='...'}}`
— the stored `version` property is always the bare integer (never
quoted internally, only at the HTTP `ETag`/`If-Match` boundary), so
comparing it against the header's digits directly avoids needing the
extension to understand quoting at all.

Case 5's response is what `update`/`delete` always rendered before this
change, plus:

- **`version` is no longer an ordinary echoed/merged field.** `create`
  always seeds it at the literal `0` (mirroring `create_exec.rs`'s
  server-side seed — a real `Create<M>Input` never carries `@version`
  at all, so nothing about it is ever taken from the request body).
  `update`'s success case always renders the *stored* version plus one
  via `{{math (state context=... property='version') '+' 1}}` (mirroring
  `update_exec.rs`'s `version = version + 1` — a real
  `UpdateModelInput` never carries it either). Both values are then
  harvested back into `recordState` from the already-rendered response
  body (`{{jsonPath response.body '$.version'}}`), same as every other
  field — never recomputed a second time (§9.5's own harvesting
  principle, unchanged).
- **`get` and `update`'s success case gain an `ETag: "<version>"`
  response header** (`crates/cratestack-axum/src/headers/etag.rs::
  set_version_etag`'s exact quoted-integer format) — `get`'s is the
  current stored version, `update`'s is the post-bump one, matching
  `get_etag_apply`/`update_etag_apply` in `crates/cratestack-macros/
  src/axum/model/prep/etag.rs` exactly. **`delete` never gets one** —
  there's no `delete_etag_apply` token in the real codegen, only
  `delete_if_match_apply`, so a deleted record's response correctly
  advertises nothing. **`create` never gets one either** (no
  `create_etag_apply` token exists at all).

Error bodies mirror `cratestack_core::CoolErrorResponse`'s exact wire
shape (`{code, message, details}`, no extra wrapper —
`crates/cratestack-axum/src/transport/http_transport.rs` serializes
this directly for REST), with `code`/`message` matching the real
`CoolError` variant and text as closely as a static-per-case mock
reasonably can. One simplification, stated plainly rather than silently
approximated: `parse_if_match_version` gives two different messages for
"not quoted at all" vs "quoted but not an integer" (both `400`); this
generator's case 3 uses one message for both, since the two collapse to
the identical `doesNotMatch` regex check and a mock's request-header
matching has no way to re-run the real function's own two-step parse to
tell them apart without hand-rolling that parse in Handlebars.

### 10.3 Real verification

Built `docker/Dockerfile` into a real image, generated `mappings/` from
a real `.cstack` schema (`model Widget { id Int @id; name String;
version Int @version }`) via the actual `cratestack generate-wiremock`
CLI, mounted it into the container, and drove real HTTP:

| Request | Response |
|---|---|
| `POST /api/widgets {"name":"gadget"}` | `201`, `{"id":147110,"name":"gadget","version":0}` |
| `GET /api/widgets/147110` | `200`, `ETag: "0"` |
| `PATCH` (no `If-Match`) | `412`, `{"code":"PRECONDITION_FAILED","message":"If-Match header required",...}` |
| `PATCH` `If-Match: *` | `400`, `{"code":"BAD_REQUEST","message":"If-Match: * is not supported on versioned models",...}` |
| `PATCH` `If-Match: bogus` | `400`, `{"code":"BAD_REQUEST","message":"If-Match must be a strong ETag of the form \"<integer>\"",...}` |
| `PATCH` `If-Match: W/"0"` (weak) | `400`, same message — a weak validator is not a strong one |
| `PATCH` `If-Match: "99"` (stale) | `412`, `{"code":"PRECONDITION_FAILED","message":"version mismatch: expected 99, found 0",...}` |
| `PATCH` `If-Match: "0"` `{"name":"renamed"}` | `200`, `ETag: "1"`, `{"id":147110,"name":"renamed","version":1}` |
| `PATCH` again, `If-Match: "0"` (now stale) | `412`, `"version mismatch: expected 0, found 1"` |
| `GET /api/widgets/147110` | `200`, `ETag: "1"`, shows `"renamed"` |
| `DELETE` `If-Match: "0"` (stale) | `412` |
| `DELETE` (absent) | `412` |
| `DELETE` `If-Match: "1"` (current) | `200`, no `ETag` header, pre-delete snapshot |
| `GET /api/widgets/147110` again | `404` (WireMock's own — no stub's `hasContext` matches) |

The full round trip (`GET` → take `ETag` → `PATCH` with it → success →
`PATCH` again with the now-stale value → `412`) is proven end to end by
rows 2, 8, and 9 above.

**Non-versioned models are unaffected — proven, not just argued.**
Generated the identical schema minus `@version` twice: once against
this change's generator, once against the pre-change generator
(`git stash`). `diff -r` on the two `mappings/` directories: identical.
Ran the pre-change-shaped stubs live too: `PATCH` with no `If-Match`
header at all against a plain model still returns `200` — the mock
never grew a header requirement it shouldn't have.

`--check` still round-trips cleanly both directions: a freshly generated
directory reports no drift, and deleting one of the new
`update-if-match-*.json` files and re-running `--check` correctly
reports it as drift (`missing: mappings/model.Widget.update-if-match-
stale.json`).

**Test counts.** `cargo test -p cratestack-mock-wiremock`: 32 tests
before this change (3 unit + 14 `models.rs` + 14 `procedures.rs` + 1
doctest), 40 after (+8 new in `tests/models_if_match.rs`). Reverting
just `src/` (keeping the new test file) and re-running: 6 of the 8 new
tests fail red, confirming they exercise real new behavior rather than
restating an existing invariant; the other 2
(`non_versioned_model_emits_no_header_matcher_anywhere`,
`versioned_model_generation_is_deterministic`) are intentionally
regression/invariant tests that are *supposed* to hold both before and
after — they weren't counted toward the 6.

### 10.4 What this doesn't cover

- **`transport rpc` model CRUD.** Still fully static (§8, §9.7) — no
  `If-Match` handling of any kind, versioned model or not. The same
  "no unique-per-request key" limitation that keeps RPC out of §9's
  statefulness applies equally here: there is no per-record context to
  gate a header check against in the first place.
- **The two distinct real 400 messages for a malformed `If-Match`
  collapse to one** in this mock (§10.2's last paragraph) — a real
  client only needs the status code and rough shape to test against a
  mock, and the two real messages differ only in wording, not meaning.
- **A negative stored version is impossible in practice** (the real
  server never produces one — `version` only ever counts up from `0`),
  but `STRONG_ETAG_PATTERN` accepts a leading `-` at the shape-check
  level anyway, for the same reason `parse_if_match_version`'s own
  `i64::parse` does: rejecting it would be an extra rule this generator
  invents that the real header parser doesn't have. A negative
  `If-Match` simply can never equal a non-negative stored version, so it
  always falls through to the "stale" case — never silently accepted.
