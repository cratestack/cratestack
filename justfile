set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Postgres connection used by every PG-backed integration test
# (banking_*.rs, policy_db_*.rs, generated_client_rust.rs, etc.).
# Matches the compose.yml mapping — port 55432 chosen so it doesn't
# fight any locally-running pg.
PG_URL := "postgres://cratestack:cratestack@localhost:55432/cratestack_test"

default:
  @just --list


all-checks:
	@echo "Running Rust formatting, lint, and checks"
	cargo fmt
	cargo fix --allow-dirty
	cargo clippy --all-targets --all-features --fix --allow-dirty -- -D warnings
	cargo check --all-targets --all-features
	cargo deny check

# Bring the Postgres test container up (idempotent; waits for ready).
pg-up:
	@docker compose up -d postgres >/dev/null
	@until docker exec cratestack-postgres pg_isready -U cratestack -d cratestack_test >/dev/null 2>&1; do sleep 1; done
	@echo "postgres ready at {{PG_URL}}"

# Stop and remove the Postgres test container (safe when nothing is up).
pg-down:
	@docker compose down >/dev/null
	@echo "postgres stopped"

# Run the full workspace test suite against a real Postgres, with auto-teardown on exit.
test-pg *args='':
	#!/usr/bin/env bash
	set -euo pipefail
	cleanup() { docker compose down >/dev/null 2>&1 || true; echo "postgres stopped"; }
	trap cleanup EXIT
	just pg-up
	CRATESTACK_TEST_DATABASE_URL='{{PG_URL}}' cargo test --workspace --exclude embedded_flutter_native {{args}}

# Run only the cratestack-crate integration tests against PG (faster inner loop).
test-pg-only *args='':
	#!/usr/bin/env bash
	set -euo pipefail
	cleanup() { docker compose down >/dev/null 2>&1 || true; echo "postgres stopped"; }
	trap cleanup EXIT
	just pg-up
	CRATESTACK_TEST_DATABASE_URL='{{PG_URL}}' cargo test -p cratestack {{args}}

# Run the workspace test suite via testcontainers (per-binary ephemeral PG, recommended for CI).
test-pg-tc *args='':
	CRATESTACK_USE_TESTCONTAINERS=1 cargo test --workspace --exclude embedded_flutter_native {{args}}
