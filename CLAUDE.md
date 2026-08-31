# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

CrateStack is a Rust-native, schema-first framework. You write a `.cstack` schema and a compile-time
macro generates the typed Rust surface — models, CRUD routes, procedures, policies, clients — for one
of three deployment roles. The framework is pre-1.0; the public crates are versioned together off the
workspace `version` in the root `Cargo.toml`.

## Commands

Most workflows are encoded in the `justfile` (`just --list`). The important ones:

> **Linux prerequisite:** the workspace includes the `tauri-*` example crates, whose Linux backend pulls
> `glib-sys`/`webkit2gtk-sys` — so a fresh `--workspace` build/test on Linux needs the GTK/WebKit dev
> packages installed (`libgtk-3-dev`, `libwebkit2gtk-4.1-dev`, …). macOS uses the system WebKit and needs
> nothing extra. The rustdoc CI job sidesteps this by building the framework crates by name, not `--workspace`.

- **Pre-PR gate:** `just all-checks` — runs `cargo fmt`, `cargo fix`, `cargo clippy --fix -D warnings`,
  `cargo check --all-targets`, `just lint`, and `cargo deny check`, all scoped `--workspace --exclude
  embedded_flutter_native`. This is the canonical formatting + lint pass; run it before opening a PR.
  (Deliberately **not** `--all-features` — see the plain-tests note below.)
  The trailing `just lint` — the same no-`--fix` command CI's clippy job runs — is what makes a green
  `all-checks` actually predictive of CI (cratestack#775). It is not redundant with the `--fix` clippy
  line: `--fix` demotes a `-D warnings` error to a warning whenever the crate still compiles, so a lint
  with no machine-applicable rewrite passes `--fix` silently *and* leaves no working-tree diff. Measured:
  with such a violation present, `just lint` exited 101 while `all-checks` exited 0 on the same tree.
  Consequence, by design: `all-checks` now fails on lint drift `--fix` used to absorb.
- **Build:** `cargo build --workspace --exclude embedded_flutter_native` (the Flutter native crate needs
  flutter_rust_bridge-generated glue that isn't checked in — see the test note below).
- **Plain tests (no DB):** `cargo test --workspace --exclude embedded_flutter_native`. PG-backed
  integration tests (`banking_*`, `policy_db_*`, `generated_client_rust`) **skip silently** when
  `CRATESTACK_TEST_DATABASE_URL` is unset — a green run here does *not* mean full coverage.
  Two flags to avoid: `embedded_flutter_native` needs flutter_rust_bridge-generated glue that isn't
  checked in (hence the `--exclude`, mirroring the `just` recipes), and `--all-features` enables both
  mutually-exclusive `decimal-*` backends, which trips a `compile_error!` in `cratestack-core`.
- **PG-backed tests:** `just test-pg` — brings up the Postgres container from `compose.yml` (port `55432`),
  runs the full suite, and tears the container down on exit even on failure. `just test-pg-only` is the
  faster inner loop (server facade only). `just test-pg-tc` uses ephemeral per-binary testcontainers
  (what CI uses; stronger isolation, per-binary spin-up cost).
- **Single test:** `cargo test -p <crate> <test_name>`, or under PG:
  `just test-pg-only -- <test_name>` (extra args pass through to `cargo test`). Note: `just test-pg`
  hardcodes `--workspace`, which conflicts with `-p`, so use `test-pg-only` to scope to one crate.
- **Release validation:** `just release-check` (check + tests, run once; `SKIP_TESTS=1` overrides).
  It does *not* retry — the `generated_routes_emit_tracing_events` flake it used to absorb was
  fixed for real and the retry loop removed (#417).
- **Version bump:** `just bump 0.x.y` rewrites every `Cargo.toml` version literal and refreshes the lock.
- **Release:** `just release 0.x.y` (bump → validate → publish in topo order → tag; `PUSH=1` to push).
  Do not hand-maintain publish order — it is topo-sorted from `cargo metadata` at recipe time.
- **CLI:** `cargo run -p cratestack-cli -- <check|generate-dart|generate-typescript|generate-wiremock|studio|migrate|init|run|eject|diff|print-ir>`
- **Regenerate committed example clients:** `just regen-examples` — rewrites the two committed generated
  clients (`examples/flutter-riverpod/client` via `generate-dart --preset riverpod`, and
  `examples/react-vite-swr/client` via `generate-typescript --swr`) in place. Takes an `*args=''`
  passthrough (same shape as `just check`): CI's `flutter (flutter-riverpod example)` and
  `js (react-vite-swr example)` drift-check steps call `just regen-examples --check` directly, so the
  recipe *is* the CI check, not a hand-copied third invocation — they cannot copy-paste diverge. Run
  `just regen-examples` (no args) locally after changing a Dart or TypeScript codegen template, review
  `git diff`, and commit the result.

### Critical test gotcha

`-p cratestack` selects an **empty documentation-only vitrine crate**, not a real package — it will
return a false green. Always target `-p cratestack-pg` (server facade) or `-p cratestack-sqlite`
(embedded facade) explicitly. Likewise `embedded_flutter_native` is excluded from workspace test runs
(`--exclude embedded_flutter_native`) because of flutter_rust_bridge toolchain requirements.

**On rootless Docker, every testcontainers-backed suite skips and still prints `ok`.**
`testcontainers-rs`/`bollard` don't read `docker context` — they default to
`unix:///var/run/docker.sock` — so the container never starts, `connect_or_skip()` reads that as "no
database", and the skip reports as a pass. `docker info` succeeding proves nothing: that's the CLI,
which *does* read the context. Export the endpoint (derived, not hardcoded — the value encodes a uid)
and make a skip fail loudly:

```bash
export DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"
export CRATESTACK_REQUIRE_DB=1      # or CRATESTACK_REQUIRE_REDIS=1
```

To tell a skip from a pass after the fact, read `finished in` rather than the summary line — both say
`ok`, but a real PG binary takes seconds and a skipped one reports `0.00s`. Prefer `--test-threads=1`
for PG binaries; parallel testcontainers hit a rootless port-bind race that mimics a test failure.
Full write-up in `CONTRIBUTING.md`, "Rootless Docker: `DOCKER_HOST` and the false green".

## Architecture

### The three-macro / role model (the central idea)

One `.cstack` schema, three mutually-exclusive entry macros — **pick one per consuming crate** based on
what that crate *is*:

- `include_server_schema!("schema.cstack", db = Postgres)` — sqlx + axum + procedures + events; owns a
  Postgres DB. (`db = Postgres` is currently the only value; the parser is wired so future backends are
  non-breaking at existing call sites.)
- `include_embedded_schema!("schema.cstack")` — rusqlite only, sync, **no policy enforcement**. Compiles
  to native *and* `wasm32-unknown-unknown` (browser/OPFS via `sqlite-wasm-rs`) from the same source.
- `include_client_schema!("schema.cstack")` — HTTP client stubs only; treats another service's schema as
  a contract, owns no DB.

### Four disjoint facades

As of 0.4.0 the umbrella crate is split into facades consumers select via Cargo's `package =` rename —
a fourth (`cratestack-client`) was added by cratestack#490:

- `cratestack = { package = "cratestack-pg" }` — Postgres + Axum + Rust client runtime; for
  `include_server_schema!("...", db = Postgres)` schemas. Does **not** pull `libsqlite3-sys`, so it
  coexists with the official `sqlx` umbrella without `links = "sqlite3"` clashes.
- `cratestack = { package = "cratestack-api" }` — Axum HTTP bindings + Rust client runtime, with no
  database backend at all; for `include_server_schema!("...", db = None)` procedures-only services. No
  `cratestack-sqlx` dependency under any feature gate. Switch to `cratestack-pg` the moment the schema
  needs even one `model` (forbidden in `db = None` schemas at parse time).
- `cratestack = { package = "cratestack-sqlite" }` — rusqlite (native + wasm) + shared surface; for
  `include_embedded_schema!` on both native and `wasm32-unknown-unknown` targets.
- `cratestack = { package = "cratestack-client" }` — pure HTTP-client SDK facade; re-exports **only**
  `include_client_schema!` (not the other two entry macros) plus the generated Rust client runtime and
  the handful of type re-exports client codegen references. `cratestack-axum` — and therefore
  `axum`/`tower`/`hyper`/`tower-http` — is structurally absent from its dependency graph under its
  default features (proved by `examples/client-only-verification`'s `cargo tree`, re-run by CI's
  `facade-disjointness` job).

**Hard rule (enforced by convention, watch for regressions):** the macro split must stay strictly
disjoint. `include_server_schema!(db = Postgres)` emits sqlx-only code; `include_server_schema!(db = None)`
emits axum-only code with no DB machinery at all; `include_embedded_schema!` emits rusqlite-only code;
`include_client_schema!` emits axum-free client code. No cross-backend impls leak between the four paths.

### Crate layering

The dependency flow is roughly: **parser → core/policy/sql → macros → backend runtimes / clients**.

- `cratestack-parser` — `.cstack` parser + semantic checker (chumsky-based).
- `cratestack-core` — shared metadata, auth context, codec, error/envelope types, transport descriptors.
- `cratestack-policy` — canonical policy literals, predicates, procedure-policy evaluation.
- `cratestack-sql` — dialect-agnostic SQL primitives shared by both backends.
- `cratestack-macros` — **the codegen heart.** All compile-time generation lives here, organized by
  concern: `include/` (the three entry macros + server collectors), `model/`, `procedure/`, `view/`,
  `relation/`, `policy/`, `transport/` (REST vs RPC dispatch), `axum/`, `client/` (rust/dart/ts, rest/rpc).
- Backend runtimes: `cratestack-sqlx` (Postgres), `cratestack-rusqlite` (embedded), `cratestack-axum`
  (route integration), `cratestack-redis` (server idempotency/rate-limit stores).
- Clients: `cratestack-client-rust`, `-dart`, `-typescript`, `-flutter`, plus client state stores
  (`-store-sqlite`, `-store-redis`).
- Codecs: `cratestack-codec-cbor` (default wire format), `cratestack-codec-json`.
- Native bindings: `cratestack-cbor-napi` — napi-rs Node addon wrapping `cratestack-codec-cbor` for
  `@cratestack/cbor-node` (`packages/cratestack-cbor-node`, issue #286). `publish = false` (ships
  only as a compiled `.node` addon inside the npm package, never as a Cargo dependency).
  `cratestack-cbor-wasm` — wasm-bindgen bindings for browser JavaScript, compiled to `@cratestack/cbor-web`
  (`packages/cratestack-cbor-web`). `publish = false` (cdylib-only with no rlib; nothing could `cargo add`
  it usefully). `cratestack-studio-ui` — Leptos+Trunk web UI, excluded from the workspace to avoid forcing
  developers onto the wasm32 toolchain. `publish = false` (not a dependency, just a bundled web asset).
- Tooling: `cratestack-cli`, `cratestack-lsp` (tower-lsp-server LSP for `.cstack`), `cratestack-migrate`,
  `cratestack-studio` (+ `-studio-ui` wasm app — see below).

### Decimal backend selection: additive, not mutually exclusive (cratestack#505)

`cratestack-core` exposes two decimal backends behind Cargo features, `decimal-rust-decimal` (the
default — fast, stack-allocated, capped at 28-29 significant digits) and `decimal-bigdecimal`
(arbitrary precision, heap-allocated, opt-in). **Both may be selected in the same build** (Direction 2,
the associated-type/marker shape — see `docs/design/decimal-backend-additivity.md` §7(b)/§13): each
backend's concrete type is exposed under its own name, `RustDecimal`/`BigDecimal`, independently of the
other, and `cratestack-sql`'s `SqlValue::Decimal` holds a `Box<dyn DecimalLike>` rather than one fixed
concrete type. A schema picks which concrete backend it uses via the `decimal = RustDecimal |
BigDecimal` argument on its `include_*_schema!` macro call (`cratestack-macros/src/include/decimal_arg.rs`)
— a schema-authored choice, not a Cargo feature — required exactly when the schema declares a `Decimal`
field somewhere. This closes what used to be a graph-wide invariant: two independent dependents in the
same build, each choosing a different backend, used to force a hard `compile_error!` into a combined
build that neither one alone controlled or could fix; that no longer happens.

Selecting **neither** feature is still not an error (cratestack#421-era versions of this crate got this
wrong, fixed by cratestack#521): a crate that legitimately narrows its graph with
`default-features = false` and never touches `Decimal` builds cleanly — `Decimal`/`RustDecimal`/
`BigDecimal` (and anything that references a decimal type unconditionally, e.g.
`cratestack_core::validate_range_decimal`, now generic over the unconditional `DecimalValue` bound
instead) simply don't exist on the public surface in that configuration, rather than hard-failing every
backend-agnostic consumer. `cratestack-core::Decimal` (the legacy single-backend alias, kept for
hand-written code) still only exists when *exactly one* backend is selected — an ambiguous name with
two backends active has no single correct answer, so it's absent rather than silently resolved, matching
the "neither" treatment. See `cratestack-core/src/decimal.rs`'s module doc and
`docs/design/decimal-backend-additivity.md` for the full mechanism, including why making `rust_decimal`
a mandatory dependency isn't the fix (it would leak it into every `decimal-bigdecimal` consumer's graph,
breaking cratestack#495's acceptance bar — still true, unaffected by this change).

### Transport: REST vs RPC

A schema declares either REST routes (default) or `transport rpc`. The two are mutually exclusive per
schema. RPC collapses the surface to two endpoints — `POST /rpc/{op_id}` (unary) and `POST /rpc/batch` —
dispatched by a generated string `match` on dotted op IDs (`model.<Model>.<verb>`, `procedure.<name>`).
Spec lives in `docs/design/rpc-transport.md`; generation is under `cratestack-macros/src/transport/` and
`include/server/rpc_module/`.

### Studio UI build

`cratestack-studio-ui` is a Trunk-built `wasm32` app, **excluded** from the workspace (`exclude` in root
`Cargo.toml`) so developers aren't forced onto the wasm toolchain. It's bundled into the served binary via
`just bundle-studio-ui` (requires `trunk` + the `wasm32-unknown-unknown` target) and shipped as gitignored
tarballs that `cargo publish` includes explicitly. `just publish-studio` re-bundles before publishing.

## Conventions

- `unsafe_code = "forbid"` workspace-wide — declared once in the root `Cargo.toml`'s `[workspace.lints.rust]`
  and actually enforced: every workspace member opts in via `[lints]\nworkspace = true` (cratestack#523;
  `just verify-lints-optin` is the regression guard, wired as a CI job). Three FFI-boundary crates
  (`cratestack-cbor-napi`, `examples/react-nextjs-daisyui/napi`, `examples/embedded-expo/native` — napi-derive
  trampolines and a raw C-ABI export) manually override `unsafe_code = "allow"` instead, each with a comment
  explaining why (Cargo rejects combining `workspace = true` with a per-package override in the same
  manifest). `cratestack-cbor-wasm` and `cratestack-studio-ui` (wasm-bindgen) need no override — verified
  clean under the forbid. Standalone example/vitrine workspaces excluded from the root `[workspace] members`
  list (`cratestack-studio-ui`, the `no-database-verification*` crates, `client-only-verification`) declare
  the same `forbid` locally, since the root table can't reach a disjoint workspace.
- **Transport parity — REST and RPC ship together, never REST first.** Any feature that touches the
  request/response surface (query parameters, projections, per-request arguments, new response shapes,
  client call surfaces) must land on BOTH transports in the same PR — server dispatch, `RpcListInput`/
  `RpcGetInput` frame slots, AND every generated client (Rust/Dart/TS, plus swr/riverpod layers where
  they exist). The mechanism that makes this cheap: RPC dispatch synthesizes a real URL query string
  and re-enters the REST parsing/validation path (`cratestack-axum/src/rpc/synthesize.rs`), so the
  server side is usually one frame field + one `pairs.push`. History says this rule is needed: the
  `@computed` params surface shipped REST-only in v1 and closing the gap took three follow-up PRs
  (cratestack#724 and kin). If a transport is genuinely excluded, that is a design-doc'd,
  changelog'd decision — not an omission.
- Rust source uses `snake_case` filenames (rustfmt convention); all other files are `kebab-case`.
- **200-LoC file ceiling:** there is an active, validated convention of keeping each source file under
  ~200 lines, splitting larger files by concern (this is why `macros/` and `axum/` are deeply nested).
  When adding code, prefer extending the existing fine-grained module layout over growing a file past
  the threshold. Refactor PRs are scoped per-crate.
- Don't commit generated build output, local DB state, or the studio tarballs (gitignored by design).
- Changelog entries go under the `## Unreleased` heading at the top of each declared changelog
  (`.ci/changelog-files.sh` — currently `CHANGELOG.md` and `dart-packages/cratestack_cbor/CHANGELOG.md`),
  never under the newest dated section. A release bump promotes `## Unreleased` into a dated section and
  re-seeds a fresh, empty one above it (`.ci/changelog-seed.sh`), so it's always there for the next PR.

<!-- BEGIN: AI Governance stanza (managed by ADORSYS-GIS/ai-governance) -->
## AI Governance

AI may accelerate the work, but humans own intent, verification, and consequences.
AI output is not truth: review AI-generated code as untrusted, and never submit work you cannot explain.

When opening issues or pull requests in this repo:

- Use the provided **issue forms** (Epic, User Story, Dev Ticket) and the **pull request template** — do not open blank issues/PRs.
- Fill in the **AI Usage Declaration** honestly (what AI was used for, what you verified).
- Include a **source-of-truth link** (a URL or `#123` reference). No source of truth means the work is not ready.
- Provide **verification evidence** (commands, logs, links, or checked verification boxes). No evidence means it is not done.

Source of truth and full doctrine: https://adorsys-gis.github.io/ai-governance/
This stanza is intentionally thin — read the site; do not duplicate the doctrine here.
<!-- END: AI Governance stanza -->
