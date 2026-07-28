# Quality Pipeline Quick Start

## For Developers

### Run quality checks locally

```bash
.ci/quality/run.sh
```

Reports appear in `.ci/quality/reports/`:
- `quality.sarif` — merged findings (for reviewdog)
- `semgrep.sarif`, `gitleaks.sarif`, etc. — individual tool reports
- `*.log` — tool execution logs

### See what was found

```bash
# Count findings by level
jq '.runs[].results | group_by(.level) | map({level: .[0].level, count: length})' \
  .ci/quality/reports/quality.sarif

# List all findings
jq '.runs[].results[] | {rule: .ruleId, file: .locations[0].physicalLocation.artifactLocation.uri, message: .message.text}' \
  .ci/quality/reports/quality.sarif
```

### Suppress a finding

Suppress findings that are false positives or intentional exceptions:

1. Add to `.ci/baselines/semgrep-allowlist.yml`:

```yaml
- id: rust-unwrap-in-lib
  paths:
    - crates/mylib/src/file.rs
  reason: "Safe in this context; tracked in issue #123"
  expires: "2025-12-31"
```

2. Re-run: `.ci/quality/run.sh`
3. Verify: The finding should no longer appear

### Validate setup

```bash
.ci/quality/validate.sh
```

Should show:
- ✓ 26 checks passed
- ⚠ Tool warnings (expected if not provisioned)

## For PR Reviewers

### GitHub PR Check

When you open a PR to `main`, a **Quality** check appears in GitHub:

- 🟢 **Pass:** No new error-level findings
- 🔴 **Fail:** Error-level findings introduced
- 🟡 **Warning:** Warning-level findings (PR can still merge)

### Viewing findings

1. Click the **Quality** check in the PR
2. See all new findings from the workflow
3. Or download artifacts (`quality-reports.zip`) for full reports

### Common findings

| Rule | Means | Typical Fix |
|------|-------|-------------|
| `rust-unwrap-in-lib` | .unwrap() in library code | Use `?` operator or return Result |
| `rust-unsafe-without-comment` | unsafe block without docs | Add safety comment above block |
| `ts-any-type` | TypeScript any used | Use concrete type or unknown |
| `ts-hardcoded-secrets` | Hardcoded API key/token | Move to env var |
| `gitleaks` | Secret in git history | Remove from code or update baseline |

## For DevOps/Infra

### Replace SonarQube CE

1. Decommission SonarQube CE server (no longer needed)
2. (Optional) Remove `SONARQUBE_*` secrets from GitHub
3. New quality checks run in GitHub Actions (built-in)

### Use a self-hosted runner

By default, quality checks run on GitHub-hosted runners (`ubuntu-latest`).

For complete scanning (all tools available), configure a self-hosted runner:

1. Set runner label in `.github/workflows/quality.yml`:
   ```yaml
   runs-on: [self-hosted, linux, quality-offline]
   ```

2. Provision runner:
   ```bash
   # On the runner machine
   bash .ci/quality/TOOLCHAIN.md  # Follow provisioning script
   ```

3. Verify:
   ```bash
   .ci/quality/validate.sh
   ```

### Update tool versions

1. Edit `.ci/quality/TOOLCHAIN.md`
2. Update version numbers and checksums
3. Re-run provisioning script on runners
4. Commit changes

## Troubleshooting

### "Scanner not found; skipping"

**On GitHub-hosted runners:** Normal. Scanners aren't pre-installed.  
**On self-hosted runners:** Run provisioning script from `.ci/quality/TOOLCHAIN.md`.

### My finding is a false positive

Add to `.ci/baselines/` with reason and expiration date. See **Suppress a finding** above.

### I need to change Semgrep rules

1. Edit `.ci/rules/semgrep/*.yml`
2. Test locally: `.ci/quality/run.sh`
3. Commit and push

### Pipeline is slow

Quality checks run in parallel. Some tools (Semgrep, Trivy) may take 1-2 minutes on large codebases. This is normal.

## Reference

- [Quality Pipeline Architecture](../docs/quality-pipeline.md)
- [Operations Manual](README.md)
- [Tool Provisioning](TOOLCHAIN.md)
- [Validation Script](validate.sh)
- [Semgrep Rules](.ci/rules/semgrep/)
- [Baselines](.ci/baselines/)

## Questions?

See [docs/quality-pipeline.md](../../docs/quality-pipeline.md) for comprehensive documentation.
