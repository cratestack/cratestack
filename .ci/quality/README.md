# Quality Pipeline Operations

This directory contains scripts for running offline quality checks and CI/CD integration.

## Quick Start

### Run locally (all scanners)

```bash
.ci/quality/run.sh
```

Reports are written to `.ci/quality/reports/`:
- `quality.sarif` — merged findings from all tools
- Individual tool reports (e.g., `semgrep.sarif`, `gitleaks.sarif`, `cargo-audit.txt`)

### Run in GitHub Actions

The workflow is triggered on:
- Pull requests to `main`
- Pushes to `main`
- Manual dispatch (`workflow_dispatch`)
- Weekly schedule (default: Sundays 02:00 UTC)

See `.github/workflows/quality.yml` for full configuration.

## Scanner Details

### Semgrep (SAST)

- **Rules:** Local only from `.ci/rules/semgrep/`
- **Languages:** Rust, TypeScript
- **Output:** `semgrep.sarif` (converted from Semgrep JSON)
- **Configuration:** `--config` always points at the local rules directory, never the Semgrep registry, so no rule download happens; `--metrics=off` is also set explicitly (there is no `--offline` flag)

To add rules:

1. Create `.yml` files in `.ci/rules/semgrep/`
2. Example structure:
   ```yaml
   rules:
     - id: unsafe-pattern
       pattern: dangerous_call()
       message: Unsafe pattern detected
       languages: [rust]
       severity: ERROR
   ```

### Gitleaks (Secrets)

- **Rules:** Built-in secret patterns
- **Scope:** PR diffs (origin/main..HEAD) or full history on scheduled scans
- **Output:** `gitleaks.sarif`
- **Configuration:** `.ci/baselines/gitleaks.toml` (optional allowlist)

To suppress a finding:

1. Edit `.ci/baselines/gitleaks.toml`
2. Add rule ID + reason + expiration date

### cargo audit & cargo deny

- **Purpose:** Rust dependency vulnerability scanning
- **Output:** Text reports (`cargo-audit.txt`, `cargo-deny.txt`)
- **Configuration:** `Cargo.toml` and `deny.toml` in project root
- **SARIF:** Converted from text output (basic stub)

### Trivy config

- **Purpose:** IaC misconfiguration scanning (Terraform, CloudFormation, Kubernetes, Helm, Dockerfile, Ansible)
- **Output:** `trivy-config.sarif`
- **Scope:** Whole repo (`trivy config .`)
- **Note:** Its default scanners don't cover GitHub Actions — this repo has none of the covered file types yet, so it currently reports "0 config files found." GitHub Actions correctness is covered by actionlint instead. (`--skip-db-update`/`--offline-scan` are vulnerability-scanning flags, not valid for `trivy config`.)

### actionlint (GitHub Actions correctness)

- **Purpose:** Lints `.github/workflows/*.yml` for syntax errors, bad expressions, shellcheck issues in `run:` blocks
- **Output:** `actionlint.sarif`, via the vendored official template at `.ci/rules/actionlint/sarif.tmpl`
- **Scope:** Auto-discovers workflow files from the project root

## Running a Specific Scanner

```bash
# Just Semgrep
semgrep scan --config=.ci/rules/semgrep --metrics=off .

# Just Gitleaks
gitleaks detect --report-format=sarif --report-path=/tmp/gitleaks.sarif

# Just cargo audit
cargo audit

# Just cargo deny
cargo deny check all

# Just Trivy config
trivy config --format=sarif --output=/tmp/trivy.sarif .

# Just actionlint
actionlint -format "$(cat .ci/rules/actionlint/sarif.tmpl)"
```

Each command assumes the tool is already installed locally — see `.ci/quality/TOOLCHAIN.md` for exact versions and install methods.

## SARIF Report Structure

The merged `quality.sarif` contains:
- Multiple `runs`, one per scanner
- Normalized paths (relative to project root)
- Standard SARIF 2.1.0 schema

Example query:

```python
import json
with open('.ci/quality/reports/quality.sarif') as f:
    report = json.load(f)
    for run in report['runs']:
        tool_name = run['tool']['driver']['name']
        findings = run['results']
        print(f"{tool_name}: {len(findings)} findings")
```

## Reviewer Reporting (reviewdog)

The GitHub Actions workflow calls reviewdog with:

```bash
reviewdog -f=sarif \
  -reporter=github-pr-check \
  -filter-mode=added \
  -fail-on-error
```

This posts one PR check with:
- All new error-level findings → fails the PR
- All new warning/note findings → warning status, does not fail
- Uses GitHub Check annotations (not individual review comments)

## Baseline & Suppression

Findings can be suppressed using `.ci/baselines/`:

1. **Gitleaks:** Edit `.ci/baselines/gitleaks.toml`
   ```toml
   [allowlist]
   regexes = ["pattern-hash"]  # Base64 commit SHA
   commits = ["abc123def456"]
   ```

2. **Semgrep:** Edit `.ci/rules/semgrep/allowlist.yml`
   ```yaml
   - id: rule-id
     paths:
       - crates/example/
     reason: "Known false positive, tracked in issue #123"
     expires: "2025-12-31"
   ```

3. **General approach:**
   - Every suppression must have an expiration date
   - Document the reason (ticket reference, false positive explanation)
   - Review on every quarter sweep

## Toolchain

There is no self-hosted runner and no pre-provisioning step. Every tool
(semgrep, gitleaks, trivy, cargo-audit, cargo-deny, actionlint, reviewdog) is
installed fresh at the start of each `quality` job run, on GitHub-hosted
`ubuntu-latest`, via pinned GitHub Actions or checksum-verified downloads —
see the "Install scanners" steps in `.github/workflows/quality.yml`.

"Offline" in this pipeline's naming means *no centralized SonarQube-style
SaaS server* — not zero network access. Installing pinned tool versions
during a run is fine; what's avoided is a hosted quality-analysis platform.

See `.ci/quality/TOOLCHAIN.md` for the exact version/checksum pinned for
each tool, why each install method was chosen, and a list of real CLI-flag
mistakes only caught by actually running each tool (several looked correct
on paper but weren't).

## GitHub Code Scanning (Optional)

To enable optional GitHub Code Scanning upload:

1. Set `vars.ENABLE_CODE_SCANNING = true` in repo settings
2. Workflow will upload `quality.sarif` to GitHub's code scanning dashboard
3. Dashboard URL: `https://github.com/owner/repo/security/code-scanning`

This is separate from PR checks and uses a different SARIF category.

## Troubleshooting

### "Scanner not found" warnings

- Ensure the runner has the tool installed
- Check `$PATH` for conflicts
- Run `which <tool>` to verify

### Empty reports

- Check `.ci/quality/reports/<tool>.log` for scan errors
- Verify the tool can run: `<tool> --version`
- For Semgrep, confirm rules exist in `.ci/rules/semgrep/`

### SARIF validation errors

Run:

```bash
python3 -c "import json; json.load(open('.ci/quality/reports/quality.sarif'))"
```

If the JSON is valid but SARIF structure is wrong, check individual tool reports.

### reviewdog not posting PR checks

- Verify `GITHUB_TOKEN` is available to the workflow
- Check GitHub Actions logs: `reviewdog: posting PR check`
- Ensure the workflow has `pull-requests: write` permission

## Maintenance Schedule

| Task | Frequency | Owner |
|------|-----------|-------|
| Semgrep rules update | Quarterly | Security lead |
| Baseline cleanup | Quarterly | Security lead |
| Tool version bumps (see TOOLCHAIN.md) | Annually | DevOps |

## See Also

- [Quality Pipeline Architecture](../../docs/quality-pipeline.md)
- [Toolchain Manifest](TOOLCHAIN.md)
- [.github/workflows/quality.yml](../../.github/workflows/quality.yml)
- [Baseline & Suppression Format](./)
