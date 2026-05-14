# Contributing

CrateStack is early public-release software. Keep changes small, tested, and aligned with the schema-first framework boundary described in `README.md`.

Before opening a pull request:

1. Run `cargo fmt`.
2. Run `cargo check --workspace --all-targets --all-features`.
3. Run `cargo test --workspace --all-features`. PG-backed integration tests (`banking_*`, `policy_db_*`, `generated_client_rust`) skip cleanly when `CRATESTACK_TEST_DATABASE_URL` isn't set, so you only see partial coverage on this command.
4. Run `just test-pg` to exercise the PG-backed paths. The recipe brings the Postgres container in `compose.yml` up before tests and tears it down on exit — even if tests fail — so you never leave a container behind. Use `just test-pg-only` for the faster `cratestack`-crate-only inner loop.
5. Run package-specific checks for editor or generated-client changes when applicable.

Do not commit generated build output, local database state, or registry tokens.
