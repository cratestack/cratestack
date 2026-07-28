# Quality Pipeline Architecture

This document describes CrateStack's offline quality scanning pipeline, which replaces SonarQube Community Edition with GitHub Actions, local scanners, and reviewdog PR reporting.

## Overview

The quality pipeline:
- **Runs offline** on self-hosted or specified runners
- **Scans for SAST, secrets, dependencies, and infrastructure issues** using language-appropriate tools
- **Produces SARIF reports** for structured, parseable output
- **Posts PR checks** via reviewdog with new findings only
- **Retains full reports** as artifacts for history and compliance
- **Fails PRs on error-level findings**, warns on warnings

```
[PR/Push/Schedule] → [GitHub Actions workflow]
                       ↓
                    [Offline Scanners]
                    ├─ Semgrep (SAST)
                    ├─ Gitleaks (secrets)
                    ├─ cargo audit (Rust advisories)
                    ├─ cargo deny (Rust deps)
                    └─ Trivy (config)
                       ↓
                    [SARIF Merge]
                       ↓
                    [Quality Gate]
                    ├─ [PR] → Fail on errors
                    └─ [Main] → Advisory only
                       ↓
                    [reviewdog] → [GitHub PR Check]
                       ↓
                    [Artifact Upload]
```

## Which Tool Replaces What (from SonarQube CE)

| SonarQube Capability | CrateStack Tool(s) | Notes |
|---|---|---|
| **SAST** (static analysis) | Semgrep + clippy + Biome | Semgrep handles patterns; clippy for Rust lint; Biome for TS/JS |
| **Secrets scanning** | Gitleaks | Offline, fast; detects hardcoded secrets in git history |
| **Dependency scanning** | cargo audit + cargo deny | Rust advisory DB + license policy |
| **Container/OS vulns** | Trivy (if applicable) | Offline mode; for GitHub Actions workflow security |
| **Code quality metrics** | Clippy + Biome warnings | Linting and formatting checks |
| **PR reporting** | reviewdog + GitHub Checks | Unified PR check, not individual comments |
| **Dashboards** | Not replicated | This pipeline focuses on scanning + PR gates, not history dashboards |
| **Quality gates** | `.ci/quality/gate.sh` | Fails on error-level findings; warnings advisory |

## Enabled Scanners & Language Coverage

### Semgrep (SAST)

- **Languages:** Rust, TypeScript/JavaScript
- **Rules:** Local-only from `.ci/rules/semgrep/`
- **Output:** SARIF
- **Key capabilities:**
  - Pattern-based code analysis
  - Security and reliability patterns
  - Customizable per-project rules
  - Can't be fooled by minification or obfuscation (unlike string search)

### Gitleaks

- **Scope:** Git history (PR diffs or full on scheduled scans)
- **Detection:** Secrets, API keys, passwords
- **Output:** SARIF
- **Offline:** Yes; uses built-in patterns

### cargo audit

- **Scope:** Rust crate dependencies
- **Detection:** Known security advisories from Rustsec advisory DB
- **Output:** Text (JSON also available)
- **Note:** Requires pre-populated advisory DB on self-hosted runner

### cargo deny

- **Scope:** Rust dependencies + licenses
- **Detection:** Duplicate versions, GPL/copyleft licenses, unknown sources
- **Output:** Text
- **Configuration:** `deny.toml` in project root

### Trivy config

- **Scope:** GitHub Actions workflow files
- **Detection:** Misconfigurations, hardcoded secrets, bad practices
- **Output:** SARIF
- **Offline:** Yes; `--offline-scan` flag

## Offline Guarantee

**Offline means:** Scanners do not download rules, binaries, or databases during a workflow run.

- ✅ **Git/GitHub interaction** (fetch repo, post PR checks) — this is acceptable; it's how CI/CD works
- ❌ **Rule downloads** during scan — not allowed
- ❌ **Package manager installs** (pip, npm, cargo install) — not allowed
- ❌ **External API calls** for scoring or enrichment — not allowed

### Pre-Provisioning Required

Every self-hosted runner must have:
1. Binary tools installed (semgrep, gitleaks, trivy)
2. Vulnerability databases pre-loaded (Rustsec for cargo audit, Trivy's DB)
3. Semgrep rules committed to the repo (`.ci/rules/semgrep/`)
4. Python 3 for SARIF converters

See `.ci/quality/README.md` → "Offline Provisioning" for setup script.

## Data Flow

### Scanning Phase

1. **Checkout:** Fetch repository with full git history (`fetch-depth: 0`)
2. **Verify toolchain:** Confirm required scanners are installed
3. **Run scanners:** Each scanner writes its report to `.ci/quality/reports/`
   - Semgrep → `semgrep.sarif` (converted from JSON)
   - Gitleaks → `gitleaks.sarif`
   - cargo audit → `cargo-audit.txt` (text; SARIF stub created)
   - cargo deny → `cargo-deny.txt`
   - Trivy → `trivy-config.sarif`

### Merge Phase

4. **Merge SARIF:** Consolidate all reports into `.ci/quality/reports/quality.sarif`
   - Deduplicates findings across runs
   - Normalizes paths (relative POSIX)
   - Preserves tool/rule metadata

### Gate Phase

5. **Quality gate:** Evaluate merged SARIF
   - On **PR:** Fail if any error-level *new* findings
   - On **main:** Report advisory; never fail

### Reporting Phase

6. **GitHub PR Check:** reviewdog posts a unified check with:
   - Errors → fails PR
   - Warnings → warning status
   - Notes → informational
   - **Filter:** Only new findings (added lines in PR)

7. **Artifacts:** Reports retained for 30 days
8. **Code Scanning (optional):** Upload to GitHub dashboard if `vars.ENABLE_CODE_SCANNING == 'true'`

## Workflow Triggers

| Trigger | Scan Type | Behavior |
|---|---|---|
| **Pull Request** | `pr` | Scan PR diffs; fail on error-level findings; post PR check |
| **Push to main** | `full` | Full scan; advisory; no PR check (already merged) |
| **Weekly schedule** | `full` | Full scan; Advisory; useful for catching retroactive vulnerabilities |
| **Manual dispatch** | User choice | Run on demand |

## Configuration & Customization

### Adding Semgrep Rules

1. Create `.yml` file in `.ci/rules/semgrep/`
2. Define rules in Semgrep YAML format (see `.ci/rules/semgrep/README.md`)
3. Commit to repo
4. Next workflow run uses the new rules

Example:
```yaml
rules:
  - id: my-security-pattern
    pattern: dangerous_func(...)
    message: Unsafe function call
    languages: [rust, typescript]
    severity: ERROR
```

### Suppressing Findings

1. Identify the finding (tool, rule ID, file)
2. Document suppression in `.ci/baselines/`
3. Include reason and expiration date
4. Commit to repo

Suppressions are version-controlled and reviewed like any other code change.

### Updating Tool Versions

1. For self-hosted runner: Update provisioning script, re-provision runners
2. For workflow: No action needed if tools are already installed
3. For Semgrep rules: Commit new rule files to repo
4. For advisories: Depends on tool (Rustsec is automatic; Trivy needs cache refresh)

See `.ci/quality/README.md` → "Maintenance Schedule" for timing recommendations.

## Quality Gate Logic

### PR Context

```
IF (number of error-level findings in added lines > 0):
  FAIL PR with message "Quality gate failed: error findings introduced"
ELSE:
  PASS PR; post warnings as advisory
```

### Main Branch

```
IF (number of error-level findings > 0):
  LOG advisory message; do NOT fail
ELSE:
  PASS
```

This prevents introducing a regression (error findings must be fixed in the PR), while not blocking main-branch pushes for pre-existing issues.

## Baseline & Suppression Strategy

Every suppression must be:
- **Specific:** Individual files/rules, not blanket ignores
- **Documented:** Reason and associated issue ticket
- **Expiring:** Auto-expires on a set date; forces re-review quarterly

Suppressions are stored in `.ci/baselines/` and reviewed alongside code.

### Example Workflow

1. Quality check finds a finding: `rust-unwrap-in-lib` in `crates/example/lib.rs`
2. Team determines it's safe (analyzed in code review)
3. Add to `baselines/semgrep-allowlist.yml`:
   ```yaml
   - id: rust-unwrap-in-lib
     paths:
       - crates/example/lib.rs
     reason: "Safe in this context; analyzed in PR #999"
     expires: "2025-12-31"
   ```
4. On 2025-12-31, suppression expires; must be renewed or finding comes back

## GitHub Code Scanning (Optional)

GitHub Code Scanning provides a historical dashboard of findings.

### Enabling

1. Set `vars.ENABLE_CODE_SCANNING = true` in repo settings
2. Workflow automatically uploads `quality.sarif` to Code Scanning API
3. Dashboard available at: `github.com/<owner>/<repo>/security/code-scanning`

### Disabling

Simply unset the variable; pipeline continues to function without upload.

### Limitations

- Dashboard shows all findings (not just new ones)
- Historical comparison requires multiple runs
- Deduplication relies on tool + rule ID + path + line number

## Local Development

### Run all checks

```bash
.ci/quality/run.sh
```

### Run one scanner

```bash
semgrep scan --config=.ci/rules/semgrep --offline .
gitleaks detect --source=git --report-format=sarif
cargo audit
trivy config .github/workflows
```

### View merged SARIF

```bash
cat .ci/quality/reports/quality.sarif | jq '.runs[0].results | length'
```

### Suppress findings locally

Add to `.ci/baselines/` files and re-run quality checks.

## Troubleshooting

### "Scanner not found" on CI

**Problem:** Workflow logs show "semgrep not found; skipping"

**Solution:**
1. Verify runner is pre-provisioned (see `.ci/quality/README.md`)
2. Check runner labels in `.github/workflows/quality.yml`
3. Re-run provisioning script on the runner

### SARIF validation error

**Problem:** JSON parsing error in merged report

**Solution:**
1. Validate individual tool SARIFs: `python3 -c "import json; json.load(open('.ci/quality/reports/semgrep.sarif'))"`
2. Check converter logs in `.ci/quality/reports/*.log`
3. Re-run `.ci/quality/run.sh` and inspect output

### reviewdog not posting PR check

**Problem:** PR check is missing but artifacts are present

**Solution:**
1. Verify workflow has `pull-requests: write` permission
2. Check GitHub Actions logs for "reviewdog" errors
3. Confirm `GITHUB_TOKEN` is available (it should be by default)
4. Try re-running the workflow

## Limitations & Tradeoffs

### What This Pipeline Does NOT Provide

1. **Historical dashboards:** Unlike SonarQube, this pipeline doesn't track findings over time
   - Workaround: GitHub Code Scanning (optional; off by default)

2. **Centralized team metrics:** No project-wide quality dashboard
   - Workaround: Aggregate locally with custom scripts

3. **Automatic fixing:** Some tools can auto-fix (e.g., rustfmt), but policy here is read-only
   - Rationale: Avoid surprise changes; developers decide remediation

4. **Fine-grained reporting per module:** Reports are repository-wide
   - Workaround: Filter reports by path after merge

### What This Pipeline DOES Provide

✅ **Offline scanning** — no external dependencies during scan
✅ **Unified PR checks** — one check per PR, not spam of comments
✅ **Actionable findings** — only new findings for PRs
✅ **Suppression control** — version-controlled, auditable allowlists
✅ **Language diversity** — Rust, TypeScript, GitHub Actions
✅ **Low maintenance** — self-hosted, no SaaS account management

## Maintenance & Operations

### Daily

- Workflows run automatically on PR/push
- No manual intervention needed

### Weekly

- Scheduled scans run Sundays 02:00 UTC
- Review findings in `security/code-scanning` (if enabled)

### Quarterly

- Audit `.ci/baselines/` for expired suppressions
- Renew or remove suppressed findings
- Commit baseline cleanup PR

### Annually

- Bump tool versions (cargo audit, semgrep, gitleaks, trivy)
- Review Semgrep rules for new patterns
- Rotate credentials (if any in CI/CD)

## See Also

- [Quality Pipeline Operations](.ci/quality/README.md)
- [Baseline & Suppression](.ci/baselines/README.md)
- [Semgrep Rules](.ci/rules/semgrep/README.md)
- [GitHub Actions Workflow](.github/workflows/quality.yml)
- [OWASP Top 10](https://owasp.org/Top10/)
- [CWE/SANS Top 25](https://cwe.mitre.org/top25/)
