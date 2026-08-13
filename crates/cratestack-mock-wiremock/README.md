# cratestack-mock-wiremock

Generates [WireMock](https://wiremock.org/) stub mappings from a parsed `.cstack` schema's procedures and models, so integration and e2e tests can run against a mock backend whose wire contract is derived from the same schema the real server is generated from — instead of a hand-maintained JSON fixture that can silently drift from it.

See the design doc, `docs/design/wiremock-stubs.md`, for the motivating case (ADORSYS-GIS/webank-mobile's 37 hand-maintained WireMock mapping files), the full design, and the "Model CRUD statefulness" section's investigation before you rely on the stateful behavior below.

## Scope (v3)

- Covered: `procedure`/`mutation procedure` declarations (always static/happy-path), and `model` blocks' `list`/`get`/`create`/`update`/`delete` CRUD routes.
- **`transport rest` model CRUD is stateful.** A record created through a mocked `create` appears in a subsequent `list`; a `PATCH` is visible on a subsequent `get`; a `delete`d record's `get` returns `404`, not a stale body. This is backed by a real per-record store (`wiremock-state-extension`), **not** a fixed example replayed on every request — but it costs something to run. See "Running the stateful stubs" below before you build anything on top of this.
- **`transport rpc` model CRUD stays static** (the pre-v3 shape — one deterministic example, replayed identically on every request, works against any vanilla WireMock). The extension's per-record store needs something unique to each request that REST gets for free (the id-bearing URL path) and RPC doesn't (the id lives in the request body, and this templating stack has no string-concatenation helper to build a unique key from it).
- **List filtering, sorting, and pagination are not implemented**, stateful or not. Every `list` response is the complete, unfiltered collection regardless of `field__operator=value`/`sort`/`limit`/`offset` in the query string — a stateful `list` reflecting *some* of a request's query params and silently ignoring the rest would look like it worked and wasn't tested, which is worse than an honestly-complete response.
- **Fields this generator can't round-trip through the state store fall back to a fixed value.** `Optional`/`List`-arity fields, `Json`/`Bytes`/`Vector(n)`, and any nested `type` reference render the same static example on every response, never reflecting what was created/patched — only `Required`-arity `String`/`Cuid`/`Uuid`/`Int`/`Float`/`Boolean`/`DateTime`/enum fields are genuinely stateful. A relation field (populated only via `include=<relation>`) and an `@server_only` field are excluded entirely, same as before.
- Not covered (tracked as follow-ups in the design doc): `transport grpc` schemas, `FindMany<T>` return types (schema validation already forbids these outside a procedure argument position, so this is defense-in-depth rather than a real gap), error-case stubs, request-body assertion, and any emulation of the auth chokepoint every procedure/model route sits behind.

## Installation

This is a build-time crate, typically invoked through the CLI:

```bash
cratestack generate-wiremock \
  --schema schemas/catalog.cstack \
  --out wiremock \
  --base-path /api
```

This writes one file per procedure under `wiremock/mappings/<procedureName>.json`, and five files per model under `wiremock/mappings/model.<ModelName>.<list|get|create|update|delete>.json` — `mappings/` is the directory a WireMock instance scans by convention, so `--out` can point directly at a project's existing WireMock root (alongside a hand-maintained `__files/` directory, if any).

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

A `transport rest` schema's model CRUD stubs need `wiremock-state-extension` loaded — **`docker run wiremock/wiremock` alone is not enough**, and neither is dropping the extension's plain Maven Central jar into `/var/wiremock/extensions`: that combination throws `AbstractMethodError`/`NoSuchMethodError` at request time against every `wiremock/wiremock` image tested (confirmed by hand across three WireMock/extension version pairings; this is a known, real upstream packaging defect — the extension's own issue #36 — not a version-pinning mistake on this generator's part).

What actually works: `docker/Dockerfile` in this crate, which builds the extension's own `shadowJar` (correctly relocated) from pinned source and layers it into a `wiremock/wiremock:3.13.2` image:

```bash
docker build -t my-org/wiremock-stateful -f crates/cratestack-mock-wiremock/docker/Dockerfile crates/cratestack-mock-wiremock/docker
docker run -p 8080:8080 -v "$(pwd)/wiremock/mappings:/home/wiremock/mappings:ro" my-org/wiremock-stateful
```

Versions are pinned in the Dockerfile itself (a commit SHA for the extension, an exact WireMock tag) — see its header comment for what's pinned, why, and how to bump both together safely. Procedure stubs and `transport rpc` model stubs don't need any of this; they work against a plain `docker run wiremock/wiremock`.

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
- **No request-body matching, still.** A stub matches on method + path only; any request body is accepted (its content drives what gets echoed/stored for stateful fields, but a malformed or unexpected body doesn't make the stub itself fail to match). Real negative-path test coverage (validation errors, `404`s from bad input, auth rejection) needs hand-authored stubs layered on top.
- **Self-referential schemas terminate, they don't hang.** A field whose type cycles back to a type already being expanded resolves to `null` (optional) or `[]` (list) instead of recursing forever; a `Required`-arity cycle with no such escape hatch is a hard [`WireMockGeneratorError::UnbreakableCycle`], not a stack overflow.
- **`get`/`update`/`delete` model routes match any id-shaped path segment**, but only actually respond (200, not the fallback 404) once a matching record exists in the state store — a `state-matcher` `customMatcher` gates each one on `wiremock-state-extension`'s own per-record context, keyed off the request's own detail-route path (`request.path`, e.g. `/api/posts/42`) so two different models' records can never collide even if their ids happen to be numerically identical.
- **A composite `@@id([...])` primary key is rejected up front**, schema-wide, with the identical message `generate-typescript`/`generate-dart` give for the same schema (`cratestack_core::composite_id`) — not the (misleading, "no `@id` field") error a model with no primary key at all gets.
