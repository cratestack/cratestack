#!/usr/bin/env bash
# Quality gate — evaluates merged SARIF report and decides whether to fail CI
# Fails on new error-level findings in PRs; warnings do not fail
# On main branch, reports but does not fail (unless explicitly configured)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
REPORTS_DIR="$PROJECT_ROOT/.ci/quality/reports"
BASELINES_DIR="$PROJECT_ROOT/.ci/baselines"
MERGED_SARIF="$REPORTS_DIR/quality.sarif"

log() { echo "[quality-gate] $*" >&2; }
error() { echo "[quality-gate] ERROR: $*" >&2; exit 1; }

# Determine context from GitHub Actions environment
IS_PR="${GITHUB_EVENT_NAME:-}=pull_request"
GITHUB_SHA="${GITHUB_SHA:-}"
HEAD_REF="${GITHUB_HEAD_REF:-}"
BASE_REF="${GITHUB_BASE_REF:-main}"

log "Quality gate evaluation"
log "IS_PR: $IS_PR"
log "MERGED_SARIF: $MERGED_SARIF"

# Verify merged SARIF exists
if [[ ! -f "$MERGED_SARIF" ]]; then
  error "Merged SARIF not found: $MERGED_SARIF"
fi

# Validate SARIF structure
if ! python3 -c "import json; json.load(open('$MERGED_SARIF'))" 2>/dev/null; then
  error "Merged SARIF is not valid JSON"
fi

# Count findings by level
count_findings() {
  local level="$1"
  python3 << EOF
import json
with open("$MERGED_SARIF") as f:
    report = json.load(f)
count = 0
for run in report.get("runs", []):
    for result in run.get("results", []):
        if result.get("level", "warning") == "$level":
            count += 1
print(count)
EOF
}

ERROR_COUNT=$(count_findings "error")
WARNING_COUNT=$(count_findings "warning")
NOTE_COUNT=$(count_findings "note")

log "Findings: $ERROR_COUNT errors, $WARNING_COUNT warnings, $NOTE_COUNT notes"

# For PR checks, fail on new errors only
if [[ "$IS_PR" == "true" ]]; then
  log "PR context detected; checking for new errors"

  # TODO: Implement baseline comparison for "new findings"
  # For now, any error in a PR fails the check
  if [[ $ERROR_COUNT -gt 0 ]]; then
    log "PR contains error-level findings; check will fail"
    echo "ERROR: Quality gate failed due to error-level findings" >&2
    exit 1
  fi

  log "PR quality gate passed (no error-level findings)"
  exit 0
fi

# On main branch, only log findings
log "Main branch context; quality gate is advisory"
if [[ $ERROR_COUNT -gt 0 ]]; then
  log "ADVISORY: Main branch has $ERROR_COUNT error-level findings"
fi

exit 0
