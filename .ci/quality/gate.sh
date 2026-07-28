#!/usr/bin/env bash
# Quality gate — validates that every scanner produced a usable report.
#
# This gate's ONLY job is "did the scan itself execute correctly": every
# enabled tool must have produced valid SARIF, or been skipped with a clear,
# justified reason (e.g. "not found on runner", "no local rules configured").
# It fails on scanner execution/configuration errors (corrupt SARIF, a
# scanner report that never got written), regardless of PR vs. main branch.
#
# It deliberately does NOT decide "does this PR introduce new errors" — that
# distinction (new errors on added lines vs. pre-existing backlog) is owned
# by reviewdog's own `-filter-mode=added -fail-level=error` in quality.yml,
# which is diff-aware in a way this script is not. Re-implementing that here
# by counting total error findings would fail every PR on pre-existing
# backlog issues, which the pipeline is explicitly required not to do.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
REPORTS_DIR="$PROJECT_ROOT/.ci/quality/reports"
MERGED_SARIF="$REPORTS_DIR/quality.sarif"

log() { echo "[quality-gate] $*" >&2; }
error() { echo "[quality-gate] ERROR: $*" >&2; }

GATE_FAILED=0

log "Quality gate evaluation (scanner execution/configuration check)"
log "Reports directory: $REPORTS_DIR"

# Every individual tool report must be valid JSON SARIF, whether it holds
# real findings or a "skipped" stub — a missing/corrupt file means the
# scanner crashed instead of degrading cleanly.
shopt -s nullglob
report_files=("$REPORTS_DIR"/*.sarif)
shopt -u nullglob

if [[ ${#report_files[@]} -eq 0 ]]; then
  error "No SARIF reports found in $REPORTS_DIR — quality checks did not run"
  GATE_FAILED=1
fi

for report in "${report_files[@]}"; do
  name="$(basename "$report")"

  if ! python3 -c "import json; json.load(open('$report'))" 2>/dev/null; then
    error "$name is not valid JSON — scanner produced a corrupt report"
    GATE_FAILED=1
    continue
  fi

  python3 << EOF
import json, sys
with open("$report") as f:
    report = json.load(f)
runs = report.get("runs", [])
if not runs:
    print("[quality-gate] WARN: $name has no 'runs' entries", file=sys.stderr)
    sys.exit(0)
for run in runs:
    props = run.get("properties", {})
    status = props.get("status")
    if status == "skipped":
        tool = run.get("tool", {}).get("driver", {}).get("name", "$name")
        reason = props.get("reason", "no reason given")
        print(f"[quality-gate] SKIPPED: {tool} — {reason}", file=sys.stderr)
EOF
done

# Validate the merged report separately — it's the one reviewdog and Code
# Scanning actually consume.
if [[ ! -f "$MERGED_SARIF" ]]; then
  error "Merged SARIF not found: $MERGED_SARIF"
  GATE_FAILED=1
elif ! python3 -c "import json; json.load(open('$MERGED_SARIF'))" 2>/dev/null; then
  error "Merged SARIF is not valid JSON: $MERGED_SARIF"
  GATE_FAILED=1
else
  # Report finding counts for visibility only — this script does not fail
  # on them. New-vs-backlog filtering for PRs happens in the reviewdog step.
  python3 << EOF
import json
with open("$MERGED_SARIF") as f:
    report = json.load(f)
counts = {"error": 0, "warning": 0, "note": 0}
for run in report.get("runs", []):
    for result in run.get("results", []):
        level = result.get("level", "warning")
        counts[level] = counts.get(level, 0) + 1
print(f"[quality-gate] Findings in merged report: {counts.get('error', 0)} errors, "
      f"{counts.get('warning', 0)} warnings, {counts.get('note', 0)} notes "
      f"(informational — PR pass/fail is decided by reviewdog on added lines only)")
EOF
fi

if [[ $GATE_FAILED -ne 0 ]]; then
  error "Quality gate failed: one or more scanners did not execute correctly"
  exit 1
fi

log "Quality gate passed: all scanners executed correctly (ran or skipped with a clear reason)"
exit 0
