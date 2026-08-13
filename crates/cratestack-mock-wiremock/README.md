# cratestack-mock-wiremock

Generates [WireMock](https://wiremock.org/) stub mappings from a parsed `.cstack` schema's procedures, so integration and e2e tests can run against a mock backend whose wire contract is derived from the same schema the real server is generated from — instead of a hand-maintained JSON fixture that can silently drift from it.

See the design doc, `docs/design/wiremock-stubs.md`, for the motivating case (ADORSYS-GIS/webank-mobile's 37 hand-maintained WireMock mapping files), the full design, and open questions this crate's v1 slice deliberately leaves open.

## Scope (v2)

- Covered: `procedure`/`mutation procedure` declarations, and `model` blocks' `list`/`get`/`create`/`update`/`delete` CRUD routes, under `transport rest` (the schema default) or `transport rpc`. Happy-path only — every generated stub responds with a synthesized instance of the declared return type / model, matching on request method + path (no body assertion, no error-case variants, no query-string filter/sort/pagination assertion).
- **Not stateful.** A record created through a mocked `create` will not appear in a subsequent `list`; an update will not appear on a subsequent `get`. See `docs/design/wiremock-stubs.md`'s §9 for the full investigation into why (vanilla WireMock scenarios hold one state string, not a per-record store) and what a real per-record store would cost (the third-party `wiremock-state-extension` Java extension) — a decision deliberately left open for the maintainer rather than picked here.
- Not covered yet (tracked as follow-ups in the design doc): `transport grpc` schemas, `FindMany<T>` return types (schema validation already forbids these outside a procedure argument position, so this is defense-in-depth rather than a real gap), error-case stubs (WireMock scenarios/priority), request-body/query filter matching, and any emulation of the auth chokepoint every procedure/model route sits behind.

## Installation

This is a build-time crate, typically invoked through the CLI:

```bash
cratestack generate-wiremock \
  --schema schemas/catalog.cstack \
  --out wiremock \
  --base-path /api
```

This writes one file per procedure under `wiremock/mappings/<procedureName>.json`, and five files per model under `wiremock/mappings/model.<ModelName>.<list|get|create|update|delete>.json` — `mappings/` is the directory a WireMock instance scans by convention, so `--out` can point directly at a project's existing WireMock root (alongside a hand-maintained `__files/` directory, if any).

Pass `--check` to run in drift-detection mode: generate in memory and diff against `--out` instead of writing, exiting non-zero and listing the files that differ. Wire this into CI the same way `generate-dart --check`/`generate-typescript --check` already are, so a schema change that isn't followed by regenerating the stubs fails the build instead of quietly leaving CI running against a stale mock.

To call the generator from Rust:

```toml
[dependencies]
cratestack-mock-wiremock = "0.7.3"
cratestack-parser = "0.7.3"
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

- **Deterministic, not random, example values.** Two runs against an unchanged schema produce byte-identical output (fixed per-scalar-type defaults: `String` -> `"string"`, `Int` -> `0`, `DateTime` -> a fixed epoch timestamp, etc. — see `src/values.rs`). This is what makes `--check` a meaningful drift gate and makes it safe to gitignore generated stubs and regenerate them from a pinned schema, the same way `ADORSYS-GIS/webank-mobile` already treats its generated Dart client.
- **No request-body matching.** A stub matches on method + path only; any request body is accepted. Real negative-path test coverage (validation errors, `404`s, auth rejection) needs hand-authored stubs layered on top — this generator produces the happy-path floor, not a full contract-test replacement.
- **Self-referential schemas terminate, they don't hang.** A field whose type cycles back to a type already being expanded resolves to `null` (optional) or `[]` (list) instead of recursing forever; a `Required`-arity cycle with no such escape hatch is a hard [`WireMockGeneratorError::UnbreakableCycle`], not a stack overflow.
- **Model CRUD stubs are not stateful.** A record created through a mocked `create` will not appear in a subsequent `list`; an update will not appear on a subsequent `get` — every route always answers the same synthesized example. `docs/design/wiremock-stubs.md`'s §9 documents the investigation into why (and what a real per-record store would cost: a third-party Java WireMock extension) in detail. Don't build an example or test suite on top of this crate that assumes create-then-list works.
- **`get`/`update`/`delete` model routes match any id.** There is no record store to know which ids "exist", so these routes use a WireMock `urlPathPattern` (e.g. `^/api/widgets/[^/]+$`) that matches any path segment, not the specific id a real record would have.
