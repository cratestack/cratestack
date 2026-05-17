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

# Bundle the Studio UI for publishing: source tarball (for `studio
# eject --with-ui`) and the Trunk-built wasm/JS dist (embedded into
# the served binary so `cratestack studio run` ships a real admin app
# out of the box). Both files are gitignored; cargo packages them via
# the explicit `include` list in cratestack-studio's Cargo.toml.
#
# Requires the wasm toolchain:
#   cargo install --locked trunk
#   rustup target add wasm32-unknown-unknown
bundle-studio-ui:
	#!/usr/bin/env bash
	set -euo pipefail
	if ! command -v trunk >/dev/null; then
	  echo "trunk not found. Run: cargo install --locked trunk" >&2
	  exit 1
	fi
	src=crates/cratestack-studio/embedded-ui.tar.gz
	dist=crates/cratestack-studio/embedded-ui-dist.tar.gz
	(cd crates/cratestack-studio-ui && trunk build --release)
	tar --exclude='target' \
	    --exclude='Cargo.lock' \
	    --exclude='.gitignore' \
	    --exclude='.trunk' \
	    --exclude='dist' \
	    -czf "$src" \
	    -C crates cratestack-studio-ui
	tar -czf "$dist" -C crates/cratestack-studio-ui/dist .
	echo "wrote $src ($(du -h "$src" | cut -f1))"
	echo "wrote $dist ($(du -h "$dist" | cut -f1))"

# End-to-end publish for cratestack-studio: refresh the embedded UI
# tarball, then publish. `--allow-dirty` is needed because the
# regenerated tarball is gitignored by design (binary, derived from
# the cratestack-studio-ui sibling); we guard the flag by refusing to
# publish if anything else in the crate is dirty.
publish-studio *args='':
	#!/usr/bin/env bash
	set -euo pipefail
	just bundle-studio-ui
	dirty=$(git status --porcelain -- crates/cratestack-studio crates/cratestack-studio-ui \
	        | grep -vE 'crates/cratestack-studio/embedded-ui(-dist)?\.tar\.gz$' || true)
	if [ -n "$dirty" ]; then
	  echo "refusing to publish: uncommitted changes besides embedded-ui tarballs:" >&2
	  echo "$dirty" >&2
	  exit 1
	fi
	cargo publish -p cratestack-studio --allow-dirty {{args}}

# Rewrite every workspace version literal `0.x.y` -> the given NEW
# version. Reads the current version from the workspace `Cargo.toml`,
# applies the substitution to every `Cargo.toml` in the repo (root +
# crates + examples + studio-ui sibling), and refreshes `Cargo.lock`.
# Idempotent: re-running with the same NEW is a no-op.
bump NEW:
	#!/usr/bin/env bash
	set -euo pipefail
	current=$(awk -F'"' '/^version = /{print $2; exit}' Cargo.toml)
	if [ -z "$current" ]; then
	  echo "could not read current version from Cargo.toml" >&2
	  exit 1
	fi
	if [ "$current" = "{{NEW}}" ]; then
	  echo "already at {{NEW}}; nothing to do"
	  exit 0
	fi
	echo "bumping $current -> {{NEW}}"
	find . -name Cargo.toml \
	  -not -path './target/*' \
	  -not -path '*/node_modules/*' \
	  -print0 | xargs -0 sed -i '' "s/\"$current\"/\"{{NEW}}\"/g"
	# Refresh Cargo.lock so all entries pick up the new version.
	cargo check --workspace --quiet
	echo "bumped to {{NEW}}. Review with: git diff -- '**/Cargo.toml' Cargo.lock"

# Workspace-wide validation gate. Mirrors what `just release` runs
# before publishing. Skips fmt + clippy because `all-checks` is the
# canonical pre-PR gate for those (it auto-fixes); release-check
# focuses on what must hold for a clean publish.
release-check:
	cargo check --workspace --all-targets
	cargo test --workspace --exclude embedded_flutter_native

# Ordered list of crates to publish, leaves first. Keep in sync with
# RELEASE.md. `cratestack-studio-ui` is `publish = false` and absent.
# Note: `cratestack-studio` ships via `just publish-studio` so it can
# refresh the embedded UI tarballs first; the publish loop below
# delegates to that recipe when it gets to studio.
PUBLISH_ORDER := "cratestack-core cratestack-policy cratestack-parser cratestack-codec-cbor cratestack-codec-json cratestack-axum cratestack-sql cratestack-sqlx cratestack-client-rust cratestack-client-dart cratestack-client-typescript cratestack-client-flutter cratestack-client-store-sqlite cratestack-client-store-redis cratestack-studio cratestack-studio-generator cratestack-migrate cratestack-macros cratestack-rusqlite cratestack cratestack-lsp cratestack-redis cratestack-cli"

# Publish every workspace crate to crates.io in dependency order. Use
# `just release-publish dry` for a dry-run (packages + verifies each
# but skips upload). Pairs with `just bump` for the version step and
# `just release` for the full end-to-end flow.
release-publish mode='real':
	#!/usr/bin/env bash
	set -euo pipefail
	if [ "{{mode}}" != "real" ] && [ "{{mode}}" != "dry" ]; then
	  echo "usage: just release-publish [real|dry]" >&2
	  exit 2
	fi
	dry=""
	[ "{{mode}}" = "dry" ] && dry="--dry-run"
	# Bundle the studio UI once up front so the studio publish leg can
	# pick up fresh tarballs without re-running trunk per crate.
	just bundle-studio-ui
	for pkg in {{PUBLISH_ORDER}}; do
	  echo ""
	  echo "=== publishing $pkg ==="
	  if [ "$pkg" = "cratestack-studio" ]; then
	    # Studio needs --allow-dirty for the regenerated tarballs.
	    cargo publish -p "$pkg" --allow-dirty $dry
	  else
	    # Retry once after a short wait if the crates.io index hasn't
	    # propagated a freshly-published dependency.
	    if ! cargo publish -p "$pkg" $dry; then
	      echo "publish failed; sleeping 30s then retrying once..." >&2
	      sleep 30
	      cargo publish -p "$pkg" $dry
	    fi
	  fi
	done

# End-to-end release: bump version, validate, publish all crates, tag,
# and push. Refuses to run on a dirty working tree (besides the
# studio's regenerated tarballs). Tag push is opt-in via PUSH=1.
#
# Usage:
#   just release 0.3.4          # bump, validate, publish, tag locally
#   PUSH=1 just release 0.3.4   # also push commit + tag to origin
#   just release 0.3.4 dry      # rehearsal: dry-run publishes, no tag
release VERSION mode='real':
	#!/usr/bin/env bash
	set -euo pipefail
	if [ "{{mode}}" != "real" ] && [ "{{mode}}" != "dry" ]; then
	  echo "usage: just release VERSION [real|dry]" >&2
	  exit 2
	fi
	just bump {{VERSION}}
	just release-check
	# Commit the bump (unless already committed). Studio tarballs are
	# gitignored by design and don't get staged.
	if ! git diff --quiet -- '**/Cargo.toml' Cargo.lock; then
	  git add ':(glob)**/Cargo.toml' Cargo.lock
	  git commit -m "chore: bump workspace to v{{VERSION}}"
	fi
	just release-publish {{mode}}
	if [ "{{mode}}" = "real" ]; then
	  git tag "v{{VERSION}}"
	  if [ "${PUSH:-0}" = "1" ]; then
	    git push origin HEAD "v{{VERSION}}"
	  else
	    echo "tagged v{{VERSION}} locally. Push with: git push origin HEAD v{{VERSION}}"
	  fi
	fi
