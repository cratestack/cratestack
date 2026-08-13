# Quality Baselines & Suppressions

This directory contains baseline configurations and allowlists for quality scanners.

## Purpose

Baselines allow suppressing findings that are:
- False positives (the tool is wrong)
- Intentional exceptions (documented and reviewed)
- Legacy code awaiting remediation (tracked in issues)

Every suppression must include:
1. A **reason** (ticket reference or explanation)
2. An **expiration date** (auto-expires if not renewed)
3. Version control tracking (reviewable in git history)

## Files

### gitleaks.toml

Allowlist of secrets and patterns known to be safe or false positives.

Example:

```toml
[allowlist]
regexes = []
commits = ["abc123def456", "fedcba654321"]
paths = []
stopwords = []

[[allowlist.rules]]
id = "secret_id_1"
description = "Test credential in examples; not used in production"
commits = ["deadbeef1234"]
reason = "Issue #456: test data, marked as example only"
expires = "2025-12-31"
```

### semgrep-allowlist.yml

Allowlist of Semgrep findings to suppress.

Example:

```yaml
suppressions:
  - id: rust-unwrap-in-lib
    paths:
      - crates/example/
    reason: "Example code; intentionally simplified for clarity"
    expires: "2025-12-31"

  - id: ts-any-type
    paths:
      - packages/cratestack-cli-npm/src/compat.ts
    reason: "Legacy wrapper; tracked in issue #789 for refactoring"
    expires: "2025-12-31"
```

## Baseline Maintenance

### Quarterly Review

Run this task every 3 months:

1. Check for expired suppressions:
   ```bash
   grep -r "expires:" .ci/baselines/ | awk -F: '{print $NF}' | sort -u | while read date; do
     if [[ $(date -d "$date" +%s) -lt $(date +%s) ]]; then
       echo "EXPIRED: $date"
     fi
   done
   ```

2. For each expired suppression:
   - Investigate if the issue still exists
   - If yes, renew the suppression with a ticket reference
   - If no, remove the suppression entirely

3. Commit the cleaned-up baselines

### Adding a New Suppression

1. Identify the finding (tool, rule ID, location)
2. Verify it's a genuine false positive or intentional exception
3. Add to the appropriate baseline file with:
   - Reason (specific and actionable)
   - Expiration date (typically 90-180 days out)
   - Associated GitHub issue/PR link

Example workflow:

```bash
# Run quality check; find a false positive
.ci/quality/run.sh

# Review the finding
grep -A5 "rust-unwrap-in-lib" .ci/quality/reports/quality.sarif

# Add to baselines/semgrep-allowlist.yml
cat >> .ci/baselines/semgrep-allowlist.yml << EOF
  - id: rust-unwrap-in-lib
    paths:
      - crates/mylib/src/false_positive.rs
    reason: "Safe in this context; analyzed in PR #XYZ"
    expires: "2025-12-31"
EOF

# Re-run quality check
.ci/quality/run.sh

# Verify the finding is now suppressed
grep "false_positive.rs" .ci/quality/reports/quality.sarif
```

## Best Practices

1. **Be specific:** Suppress individual files/rules, not entire directories or tools
2. **Document thoroughly:** Future reviewers should understand the decision
3. **Set expiration dates:** Suppressions are not permanent; they require renewal
4. **Escalate false positives:** If a tool is consistently wrong, file a bug with the tool maintainer
5. **Use version control:** Every baseline change is reviewable and auditable

## See Also

- [Quality Pipeline README](.../quality/README.md)
- [OWASP Top 10](https://owasp.org/Top10/)
- [CWE List](https://cwe.mitre.org/)
