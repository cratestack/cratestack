#!/usr/bin/env bash
# Quality check orchestrator — runs all scanners and produces SARIF reports
# Usage: .ci/quality/run.sh [--scan-type=pr|full|scheduled]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
REPORTS_DIR="$PROJECT_ROOT/.ci/quality/reports"
RULES_DIR="$PROJECT_ROOT/.ci/rules/semgrep"

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

  if ! command -v cargo &> /dev/null; then
    warn "cargo not found; skipping cargo deny"
    create_sarif_stub "cargo-deny" "cargo not found on runner"
    return
  fi

  # cargo deny doesn't produce SARIF directly; capture human output
  if ! cargo deny check --all 2>&1 | tee "$REPORTS_DIR/cargo-deny.txt"; then
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

  # Use --offline flag to prevent rule downloading; only use local rules
  if semgrep scan \
    --config="$RULES_DIR" \
    --json \
    --output="$REPORTS_DIR/semgrep-raw.json" \
    --no-git-ignore \
    --offline \
    . 2>&1 | tee "$REPORTS_DIR/semgrep.log"; then
    log "Semgrep scan completed (no findings)"
  else
    # Non-zero exit is normal if findings exist
    if [[ -f "$REPORTS_DIR/semgrep-raw.json" ]]; then
      log "Semgrep found issues (expected in scans)"
    else
      error "Semgrep scan failed without producing output"
    fi
  fi

  # Convert semgrep JSON to SARIF using a small utility
  if [[ -f "$REPORTS_DIR/semgrep-raw.json" ]]; then
    python3 "$SCRIPT_DIR/semgrep-to-sarif.py" \
      "$REPORTS_DIR/semgrep-raw.json" \
      "$REPORTS_DIR/semgrep.sarif"
  else
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

  if gitleaks detect \
    --source=git \
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
# Scanner: Trivy config (GitHub Actions workflows)
# ============================================================================

scan_trivy_config() {
  log "Running Trivy config scanner..."

  if ! command -v trivy &> /dev/null; then
    warn "trivy not found; skipping"
    create_sarif_stub "trivy-config" "trivy not found on runner"
    return
  fi

  # Scan .github/workflows for misconfigurations
  if trivy config \
    --format=sarif \
    --output="$REPORTS_DIR/trivy-config.sarif" \
    --skip-db-update \
    --offline-scan \
    --skip-version-check \
    .github/workflows 2>&1 | tee "$REPORTS_DIR/trivy-config.log"; then
    log "Trivy config scan completed (no misconfigs found)"
  else
    log "Trivy config found issues (expected in scans)"
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
