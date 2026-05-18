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
#
# Test stage is retried up to 3x to absorb known-flaky tests (notably
# `generated_routes_emit_tracing_events`, which intermittently misses
# tracing events under workspace concurrency — see its source comment).
# A genuine regression will fail all 3 attempts and still block the
# release; only flakes get masked.
#
# Emergency override: `SKIP_TESTS=1 just release-check` bypasses the
# test stage entirely. Use only when you know the failing test is the
# known flake and you've already verified it passes in isolation.
release-check:
	#!/usr/bin/env bash
	set -euo pipefail
	cargo check --workspace --all-targets
	if [ "${SKIP_TESTS:-0}" = "1" ]; then
	  echo "release-check: SKIP_TESTS=1 — bypassing workspace tests." >&2
	  exit 0
	fi
	attempt=1
	max=3
	while [ "$attempt" -le "$max" ]; do
	  echo ""
	  echo "=== test attempt $attempt/$max ==="
	  if cargo test --workspace --exclude embedded_flutter_native; then
	    exit 0
	  fi
	  if [ "$attempt" -eq "$max" ]; then
	    echo "" >&2
	    echo "release-check: tests failed after $max attempts." >&2
	    echo "If you've verified this is a known flake (e.g. tracing event capture)," >&2
	    echo "rerun with: SKIP_TESTS=1 just release-check" >&2
	    exit 1
	  fi
	  echo "tests failed; retrying ($((attempt + 1))/$max)..."
	  attempt=$((attempt + 1))
	done

# Publish every workspace crate to crates.io in dependency order.
#
# The order is computed at recipe-time by topologically sorting the
# workspace graph from `cargo metadata`, so it can't drift when a new
# inter-crate dependency lands. Previously a hand-maintained list in
# this file (and in RELEASE.md) silently broke when cratestack-studio
# took on a cratestack-migrate dep but migrate stayed later in the list.
#
# Idempotent: each crate is checked against the crates.io API for the
# current workspace version before publishing — already-uploaded
# versions are skipped, so reruns after a partial failure pick up
# where they left off. Use `FROM=cratestack-studio just release-publish`
# to skip straight to a specific crate without hitting crates.io for
# the earlier ones.
#
# `dry` mode runs `cargo publish --dry-run` (packages + verifies each
# but skips upload). Pairs with `just bump` for the version step and
# `just release` for the full end-to-end flow.
release-publish mode='real':
	#!/usr/bin/env bash
	# Note: `set +e` here — we want to detect "already uploaded" errors
	# rather than abort the whole loop on them. Each command's exit
	# status is checked explicitly.
	set -uo pipefail
	if [ "{{mode}}" != "real" ] && [ "{{mode}}" != "dry" ]; then
	  echo "usage: just release-publish [real|dry]" >&2
	  exit 2
	fi
	# Compute the publish order by topo-sorting cratestack-* workspace
	# packages from `cargo metadata`. Requires python3 (ubiquitous on
	# dev machines). On failure (parse, cycle, missing python) the
	# error is loud and the recipe aborts before touching crates.io.
	publish_order=$(cargo metadata --format-version=1 --no-deps 2>/dev/null | \
	  python3 -c "$(cat <<'PYEOF'
	import json, sys, copy
	m = json.load(sys.stdin)
	pkgs = {p["name"]: p for p in m["packages"]
	        if p["name"].startswith("cratestack") and p.get("publish") != []}
	graph = {n: {d["name"] for d in p["dependencies"]
	             if d["name"] in pkgs and d["name"] != n}
	         for n, p in pkgs.items()}
	order, remaining = [], copy.deepcopy(graph)
	while remaining:
	    leaves = sorted(n for n, d in remaining.items() if not d)
	    if not leaves:
	        sys.exit(f"dependency cycle: {remaining}")
	    for n in leaves:
	        order.append(n); del remaining[n]
	    for d in remaining.values():
	        d.difference_update(leaves)
	print(" ".join(order))
	PYEOF
	)")
	if [ -z "$publish_order" ]; then
	  echo "failed to compute publish order from cargo metadata" >&2
	  exit 1
	fi
	dry=""
	[ "{{mode}}" = "dry" ] && dry="--dry-run"
	version=$(awk -F'"' '/^version = /{print $2; exit}' Cargo.toml)
	from="${FROM:-}"
	skipping=true
	[ -z "$from" ] && skipping=false

	# Returns 0 if `crate@version` is already published on crates.io.
	# Uses the JSON API; falls back to "not published" on any network
	# error so we surface a fresh publish failure rather than masking it.
	is_published() {
	  local pkg=$1 ver=$2
	  curl -fsS -A "cratestack-release/1.0" \
	    "https://crates.io/api/v1/crates/$pkg/$ver" \
	    -o /dev/null 2>/dev/null
	}

	# Run a single `cargo publish` and classify the outcome:
	#   0 = published or already uploaded (idempotent success)
	#   1 = real failure (caller decides retry vs abort)
	publish_once() {
	  local pkg=$1 extra=$2
	  local out rc
	  out=$(cargo publish -p "$pkg" $extra $dry 2>&1)
	  rc=$?
	  printf '%s\n' "$out"
	  if [ $rc -eq 0 ]; then
	    return 0
	  fi
	  if printf '%s' "$out" | grep -qE 'already (uploaded|exists)'; then
	    echo "  → already uploaded, treating as success"
	    return 0
	  fi
	  return 1
	}

	bundled=false
	bundle_studio() {
	  if [ "$bundled" = "false" ]; then
	    just bundle-studio-ui >&2
	    bundled=true
	  fi
	}

	failed=""
	for pkg in $publish_order; do
	  if [ "$skipping" = "true" ]; then
	    if [ "$pkg" = "$from" ]; then
	      skipping=false
	    else
	      echo "skipping $pkg (before FROM=$from)"
	      continue
	    fi
	  fi

	  echo ""
	  echo "=== $pkg @ $version ==="

	  # Cheap pre-check: if already on crates.io, skip the cargo publish
	  # round-trip entirely. Only meaningful in real mode; dry runs
	  # always want to exercise the package + verify path.
	  if [ "{{mode}}" = "real" ] && is_published "$pkg" "$version"; then
	    echo "  → already on crates.io, skipping"
	    continue
	  fi

	  extra=""
	  if [ "$pkg" = "cratestack-studio" ]; then
	    # Re-bundle right before studio's publish so OUT_DIR matches the
	    # current sibling state, even if a prior loop iteration cleaned it.
	    bundle_studio
	    extra="--allow-dirty"
	  fi

	  if publish_once "$pkg" "$extra"; then
	    continue
	  fi
	  echo "  → first attempt failed; sleeping 30s and retrying once..." >&2
	  sleep 30
	  if publish_once "$pkg" "$extra"; then
	    continue
	  fi
	  failed="$pkg"
	  break
	done

	if [ -n "$failed" ]; then
	  echo "" >&2
	  echo "release-publish failed at $failed." >&2
	  echo "Resume with: FROM=$failed just release-publish {{mode}}" >&2
	  exit 1
	fi

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
