#!/usr/bin/env bash
# Validation script — checks that the quality pipeline is properly configured
# and all components are in place.

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

PASSED=0
FAILED=0
WARNINGS=0

log_pass() { echo "✓ $*"; PASSED=$((PASSED + 1)); }
log_fail() { echo "✗ $*"; FAILED=$((FAILED + 1)); }
log_warn() { echo "⚠ $*"; WARNINGS=$((WARNINGS + 1)); }
section() { echo ""; echo "=== $* ==="; }

# ============================================================================
# Validate directory structure
# ============================================================================

section "Directory Structure"

check_dir() {
  if [[ -d "$1" ]]; then
    log_pass "Directory exists: $1"
  else
    log_fail "Directory missing: $1"
  fi
}

check_file() {
  if [[ -f "$1" ]]; then
    log_pass "File exists: $1"
  else
    log_fail "File missing: $1"
  fi
}

check_dir "$PROJECT_ROOT/.ci/quality"
check_dir "$PROJECT_ROOT/.ci/rules/semgrep"
check_dir "$PROJECT_ROOT/.ci/baselines"
check_file "$PROJECT_ROOT/.github/workflows/quality.yml"
check_file "$PROJECT_ROOT/docs/quality-pipeline.md"

# ============================================================================
# Validate scripts
# ============================================================================

section "Scripts"

check_file "$SCRIPT_DIR/run.sh"
check_file "$SCRIPT_DIR/merge-sarif.sh"
check_file "$SCRIPT_DIR/gate.sh"
check_file "$SCRIPT_DIR/semgrep-to-sarif.py"

# Check executability
for script in run.sh merge-sarif.sh gate.sh semgrep-to-sarif.py; do
  if [[ -x "$SCRIPT_DIR/$script" ]]; then
    log_pass "Script is executable: $script"
  else
    log_warn "Script is not executable: $script (run: chmod +x .ci/quality/$script)"
  fi
done

# ============================================================================
# Validate YAML files
# ============================================================================

section "YAML Validation"

# Workflow
if python3 -c "import yaml; yaml.safe_load(open('$PROJECT_ROOT/.github/workflows/quality.yml'))" 2>/dev/null; then
  log_pass "GitHub Actions workflow YAML is valid"
else
  log_fail "GitHub Actions workflow YAML is invalid"
fi

# Semgrep rules
for yml in "$PROJECT_ROOT"/.ci/rules/semgrep/*.yml; do
  if python3 -c "import yaml; yaml.safe_load(open('$yml'))" 2>/dev/null; then
    log_pass "Semgrep rules valid: $(basename "$yml")"
  else
    log_fail "Semgrep rules invalid: $(basename "$yml")"
  fi
done

# ============================================================================
# Validate Python scripts
# ============================================================================

section "Python Scripts"

for script in merge-sarif.sh semgrep-to-sarif.py; do
  if python3 -m py_compile "$SCRIPT_DIR/$script" 2>/dev/null; then
    log_pass "Python script compiles: $script"
  else
    log_fail "Python script compilation failed: $script"
  fi
done

# ============================================================================
# Validate shell scripts
# ============================================================================

section "Shell Scripts"

for script in run.sh gate.sh; do
  if bash -n "$SCRIPT_DIR/$script" 2>/dev/null; then
    log_pass "Shell script syntax valid: $script"
  else
    log_fail "Shell script syntax error: $script"
  fi
done

# ============================================================================
# Check tool availability (warnings only)
# ============================================================================

section "Tool Availability"

check_tool() {
  if command -v "$1" &> /dev/null; then
    version=$("$1" --version 2>&1 | head -1 || echo "unknown version")
    log_pass "Tool found: $1 ($version)"
  else
    log_warn "Tool not found: $1 (required for scans; install via provisioning)"
  fi
}

check_tool semgrep
check_tool gitleaks
check_tool trivy
check_tool cargo-audit
check_tool python3

# ============================================================================
# Configuration checks
# ============================================================================

section "Configuration"

if [[ -f "$PROJECT_ROOT/deny.toml" ]]; then
  log_pass "Rust dependency policy: deny.toml"
else
  log_warn "Rust dependency policy not found: deny.toml"
fi

if [[ -f "$PROJECT_ROOT/biome.json" ]]; then
  log_pass "JS/TS linting: biome.json"
else
  log_warn "JS/TS linting config not found: biome.json"
fi

if [[ -f "$PROJECT_ROOT/rust-toolchain.toml" ]]; then
  log_pass "Rust toolchain pinned: rust-toolchain.toml"
else
  log_warn "Rust toolchain not pinned: rust-toolchain.toml"
fi

# ============================================================================
# Semgrep rules
# ============================================================================

section "Semgrep Rules"

rule_files=$(find "$PROJECT_ROOT/.ci/rules/semgrep" -name "*.yml" -type f | wc -l)
rule_count=$(grep -r "^  - id:" "$PROJECT_ROOT/.ci/rules/semgrep" 2>/dev/null | wc -l || echo "0")

if [[ $rule_files -gt 0 ]]; then
  log_pass "Semgrep rules configured: $rule_files files, $rule_count rules"
else
  log_fail "No Semgrep rules found in .ci/rules/semgrep/"
fi

# ============================================================================
# Summary
# ============================================================================

section "Validation Summary"

echo ""
echo "Results:"
echo "  ✓ Passed: $PASSED"
echo "  ✗ Failed: $FAILED"
echo "  ⚠ Warnings: $WARNINGS"
echo ""

if [[ $FAILED -eq 0 ]]; then
  echo "✓ Quality pipeline is properly configured"
  exit 0
else
  echo "✗ Quality pipeline has $FAILED configuration error(s)"
  echo ""
  echo "Next steps:"
  echo "  1. Address failed checks above"
  echo "  2. For tool warnings: see .ci/quality/README.md → Offline Provisioning"
  echo "  3. Run .ci/quality/run.sh to execute scans"
  exit 1
fi
