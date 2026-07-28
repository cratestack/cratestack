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
- **Configuration:** Runs with `--offline` to prevent rule downloads

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

- **Purpose:** GitHub Actions workflow security scanning
- **Output:** `trivy-config.sarif`
- **Scope:** `.github/workflows/`
- **Configuration:** Runs with `--offline-scan --skip-db-update`

## Running a Specific Scanner

```bash
# Just Semgrep
semgrep scan --config=.ci/rules/semgrep --sarif --offline .

# Just Gitleaks
gitleaks detect --source=git --report-format=sarif

# Just cargo audit
cargo audit

# Just Trivy config
trivy config --format=sarif .github/workflows
```

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

## Offline Provisioning (Self-Hosted Runner)

The following tools must be pre-installed on any self-hosted runner:

```
Tool              | Version    | Binary/Package
------------------+------------+-----------------------------------
semgrep           | >=1.50.0   | semgrep (GitHub releases)
gitleaks          | >=8.18.0   | gitleaks (GitHub releases)
trivy             | >=0.48.0   | trivy (GitHub releases)
cargo-audit       | >=0.18.0   | cargo install cargo-audit
cargo             | nightly    | From rust-toolchain.toml
python3           | >=3.8      | System package (needed for converters)
```

### Setup Script (Recommended)

Create a provisioning script in your infrastructure:

```bash
#!/bin/bash
# Provision a self-hosted runner with offline quality tools

RUNNER_HOME=/opt/github-runner

# Semgrep
curl -L https://github.com/returntocorp/semgrep/releases/download/v1.50.0/semgrep-1.50.0-alpine-x86_64.tar.gz \
  | tar xz -C /usr/local/bin

# Gitleaks
curl -L https://github.com/gitleaks/gitleaks/releases/download/v8.18.0/gitleaks-linux-x64 \
  -o /usr/local/bin/gitleaks && chmod +x /usr/local/bin/gitleaks

# Trivy
curl -L https://github.com/aquasecurity/trivy/releases/download/v0.48.0/trivy_0.48.0_Linux-64bit.tar.gz \
  | tar xz -C /usr/local/bin

# Rust tools
rustup component add clippy rustfmt
cargo install cargo-audit

# Verify installation
semgrep --version
gitleaks --version
trivy version
cargo audit --version
```

## Offline Database Updates

### Trivy

Trivy caches databases locally. Pre-populate on your runner:

```bash
# On the runner provisioning step
trivy image --skip-update busybox:latest  # Populates the cache
trivy config --skip-update .github/workflows/
```

### Semgrep

Semgrep rules are stored in `.ci/rules/semgrep/` (version-controlled).
To update rules on the runner, commit changes to the repository.

### Gitleaks

Uses built-in patterns (no external database).

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
| Trivy DB sync | Monthly | Ops (via runner provisioning) |
| Baseline cleanup | Quarterly | Security lead |
| Tool version bumps | Annually | DevOps |

## See Also

- [Quality Pipeline Architecture](../../docs/quality-pipeline.md)
- [.github/workflows/quality.yml](../../.github/workflows/quality.yml)
- [Baseline & Suppression Format](./)
