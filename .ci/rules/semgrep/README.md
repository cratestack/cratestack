# Semgrep Rules

This directory contains local Semgrep rules for CrateStack static analysis.

Rules are organized by concern and committed to version control. They are used with `semgrep scan --config=.` during offline quality checks.

## Adding Rules

1. Create a new `.yml` file, e.g. `rust-safety.yml`
2. Follow Semgrep YAML structure (see examples below)
3. Commit the file and verify it runs locally with `semgrep scan --config=. --offline`

## Rule Format

```yaml
rules:
  - id: rule-id-in-kebab-case
    pattern: |
      # Pattern matching syntax; see https://semgrep.dev/docs/writing-rules/
    message: Clear, actionable message describing the issue
    languages: [rust]  # or [javascript], [typescript], etc.
    severity: ERROR    # or WARNING, INFO
    metadata:
      cwe: CWE-XXX
      owasp: "A03:2021 – Injection"
      category: security
```

## Example Rules

### Rust: Unwrap Without Error Handling

```yaml
rules:
  - id: rust-unwrap-without-context
    pattern-either:
      - pattern: $OBJ.unwrap()
      - pattern: $OBJ.expect(...)
    message: |
      .unwrap() called without error context.
      Consider using match/if let or ? operator instead.
    languages: [rust]
    severity: WARNING
    metadata:
      category: reliability
```

### TypeScript: Console Logs in Production

```yaml
rules:
  - id: ts-console-log-in-production
    pattern-either:
      - pattern: console.log(...)
      - pattern: console.debug(...)
    message: |
      console.log() should not be in production code.
      Use a structured logger instead.
    languages: [typescript, javascript]
    severity: WARNING
    metadata:
      category: maintainability
```

### Secrets Pattern (as example)

```yaml
rules:
  - id: hardcoded-secret-example
    pattern: |
      $VAR = "...$SECRET_PATTERN..."
    message: Possible hardcoded secret
    languages: [rust, typescript, javascript]
    severity: ERROR
    metadata:
      category: security
```

## Baseline (Allowlist)

Findings can be suppressed per rule via `allowlist.yml`:

```yaml
- id: rust-unwrap-without-context
  paths:
    - crates/example/  # Entire directory
    - src/main.rs      # Specific file
  reason: "Known pattern in legacy code, tracked in issue #123"
  expires: "2025-12-31"
```

Suppressions are version-controlled and expire automatically on the date specified.

## Running Rules Locally

```bash
# Scan the entire project
semgrep scan --config=.ci/rules/semgrep --offline .

# Scan with verbose output
semgrep scan --config=.ci/rules/semgrep --offline --verbose .

# Scan one rule only
semgrep scan --config=.ci/rules/semgrep/rust-safety.yml --offline .

# Generate SARIF output
semgrep scan --config=.ci/rules/semgrep --offline --sarif .
```

## Resources

- [Semgrep Rule Writing](https://semgrep.dev/docs/writing-rules/)
- [Semgrep Registry](https://semgrep.dev/r) (reference only; CrateStack uses local rules only)
- [OWASP Top 10](https://owasp.org/Top10/)
