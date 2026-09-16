#!/usr/bin/env bash
# Quality check orchestrator — runs all scanners and produces SARIF reports
# Usage: .ci/quality/run.sh [--scan-type=pr|full|scheduled]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
REPORTS_DIR="$PROJECT_ROOT/.ci/quality/reports"
RULES_DIR="$PROJECT_ROOT/.ci/rules/semgrep"
ACTIONLINT_SARIF_TEMPLATE="$PROJECT_ROOT/.ci/rules/actionlint/sarif.tmpl"
GITLEAKS_BASELINE="$PROJECT_ROOT/.ci/baselines/gitleaks.toml"

SCAN_TYPE="${1:-pr}"
if [[ "$SCAN_TYPE" == --scan-type=* ]]; then
  SCAN_TYPE="${SCAN_TYPE#--scan-type=}"
fi

# Ensure reports directory exists
mkdir -p "$REPORTS_DIR"

log() { echo "[quality] $*" >&2; }
warn() { echo "[quality] WARN: $*" >&2; }

# Fatal errors ALSO emit a GitHub Actions `::error::` annotation, not just a
# log line. A log line is not reliably readable: this script's console output
# runs to tens of thousands of lines (cargo-deny alone emits ~14k when it has
# findings), GitHub caps a step's log, and the cap is reached *before* the end
# of this script — so the one line that says why the job failed is exactly the
# line most likely to be truncated away. That is not hypothetical: on
# 2026-09-16 the job failed on RUSTSEC-2026-0285, this line was cut, and the
# last visible output was an unrelated semgrep rule warning.
#
# An annotation is rendered from the API rather than the log body, so it
# survives truncation and shows up in the job's Annotations panel. Outside
# Actions the `::error::` prefix is just inert text on stderr, so this is safe
# to run locally.
error() {
  echo "::error title=Quality check failed::$*"
  echo "[quality] ERROR: $*" >&2
  exit 1
}

# Track errors but don't fail immediately — collect all reports first
SCAN_ERRORS=0

# cargo-deny is CrateStack's actual dependency-risk gate (see scan_cargo_deny
# below for why it's handled differently from the other, informational
# scanners in this file): a real finding here sets this flag, which is
# checked at the very end of the script, after every scanner has run.
CARGO_DENY_FAILED=0

# ============================================================================
# Utility: Create a minimal SARIF report from non-SARIF output
# ============================================================================

create_sarif_stub() {
  local tool_name="$1"
  local message="$2"
  cat > "$REPORTS_DIR/${tool_name}.sarif" << EOF
{
  "\$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
  "version": "2.1.0",
  "runs": [
    {
      "tool": {
        "driver": {
          "name": "$tool_name",
          "version": "offline",
          "informationUri": ""
        }
      },
      "results": [],
      "properties": {
        "status": "skipped",
        "reason": "$message"
      }
    }
  ]
}
EOF
}

# ============================================================================
# Scanner: cargo deny (dependency scanning)
# ============================================================================

scan_cargo_deny() {
  log "Running cargo deny..."

  # cargo subcommands are separate binaries (cargo-<name>) on PATH; checking
  # `cargo` alone would let this fall through to `cargo deny` even when the
  # cargo-deny subcommand itself isn't installed, misreporting "no such
  # command: deny" as "found issues" instead of a real execution error.
  if ! command -v cargo-deny &> /dev/null; then
    warn "cargo-deny not found; skipping"
    create_sarif_stub "cargo-deny" "cargo-deny not found on runner"
    return
  fi

  # cargo deny doesn't produce SARIF directly; capture human output
  # "all" is a positional check-selector value, not a flag (`--all` doesn't
  # exist — confirmed via `cargo deny check --help` against a real binary;
  # it errors "unexpected argument '--all' found").
  #
  # Unlike the SAST/secrets/IaC scanners in this file (semgrep, gitleaks,
  # trivy), whose findings are informational and gated downstream by
  # reviewdog's diff-aware filtering over the merged SARIF, cargo-deny never
  # emits real SARIF (see the stub below — "SARIF conversion not yet
  # implemented"), so its findings never reach that pipeline at all. A
  # license/advisory/ban/source violation here is CrateStack's actual
  # dependency-risk gate (CLAUDE.md's `just all-checks` ends in `cargo deny
  # check`), so it must fail this script for real rather than being logged
  # and swallowed. That failure is deferred to CARGO_DENY_FAILED (checked
  # at the very end of the script, after every other scanner has still had
  # its chance to run and produce a report) instead of exiting here
  # immediately, to preserve this script's "collect every report before
  # failing" design — an immediate exit here would starve the later
  # scanners of a run and break gate.sh's "every scanner produced a SARIF"
  # check for unrelated reasons.
  # Redirected, NOT `tee`d. `cargo deny check all` prints the full dependency
  # tree for every finding — ~14k lines on this workspace when advisories fire,
  # which is on its own enough to exhaust a GitHub step's log budget and
  # truncate everything this script prints afterwards, including its own fatal
  # error. The full report is still written to cargo-deny.txt and uploaded as
  # a build artifact; the console gets the part a human reads first.
  if ! cargo deny check all > "$REPORTS_DIR/cargo-deny.txt" 2>&1; then
    warn "cargo deny FAILED — this WILL fail the quality job (see the summary below, and $REPORTS_DIR/cargo-deny.txt for the full report with dependency trees)"
    # The `error[...]`/`warning[...]` headline lines and cargo-deny's own
    # per-category summary. Capped, so a pathological run cannot reintroduce
    # the flooding this redirect exists to prevent.
    grep -E '^(error|warning)\[' "$REPORTS_DIR/cargo-deny.txt" | head -40 >&2 || true
    grep -E '^(advisories|bans|licenses|sources) ' "$REPORTS_DIR/cargo-deny.txt" | tail -1 >&2 || true
    CARGO_DENY_FAILED=1
  else
    log "cargo deny check passed"
  fi

  # For now, create a stub SARIF — a proper converter would parse the text
  # In production, integrate `cargo deny --format json` if available
  create_sarif_stub "cargo-deny" "Use cargo-deny.txt report; SARIF conversion not yet implemented"
}

# ============================================================================
# Scanner: cargo audit (advisory scanning)
# ============================================================================

scan_cargo_audit() {
  log "Running cargo audit..."

  if ! command -v cargo-audit &> /dev/null; then
    warn "cargo-audit not found; skipping"
    create_sarif_stub "cargo-audit" "cargo-audit not found on runner"
    return
  fi

  if ! cargo audit 2>&1 | tee "$REPORTS_DIR/cargo-audit.txt"; then
    log "cargo audit found advisories (expected in scans)"
  fi

  create_sarif_stub "cargo-audit" "Use cargo-audit.txt report; SARIF conversion not yet implemented"
}

# ============================================================================
# Scanner: Semgrep (SAST)
# ============================================================================

scan_semgrep() {
  log "Running Semgrep..."

  if ! command -v semgrep &> /dev/null; then
    warn "semgrep not found; skipping"
    create_sarif_stub "semgrep" "semgrep not found on runner"
    return
  fi

  # Check if rules directory exists and has rules
  if [[ ! -d "$RULES_DIR" ]] || [[ -z "$(find "$RULES_DIR" -name "*.yml" -o -name "*.yaml" 2>/dev/null | head -1)" ]]; then
    warn "No Semgrep rules found in $RULES_DIR; skipping"
    create_sarif_stub "semgrep" "No local Semgrep rules configured"
    return
  fi

  # --config points at a local directory (never the Semgrep registry), so
  # no rule download happens regardless of --metrics; --metrics=off is set
  # explicitly anyway rather than relying on the "auto" default. (There is
  # no --offline flag — confirmed via `semgrep scan --help` against a real
  # install; it errors "unknown option '--offline'".)
  #
  # --sarif-output produces SARIF natively (with real fingerprints and code
  # snippets) — no custom JSON→SARIF conversion needed.
  #
  # `--sarif` is deliberately NOT passed alongside it. The two are not
  # complementary: --sarif switches stdout to the SARIF document, while
  # --sarif-output writes the same document to a file, so passing both
  # emits it TWICE. Measured against the pinned semgrep 1.171.0 on a
  # one-finding fixture: with --sarif, 866 bytes to stdout and 865 to the
  # file; without it, 330 bytes of human-readable summary to stdout and a
  # byte-identical SARIF file (compared as parsed JSON — equal, whole
  # document, not just `results`).
  #
  # That duplication is not cosmetic. stdout here is piped through `tee`
  # into the CI step log, and on this repository the SARIF document is
  # ~1.4 MB — enough to blow past GitHub's per-step log cap and truncate
  # away everything printed AFTER the scan. On 2026-09-16 that is exactly
  # what happened: `cargo deny` failed on RUSTSEC-2026-0285, this script
  # reported it at the very end via `error` as designed, and that message
  # was among the truncated lines. The job showed "Process completed with
  # exit code 1" over a log whose last visible line was a semgrep rule
  # warning — pointing every reader at the wrong scanner.
  if semgrep scan \
    --config="$RULES_DIR" \
    --sarif-output="$REPORTS_DIR/semgrep.sarif" \
    --no-git-ignore \
    --metrics=off \
    . 2>&1 | tee "$REPORTS_DIR/semgrep.log"; then
    log "Semgrep scan completed (no findings)"
  else
    # Non-zero exit is normal if findings exist
    if [[ -f "$REPORTS_DIR/semgrep.sarif" ]]; then
      log "Semgrep found issues (expected in scans)"
    else
      error "Semgrep scan failed without producing output"
    fi
  fi

  if [[ ! -f "$REPORTS_DIR/semgrep.sarif" ]]; then
    create_sarif_stub "semgrep" "Semgrep scan produced no output"
  fi
}

# ============================================================================
# Scanner: Gitleaks (secrets)
# ============================================================================

scan_gitleaks() {
  log "Running Gitleaks..."

  if ! command -v gitleaks &> /dev/null; then
    warn "gitleaks not found; skipping"
    create_sarif_stub "gitleaks" "gitleaks not found on runner"
    return
  fi

  local scan_opts=()

  # For PR scans, check only the PR branch; for full/scheduled, check all history
  if [[ "$SCAN_TYPE" == "pr" ]]; then
    # Scan commits reachable from HEAD but not from origin/main
    scan_opts+=(--log-opts="origin/main..HEAD")
  fi

  # gitleaks scans git history by commit, not "the current diff" — a
  # since-fixed false positive stays flagged forever on the commit that
  # introduced it, even after a later commit in the same PR corrects it,
  # because that history is still reachable from HEAD. Baselining specific
  # commits (see .ci/baselines/README.md) is the sanctioned fix for that,
  # rather than rewriting history. `[extend] useDefault = true` in the
  # baseline keeps every other commit under full detection.
  if [[ -f "$GITLEAKS_BASELINE" ]]; then
    scan_opts+=(--config="$GITLEAKS_BASELINE")
  fi

  # gitleaks detect scans git history by default (unless --no-git is passed),
  # so no --source flag is needed to select that mode; --source/-s takes a
  # path (default "."), not a "git" keyword.
  if gitleaks detect \
    --report-format=sarif \
    --report-path="$REPORTS_DIR/gitleaks.sarif" \
    "${scan_opts[@]}" \
    2>&1 | tee "$REPORTS_DIR/gitleaks.log"; then
    log "Gitleaks scan completed (no secrets found)"
  else
    # Non-zero exit is normal if secrets detected
    log "Gitleaks found potential secrets (expected in scans)"
  fi
}

# ============================================================================
# Scanner: Trivy config (IaC misconfigurations — Terraform, CloudFormation,
# Kubernetes, Helm, Dockerfile, Ansible)
#
# NOTE: trivy config's default --misconfig-scanners list does NOT include a
# GitHub Actions checker (its scanners are azure-arm, cloudformation,
# dockerfile, helm, kubernetes, terraform, terraformplan-json,
# terraformplan-snapshot, ansible — confirmed against a real trivy binary).
# This repo has none of those IaC file types today, so this scanner
# currently has nothing to check and will always report 0 config files
# found — that's an accurate "not applicable" result, not a bug, and it's
# kept for when/if this repo adds Terraform/Dockerfile/etc. GitHub Actions
# workflow files are covered by actionlint below instead.
# ============================================================================

scan_trivy_config() {
  log "Running Trivy config scanner..."

  if ! command -v trivy &> /dev/null; then
    warn "trivy not found; skipping"
    create_sarif_stub "trivy-config" "trivy not found on runner"
    return
  fi

  # --skip-db-update and --offline-scan are vulnerability-scanning flags
  # (trivy image/fs), not valid for `trivy config` — confirmed via
  # `trivy config --help` against a real binary; passing them is a hard
  # "unknown flag" error, not a graceful no-op.
  if trivy config \
    --format=sarif \
    --output="$REPORTS_DIR/trivy-config.sarif" \
    --skip-version-check \
    . 2>&1 | tee "$REPORTS_DIR/trivy-config.log"; then
    log "Trivy config scan completed (no misconfigs found)"
  else
    log "Trivy config found issues (expected in scans)"
  fi
}

# ============================================================================
# Scanner: actionlint (GitHub Actions workflow correctness)
# ============================================================================

scan_actionlint() {
  log "Running actionlint..."

  if ! command -v actionlint &> /dev/null; then
    warn "actionlint not found; skipping"
    create_sarif_stub "actionlint" "actionlint not found on runner"
    return
  fi

  if [[ ! -f "$ACTIONLINT_SARIF_TEMPLATE" ]]; then
    warn "actionlint SARIF template not found at $ACTIONLINT_SARIF_TEMPLATE; skipping"
    create_sarif_stub "actionlint" "SARIF template missing"
    return
  fi

  # actionlint auto-discovers .github/workflows/*.yml from the project root
  # (detected via git); -format takes a literal Go template string (not a
  # file path), so the vendored template is read into the argument.
  if actionlint \
    -format "$(cat "$ACTIONLINT_SARIF_TEMPLATE")" \
    > "$REPORTS_DIR/actionlint.sarif" 2> "$REPORTS_DIR/actionlint.log"; then
    log "actionlint scan completed (no issues found)"
  else
    # actionlint exits 1 for found issues, 2 for bad CLI args, 3 for fatal
    # errors — only 1 means "ran fine, found something to report."
    exit_code=$?
    if [[ $exit_code -eq 1 ]]; then
      log "actionlint found issues (expected in scans)"
    else
      error "actionlint failed to run (exit $exit_code): $(cat "$REPORTS_DIR/actionlint.log")"
    fi
  fi
}

# ============================================================================
# Main execution
# ============================================================================

log "Starting quality checks (scan_type=$SCAN_TYPE)"
log "Reports directory: $REPORTS_DIR"

# Run all scanners; capture errors but continue to collect all reports
scan_cargo_deny || ((SCAN_ERRORS++))
scan_cargo_audit || ((SCAN_ERRORS++))
scan_semgrep || ((SCAN_ERRORS++))
scan_gitleaks || ((SCAN_ERRORS++))
scan_trivy_config || ((SCAN_ERRORS++))
scan_actionlint || ((SCAN_ERRORS++))

# Merge all SARIF reports
log "Merging SARIF reports..."
python3 "$SCRIPT_DIR/merge-sarif.sh" "$REPORTS_DIR"

log "Quality checks completed"
log "Reports available in: $REPORTS_DIR"
log "Merged report: $REPORTS_DIR/quality.sarif"

if [[ $SCAN_ERRORS -gt 0 ]]; then
  warn "One or more scanners had configuration errors (see above)"
  # Don't fail the script itself; the gate script will decide
fi

# cargo-deny is the one scanner in this file whose findings are a real gate
# rather than informational (see scan_cargo_deny) — checked last, after
# every scanner above has had its chance to run and produce a report, so a
# dependency-risk failure never starves the rest of the pipeline of output.
if [[ $CARGO_DENY_FAILED -ne 0 ]]; then
  error "cargo deny found dependency-risk issues (license/advisory/ban/source) — see $REPORTS_DIR/cargo-deny.txt. This is CrateStack's documented pre-PR gate (CLAUDE.md's \`just all-checks\`, which ends in \`cargo deny check\`)."
fi

exit 0
