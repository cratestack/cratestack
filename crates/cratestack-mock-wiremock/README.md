# cratestack-mock-wiremock

Generates [WireMock](https://wiremock.org/) stub mappings from a parsed `.cstack` schema's procedures and models, so integration and e2e tests can run against a mock backend whose wire contract is derived from the same schema the real server is generated from — instead of a hand-maintained JSON fixture that can silently drift from it.

See the design doc, `docs/design/wiremock-stubs.md`, for the motivating case (ADORSYS-GIS/webank-mobile's 37 hand-maintained WireMock mapping files), the full design, and the "Model CRUD statefulness" section's investigation before you rely on the stateful behavior below.

## Scope (v3)

- Covered: `procedure`/`mutation procedure` declarations (always static/happy-path), and `model` blocks' `list`/`get`/`create`/`update`/`delete` CRUD routes.
- **`transport rest` model CRUD is stateful.** A record created through a mocked `create` appears in a subsequent `list`; a `PATCH` is visible on a subsequent `get`; a `delete`d record's `get` returns `404`, not a stale body. This is backed by a real per-record store (`wiremock-state-extension`), **not** a fixed example replayed on every request — but it costs something to run. See "Running the stateful stubs" below before you build anything on top of this.
- **A model with `@version` gets real `If-Match` optimistic locking, mirroring the real server byte-for-byte.** `update`/`delete` require a strong quoted-integer `If-Match` matching the record's current version: absent → `412`, `If-Match: *` → `400`, malformed → `400`, a well-formed but stale value → `412`, the current value → succeeds and bumps the stored version. `get`/`update` responses carry a matching `ETag: "<version>"` header (`delete`/`create` never do — the real server doesn't either). A model with no `@version` field is completely unaffected — no header matching of any kind is emitted for it. See "If-Match / optimistic locking" in the design doc for the `wiremock-state-extension` capability investigation this is built on.
- **`transport rpc` model CRUD stays static** (the pre-v3 shape — one deterministic example, replayed identically on every request, works against any vanilla WireMock). The extension's per-record store needs something unique to each request that REST gets for free (the id-bearing URL path) and RPC doesn't (the id lives in the request body, and this templating stack has no string-concatenation helper to build a unique key from it).
- **List filtering, sorting, and pagination are not implemented**, stateful or not. Every `list` response is the complete, unfiltered collection regardless of `field__operator=value`/`sort`/`limit`/`offset` in the query string — a stateful `list` reflecting *some* of a request's query params and silently ignoring the rest would look like it worked and wasn't tested, which is worse than an honestly-complete response.
- **Fields this generator can't round-trip through the state store fall back to a fixed value.** `Optional`/`List`-arity fields, `Json`/`Bytes`/`Vector(n)`, and any nested `type` reference render the same static example on every response, never reflecting what was created/patched — only `Required`-arity `String`/`Cuid`/`Uuid`/`Int`/`Float`/`Boolean`/`DateTime`/enum fields are genuinely stateful. A relation field (populated only via `include=<relation>`) and an `@server_only` field are excluded entirely, same as before.
- Not covered (tracked as follow-ups in the design doc): `FindMany<T>` return types (schema validation already forbids these outside a procedure argument position, so this is defense-in-depth rather than a real gap), error-case stubs, request-body assertion, and any emulation of the auth chokepoint every procedure/model route sits behind.

## Installation

This is a build-time crate, typically invoked through the CLI:

```bash
cratestack generate-wiremock \
  --schema schemas/catalog.cstack \
  --out wiremock \
  --base-path /api
```

This writes one file per procedure under `wiremock/mappings/<procedureName>.json`, and five files per model under `wiremock/mappings/model.<ModelName>.<list|get|create|update|delete>.json` — thirteen instead, for a `transport rest` model that declares `@version`: `update`/`delete` each fan out into `<verb>.json` (success) plus `<verb>-if-match-required.json`/`-if-match-wildcard.json`/`-if-match-malformed.json`/`-if-match-stale.json` (see "If-Match / optimistic locking" above). `mappings/` is the directory a WireMock instance scans by convention, so `--out` can point directly at a project's existing WireMock root (alongside a hand-maintained `__files/` directory, if any).

Pass `--check` to run in drift-detection mode: generate in memory and diff against `--out` instead of writing, exiting non-zero and listing the files that differ. Wire this into CI the same way `generate-dart --check`/`generate-typescript --check` already are. This still works exactly the same way for the stateful stubs: the generated *file content* (Handlebars template text) is fully deterministic even though what it renders to at request time isn't — two `generate_package` calls against an unchanged schema are byte-identical.

To call the generator from Rust:

```toml
[dependencies]
cratestack-mock-wiremock = "0.7.15"
cratestack-parser = "0.7.15"
```

```rust
let schema = cratestack_parser::parse_schema_file("schema.cstack")?;
let package = cratestack_mock_wiremock::generate_package(
    &schema,
    &cratestack_mock_wiremock::WireMockGeneratorConfig::default(),
)?;
for file in package.files {
    println!("{}: {}", file.file_name, file.contents);
}
```

## Running the stateful stubs

A `transport rest` schema's model CRUD stubs need `wiremock-state-extension` loaded — **`docker run wiremock/wiremock` alone is not enough**, and neither is dropping the extension's *plain* (non-`-standalone`) Maven Central jar into `/var/wiremock/extensions`: that combination throws `AbstractMethodError`/`NoSuchMethodError` at request time against every `wiremock/wiremock` image tested (confirmed by hand across three WireMock/extension version pairings — not a version-pinning mistake on this generator's part). The plain jar's Handlebars `Helper` classes are compiled against an unrelocated `com.github.jknack.handlebars`, but every WireMock standalone distribution relocates that package internally. The extension's issue #36 is the identical error, but it was closed as completed in 2023 and the 2024 recurrence report never became its own issue — so the evidence above is this repo's own testing, not an open upstream ticket.

What actually works is the `-standalone` classifier artifact — the output of the extension's own `shadowJar` task, correctly relocated, and published to Maven Central from release 0.9.x on. `docker/Dockerfile` in this crate downloads exactly that jar, verifies it against a pinned sha256, and layers it into a `wiremock/wiremock:3.13.2` image:

```bash
docker build -t my-org/wiremock-stateful -f crates/cratestack-mock-wiremock/docker/Dockerfile crates/cratestack-mock-wiremock/docker
docker run -p 8080:8080 -v "$(pwd)/wiremock/mappings:/home/wiremock/mappings:ro" my-org/wiremock-stateful
```

Versions are pinned in the Dockerfile itself (an exact extension release plus the sha256 of its jar, and an exact WireMock tag) — see its header comment for what's pinned, why, and how to bump both together safely. The build has no JDK and no Gradle step; it is a single-stage image over `wiremock/wiremock:3.13.2` plus a checksum-verified download. Procedure stubs and `transport rpc` model stubs don't need any of this; they work against a plain `docker run wiremock/wiremock`.

Every generated stub also declares `"transformers": ["response-template"]` itself, so `--global-response-templating` isn't strictly required — the Dockerfile sets it as the default `CMD` anyway since it's harmless either way.

## Example

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

`cratestack generate-wiremock --schema schema.cstack --out wiremock` writes `wiremock/mappings/hello.json`:

```json
{
  "metadata": {
    "cratestack": {
      "generated": true,
      "kind": "query",
      "procedure": "hello"
    }
  },
  "request": {
    "method": "POST",
    "urlPath": "/api/$procs/hello"
  },
  "response": {
    "headers": {
      "Content-Type": "application/json"
    },
    "jsonBody": {
      "message": "string"
    },
    "status": 200
  }
}
```

## Design choices worth knowing before you rely on this

- **Procedures stay deterministic, not random.** Two runs against an unchanged schema produce byte-identical output (fixed per-scalar-type defaults: `String` -> `"string"`, `Int` -> `0`, `DateTime` -> a fixed epoch timestamp, etc. — see `src/values.rs`). Model CRUD *responses* are dynamic at request time (that's the point), but the generated *stub files* are just as deterministic — see "Installation" above.
- **No request-body matching, still.** A stub matches on method + path (+ the `If-Match` request *header*, for an `@version` model's `update`/`delete` — see above) only; any request body is accepted (its content drives what gets echoed/stored for stateful fields, but a malformed or unexpected body doesn't make the stub itself fail to match). Real negative-path test coverage (validation errors, `404`s from bad input, auth rejection) needs hand-authored stubs layered on top.
- **Self-referential schemas terminate, they don't hang.** A field whose type cycles back to a type already being expanded resolves to `null` (optional) or `[]` (list) instead of recursing forever; a `Required`-arity cycle with no such escape hatch is a hard [`WireMockGeneratorError::UnbreakableCycle`], not a stack overflow.
- **`get`/`update`/`delete` model routes match any id-shaped path segment**, but only actually respond (200, not the fallback 404) once a matching record exists in the state store — a `state-matcher` `customMatcher` gates each one on `wiremock-state-extension`'s own per-record context, keyed off the request's own detail-route path (`request.path`, e.g. `/api/posts/42`) so two different models' records can never collide even if their ids happen to be numerically identical.
- **A composite `@@id([...])` primary key is rejected up front**, schema-wide, with the identical message `generate-typescript`/`generate-dart` give for the same schema (`cratestack_core::composite_id`) — not the (misleading, "no `@id` field") error a model with no primary key at all gets.
