# WireMock stub generation from `.cstack` schemas

Status: **v1 implemented** (`cratestack-mock-wiremock`, `cratestack
generate-wiremock`) — happy-path stubs for procedures under `transport
rest`/`transport rpc`. Scope and open questions below are current, not
historical: several are deliberately deferred, not resolved.
Scope: a new generator crate (`crates/cratestack-mock-wiremock`), a new CLI
subcommand (`generate-wiremock`), no changes to `cratestack-parser` or
`cratestack-macros`.
Tracking: issue #438.

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
  `POST /rpc/<name>` (`cratestack_core::rpc::RPC_UNARY_PATH`) instead.
  Either way, one route per procedure, always `POST`, body in and body out
  with no envelope.
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
  five REST routes each (`generate_model_transport_constants` in the same
  file): `GET`/`POST /<plural>`, `GET`/`PATCH`/`DELETE /<plural>/{id}`,
  `POST` returning `201` instead of `200`. Out of scope for v1 (see §7) —
  procedures were both the simpler case and webank's actual motivating
  case (`crates/bff`'s schema is `provider = "none"`: procedures only, no
  models, by construction — see `docs/design/no-database-mode.md`).
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

One file per procedure, `mappings/<procedureName>.json` — `mappings/` is
the directory name a WireMock instance scans by convention, so `--out` can
point directly at a project's existing WireMock root.

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

## 7. Explicitly out of scope for v1 (tracked, not forgotten)

- **`model` blocks' REST CRUD routes.** Five routes per model, list
  pagination (`Page<T>`, already handled for procedure return types, is
  reusable), a `201` on create, and no request body to synthesize for
  `GET`/`DELETE`. Procedures were the motivating case (webank's schema has
  none); models are a real follow-up once there's a concrete consumer.
- **`transport grpc` schemas.** Rejected up front
  (`WireMockGeneratorError::UnsupportedTransport`) — WireMock stubs a
  JSON/HTTP wire shape, not protobuf-over-HTTP/2; grpc needs a different
  mock target (a gRPC mock server, or `grpcurl`-style fixtures), not an
  extension of this crate.
- **Error-case stubs, request-body matching, auth emulation.** See §4.
- **`FindMany<T>` return types.** Schema validation already forbids
  `FindMany<T>` anywhere outside a procedure *argument* position, so this
  is defense-in-depth in `values.rs`, not a real gap in practice — kept as
  a real error rather than an `unreachable!()` only because this crate's
  public API takes `&Schema` directly, so an unvalidated or hand-built
  schema could still reach it.
