# Contributing

CrateStack is early public-release software. Keep changes small, tested, and aligned with the schema-first framework boundary described in `README.md`.

Before opening a pull request:

1. Run `cargo fmt`.
2. Run `cargo check --workspace --exclude embedded_flutter_native --all-targets`. (`just all-checks` wraps fmt + clippy + this check.) Do **not** add `--all-features`: it enables both mutually-exclusive `decimal-*` backends and trips a `compile_error!` in `cratestack-core`. Exclude `embedded_flutter_native` — its `flutter_rust_bridge`-generated glue isn't checked in, so a bare `--workspace` build fails with E0583.
3. Run `cargo test --workspace --exclude embedded_flutter_native`. PG-backed integration tests (`banking_*`, `policy_db_*`, `generated_client_rust`) skip cleanly when `CRATESTACK_TEST_DATABASE_URL` isn't set, so you only see partial coverage on this command.
4. Run `just test-pg` to exercise the PG-backed paths. The recipe brings the Postgres container in `compose.yml` up before tests and tears it down on exit — even if tests fail — so you never leave a container behind. Use `just test-pg-only` for the faster `cratestack`-crate-only inner loop.
   - **Alternative — testcontainers**: `just test-pg-tc` runs the same suite but with `CRATESTACK_USE_TESTCONTAINERS=1`, which makes each test binary spawn its own ephemeral PG via `testcontainers`. Cleanup is automatic via `Drop`; you'll see a per-binary spin-up cost of a few seconds. Use this when you want stronger isolation guarantees (CI does), accept that you can't `psql` into a mid-test container easily.
5. Run package-specific checks for editor or generated-client changes when applicable.

Do not commit generated build output, local database state, or registry tokens.

## AI Governance

This repository follows the [ADORSYS-GIS AI Governance](https://adorsys-gis.github.io/ai-governance/) discipline:
**AI may accelerate the work, but humans own intent, verification, and consequences.**

- **Open issues** with the structured forms — [Epic](.github/ISSUE_TEMPLATE/epic.yml),
  [User Story](.github/ISSUE_TEMPLATE/user-story.yml), or
  [Development Ticket](.github/ISSUE_TEMPLATE/dev-ticket.yml). Blank issues are disabled on purpose.
- **Open pull requests** using the [pull request template](.github/PULL_REQUEST_TEMPLATE.md). Fill in every section.
- Always complete the **AI Usage Declaration**, link a **source of truth** (a URL or `#123` reference), and attach **verification evidence** (commands, logs, links, or checked boxes).

A governance CI check (`.github/workflows/governance.yml`) enforces that every PR body declares AI usage,
references a source of truth, and shows verification evidence. Work is **Ready** only when its intent is clear,
its source of truth is linked, and any AI-generated content has been reviewed by a human; it is **Done** only
when acceptance criteria are met, tests pass, evidence is attached, and a named human owner accepts
responsibility — see the [AI Working Agreement](https://adorsys-gis.github.io/ai-governance/12-ai-working-agreement)
and the [Doctrine](https://adorsys-gis.github.io/ai-governance/13-doctrine).
