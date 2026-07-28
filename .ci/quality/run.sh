#!/usr/bin/env bash
# Quality check orchestrator — runs all scanners and produces SARIF reports
# Usage: .ci/quality/run.sh [--scan-type=pr|full|scheduled]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
REPORTS_DIR="$PROJECT_ROOT/.ci/quality/reports"
RULES_DIR="$PROJECT_ROOT/.ci/rules/semgrep"
ACTIONLINT_SARIF_TEMPLATE="$PROJECT_ROOT/.ci/rules/actionlint/sarif.tmpl"

SCAN_TYPE="${1:-pr}"
if [[ "$SCAN_TYPE" == --scan-type=* ]]; then
  SCAN_TYPE="${SCAN_TYPE#--scan-type=}"
fi

# Ensure reports directory exists
mkdir -p "$REPORTS_DIR"

log() { echo "[quality] $*" >&2; }
error() { echo "[quality] ERROR: $*" >&2; exit 1; }
warn() { echo "[quality] WARN: $*" >&2; }

# Track errors but don't fail immediately — collect all reports first
SCAN_ERRORS=0

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
  if ! cargo deny check all 2>&1 | tee "$REPORTS_DIR/cargo-deny.txt"; then
    log "cargo deny found issues (expected in scans)"
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
  # --sarif/--sarif-output produce SARIF natively (with real fingerprints
  # and code snippets) — no custom JSON→SARIF conversion needed.
  if semgrep scan \
    --config="$RULES_DIR" \
    --sarif \
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

exit 0
