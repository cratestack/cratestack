# Quality Pipeline Architecture

This document describes CrateStack's offline quality scanning pipeline, which replaces SonarQube Community Edition with GitHub Actions, local scanners, and reviewdog PR reporting.

## Overview

The quality pipeline:
- **Runs on GitHub-hosted `ubuntu-latest`** — no self-hosted runner or pre-provisioning needed. Every scanner is installed fresh at the start of each job via pinned GitHub Actions or checksum-verified downloads (see `.ci/quality/TOOLCHAIN.md`)
- **Scans for SAST, secrets, dependencies, and GitHub Actions correctness** using language-appropriate tools
- **Produces SARIF reports** for structured, parseable output
- **Posts PR checks** via reviewdog with new findings only
- **Retains full reports** as artifacts for history and compliance
- **Fails PRs on new error-level findings**, warns on warnings

```
[PR/Push/Schedule] → [GitHub Actions workflow]
                       ↓
                    [Install Scanners]
                    (pinned actions / checksum-verified downloads)
                       ↓
                    [Run Scanners]
                    ├─ Semgrep (SAST)
                    ├─ Gitleaks (secrets)
                    ├─ cargo audit (Rust advisories)
                    ├─ cargo deny (Rust deps/licenses)
                    ├─ Trivy (IaC config — no matching file types yet)
                    └─ actionlint (GitHub Actions correctness)
                       ↓
                    [SARIF Merge]
                       ↓
                    [Quality Gate (gate.sh)]
                    scanner execution/config health only
                       ↓
                    [reviewdog] → [GitHub PR Check]
                    fails only on new errors on added lines (PRs only)
                       ↓
                    [Artifact Upload]
```

## Which Tool Replaces What (from SonarQube CE)

| SonarQube Capability | CrateStack Tool(s) | Notes |
|---|---|---|
| **SAST** (static analysis) | Semgrep + clippy + Biome | Semgrep handles patterns; clippy for Rust lint; Biome for TS/JS |
| **Secrets scanning** | Gitleaks | Fast; detects hardcoded secrets in git history |
| **Dependency scanning** | cargo audit + cargo deny | Rust advisory DB + license policy |
| **GitHub Actions correctness** | actionlint | Syntax/semantic checks for workflow files (Trivy's config scanner doesn't cover GitHub Actions — see below) |
| **Code quality metrics** | Clippy + Biome warnings | Linting and formatting checks |
| **PR reporting** | reviewdog + GitHub Checks | Unified PR check, not individual comments |
| **Dashboards** | Not replicated | This pipeline focuses on scanning + PR gates, not history dashboards |
| **Quality gates** | `.ci/quality/gate.sh` + reviewdog | `gate.sh` fails on scanner execution errors; reviewdog fails the PR check on new error-level findings on added lines only |

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

### cargo audit

- **Scope:** Rust crate dependencies
- **Detection:** Known security advisories from the Rustsec advisory DB
- **Output:** Text (JSON also available; SARIF conversion not yet implemented)

### cargo deny

- **Scope:** Rust dependencies + licenses
- **Detection:** Duplicate versions, GPL/copyleft licenses, unknown sources
- **Output:** Text
- **Configuration:** `deny.toml` in project root

### Trivy config

- **Scope:** IaC misconfigurations — Terraform, CloudFormation, Kubernetes, Helm, Dockerfile, Ansible
- **Detection:** Misconfigurations and bad practices in the file types above
- **Output:** SARIF
- **Note:** This repo has none of those file types yet, so it currently reports "0 config files found" — an accurate result, not a bug. It does **not** cover GitHub Actions workflows (see actionlint below).

### actionlint

- **Scope:** `.github/workflows/*.yml`
- **Detection:** Syntax errors, invalid expressions, shellcheck issues in `run:` blocks, permission/typing mistakes
- **Output:** SARIF, via a vendored official template (`.ci/rules/actionlint/sarif.tmpl`)

## Toolchain

Every scanner is installed at the start of the `quality` job — see `.ci/quality/TOOLCHAIN.md` for the exact install method, pinned version, and checksum for each tool, plus a list of real CLI-flag mistakes that were only caught by actually running each tool against this repo (several looked correct on paper but weren't).

"Offline" in this pipeline's naming means *no centralized SonarQube-style SaaS server* — not zero network access. Installing pinned tool versions during a run is fine; what's avoided is a hosted quality-analysis platform and floating/unpinned dependencies.

## Data Flow

### Scanning Phase

1. **Checkout:** Fetch repository with full git history (`fetch-depth: 0`)
2. **Install scanners:** Each tool is installed fresh via pinned actions/downloads (see `.ci/quality/TOOLCHAIN.md`)
3. **Verify toolchain:** Confirm every install actually succeeded — fails the job with a clear message if not
4. **Run scanners:** Each scanner writes its report to `.ci/quality/reports/`
   - Semgrep → `semgrep.sarif` (converted from JSON)
   - Gitleaks → `gitleaks.sarif`
   - cargo audit → `cargo-audit.txt` (text; SARIF stub created — conversion not yet implemented)
   - cargo deny → `cargo-deny.txt`
   - Trivy → `trivy-config.sarif`
   - actionlint → `actionlint.sarif`

### Merge Phase

5. **Merge SARIF:** Consolidate all reports into `.ci/quality/reports/quality.sarif`
   - Deduplicates findings keyed on (tool, rule ID, path, line, message) — this also guards against a previous local `run.sh` invocation's leftover `quality.sarif` being folded into a fresh merge, since it matches the same `*.sarif` glob as the inputs
   - Normalizes paths (relative POSIX)
   - Preserves tool/rule metadata

### Gate Phase

6. **Quality gate (`gate.sh`):** Confirms every scanner produced valid SARIF or skipped with a justified reason — fails on execution/configuration errors only, never on finding counts (see "Quality Gate Logic" below)

### Reporting Phase

7. **GitHub PR Check:** reviewdog posts a unified check with:
   - Errors → fails PR
   - Warnings → warning status
   - Notes → informational
   - **Filter:** Only new findings (added lines in PR)

8. **Artifacts:** Reports retained for 30 days
9. **Code Scanning (optional):** Upload to GitHub dashboard if `vars.ENABLE_CODE_SCANNING == 'true'`

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

1. Update the pinned version/SHA in `.github/workflows/quality.yml`'s install steps
2. For checksum-verified downloads (gitleaks, actionlint): fetch the new release's checksums file and update the pinned sha256 alongside the version
3. For Semgrep rules: commit new rule files to `.ci/rules/semgrep/`
4. Test with a `workflow_dispatch` run — no runner provisioning needed

See `.ci/quality/TOOLCHAIN.md` for the exact pin locations and `.ci/quality/README.md` → "Maintenance Schedule" for timing recommendations.

## Quality Gate Logic

Two separate mechanisms make up "the gate," each owning a distinct question:

### `.ci/quality/gate.sh` — did the scan itself execute correctly?

Runs on every trigger (PR, push, schedule). Its only question is whether each
enabled scanner produced valid SARIF or was skipped with a clear, justified
reason (tool not found, no local rules configured, etc.):

```
FOR each *.sarif report in .ci/quality/reports/:
  IF report is missing or not valid JSON:
    FAIL — scanner execution/configuration error
  IF report.properties.status == "skipped":
    LOG the skip reason (informational, does not fail)
Merged quality.sarif finding counts are logged for visibility only —
this script never fails a build because of finding counts.
```

### reviewdog — does this PR introduce new errors?

The `Post PR check via reviewdog` step in `quality.yml` owns the PR-vs-backlog
distinction, using reviewdog's own diff-aware filtering rather than
re-counting findings:

```
reviewdog -f=sarif -reporter=github-pr-check \
  -filter-mode=added -fail-level=error < quality.sarif

IF any error-level finding lands on an added/modified line:
  exit 1 → step fails → PR check fails
ELSE:
  exit 0 (warnings/notes are still posted as annotations, non-blocking)
```

This step only runs on `pull_request` events, so pushes to `main` and
scheduled scans are never gated on finding counts — they rely solely on
`gate.sh`'s execution-correctness check. This is deliberate: re-implementing
"new vs. backlog" in `gate.sh` by counting total errors would fail every PR
on pre-existing repository backlog, which the pipeline is explicitly required
not to do.

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
semgrep scan --config=.ci/rules/semgrep --metrics=off .
gitleaks detect --report-format=sarif --report-path=/tmp/gitleaks.sarif
cargo audit
cargo deny check all
trivy config --format=sarif --output=/tmp/trivy.sarif .
actionlint -format "$(cat .ci/rules/actionlint/sarif.tmpl)"
```

(These commands assume the tool is already installed locally — see `.ci/quality/TOOLCHAIN.md` for each tool's exact version and install method.)

### View merged SARIF

```bash
cat .ci/quality/reports/quality.sarif | jq '.runs[0].results | length'
```

### Suppress findings locally

Add to `.ci/baselines/` files and re-run quality checks.

## Troubleshooting

### "Tool not found" during "Verify toolchain"

**Problem:** Workflow logs show "error: semgrep not found — its install step above must have failed"

**Solution:** This means the install step failed, not that a runner needs provisioning (there's no self-hosted runner in this design). Check the specific install step's own log for the actual error — a checksum mismatch, a changed download URL, a yanked pip version, etc. See `.ci/quality/TOOLCHAIN.md` → "Troubleshooting."

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

5. **Trivy doesn't cover GitHub Actions:** its config scanner's default checks are Terraform/CloudFormation/Kubernetes/Helm/Dockerfile/Ansible — none of which this repo has yet, so it currently finds "0 config files." GitHub Actions correctness is covered by actionlint instead.

### What This Pipeline DOES Provide

✅ **No SaaS quality platform** — runs entirely in GitHub Actions, no external server or account
✅ **Unified PR checks** — one check per PR, not spam of comments
✅ **Actionable findings** — only new findings for PRs
✅ **Suppression control** — version-controlled, auditable allowlists
✅ **Language diversity** — Rust, TypeScript, GitHub Actions
✅ **Low maintenance** — no self-hosted runner to provision or keep patched; tool updates are a one-line version bump in `quality.yml`

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
- [Toolchain Manifest](.ci/quality/TOOLCHAIN.md)
- [Baseline & Suppression](.ci/baselines/README.md)
- [Semgrep Rules](.ci/rules/semgrep/README.md)
- [GitHub Actions Workflow](.github/workflows/quality.yml)
- [OWASP Top 10](https://owasp.org/Top10/)
- [CWE/SANS Top 25](https://cwe.mitre.org/top25/)
