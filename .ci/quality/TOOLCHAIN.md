# Offline Quality Toolchain Manifest

This document specifies the required tools, versions, and offline databases for the quality pipeline.

## Required Tools

| Tool | Version | Source | Checksum/Digest | Notes |
|------|---------|--------|-----------------|-------|
| semgrep | ≥1.50.0 | GitHub Releases | [verify](https://github.com/returntocorp/semgrep/releases) | SAST analysis |
| gitleaks | ≥8.18.0 | GitHub Releases | [verify](https://github.com/gitleaks/gitleaks/releases) | Secrets scanning |
| trivy | ≥0.48.0 | GitHub Releases | [verify](https://github.com/aquasecurity/trivy/releases) | Config + dependency scanning |
| cargo-audit | ≥0.18.0 | crates.io | `cargo install cargo-audit` | Rust advisory scanning |
| reviewdog | ≥0.20.0 | [GitHub Releases](https://github.com/reviewdog/reviewdog/releases) | [verify](https://github.com/reviewdog/reviewdog/releases) | PR check reporting — invoked directly by `quality.yml`, never via `reviewdog/action-setup` (that action's `install.sh` downloads a binary over the network at runtime, which the offline rule forbids) |
| python3 | ≥3.8 | System package | N/A | SARIF conversion scripts |
| cargo | pinned | rust-toolchain.toml | See root `rust-toolchain.toml` | Rust toolchain |

## Offline Databases & Caches

### Trivy Vulnerability Database

Trivy requires a pre-populated local cache to avoid downloads during scans.

**Location:** `~/.cache/trivy/db/` (configurable via `TRIVY_CACHE_DIR`)

**Populating (one-time setup):**

```bash
# Download and cache Trivy's vulnerability databases
trivy image --skip-update busybox:latest
trivy config --skip-update /path/to/scan

# Verify cache is present
ls -lh ~/.cache/trivy/db/
```

**Offline flags used in workflow:**
- `--skip-db-update` — don't check for newer DB
- `--skip-version-check` — don't check for newer Trivy version
- `--offline-scan` — offline mode (explicit)

### Rustsec Advisory Database

cargo audit downloads advisories on first run, but subsequent runs are cached.

**Caching:**

```bash
# First run (requires network)
cargo audit --deny warnings

# Subsequent runs use cache from ~/.cargo/advisory-db/
cargo audit --offline  # Uses cached DB
```

**For true offline operation**, pre-populate the cache during runner provisioning.

### Semgrep Rules

All rules are committed to `.ci/rules/semgrep/` (version-controlled).

Semgrep is configured with `--offline` to prevent rule downloads.

## Platform-Specific Notes

### Linux (GitHub Actions ubuntu-latest)

```bash
# Install all tools (requires sudo/provisioning)
sudo apt-get update
sudo apt-get install -y python3-dev libssl-dev

# semgrep
curl -L https://github.com/returntocorp/semgrep/releases/download/v1.50.0/semgrep-1.50.0-alpine-x86_64.tar.gz | tar xz -C /usr/local/bin

# gitleaks
curl -L https://github.com/gitleaks/gitleaks/releases/download/v8.18.0/gitleaks-linux-x64 -o /usr/local/bin/gitleaks && chmod +x /usr/local/bin/gitleaks

# trivy
curl -L https://github.com/aquasecurity/trivy/releases/download/v0.48.0/trivy_0.48.0_Linux-64bit.tar.gz | tar xz -C /usr/local/bin

# cargo-audit (via rustup)
cargo install cargo-audit --force

# reviewdog (installed once during provisioning; the workflow itself never
# downloads it — see the note on the reviewdog row in Required Tools above)
curl -L https://github.com/reviewdog/reviewdog/releases/download/v0.20.3/reviewdog_0.20.3_Linux_x86_64.tar.gz | tar xz -C /usr/local/bin reviewdog

# Verify
semgrep --version
gitleaks --version
trivy version
cargo audit --version
reviewdog -version
python3 --version
```

### macOS (darwin-x86_64 / darwin-arm64)

```bash
# Install via Homebrew or direct download
brew install semgrep gitleaks trivy
cargo install cargo-audit

# Or direct download:
curl -L https://github.com/returntocorp/semgrep/releases/download/v1.50.0/semgrep-1.50.0-osx-x86_64.zip -o /tmp/semgrep.zip
unzip /tmp/semgrep.zip -d /usr/local/bin
```

## Provisioning Checklist

For a self-hosted runner, verify:

- [ ] semgrep installed and in `$PATH`
- [ ] gitleaks installed and in `$PATH`
- [ ] trivy installed and in `$PATH`
- [ ] cargo-audit installed via `cargo`
- [ ] reviewdog installed and in `$PATH` (workflow invokes the CLI directly, not via `reviewdog/action-setup`)
- [ ] python3 ≥3.8 available
- [ ] Trivy cache pre-populated: `~/.cache/trivy/db/` has files
- [ ] Rustsec cache pre-populated (run `cargo audit` once)
- [ ] Git available and configured
- [ ] Docker (optional, if testcontainers tests run)

## Version Pinning Strategy

- **semgrep, gitleaks, trivy:** Pinned in GitHub Actions workflow (hardcoded versions)
- **cargo-audit:** Latest via `cargo install` (pulled during workflow; consider preinstalling)
- **Semgrep rules:** Version-controlled in `.ci/rules/semgrep/`
- **Rust toolchain:** Pinned in `rust-toolchain.toml`

To update tool versions:

1. Test locally with new version
2. Update workflow and provisioning scripts
3. Commit changes
4. Re-provision runners (if self-hosted)

## Database Update Cadence

| Database | Update Frequency | How | Owner |
|----------|------------------|-----|-------|
| Trivy DB | Monthly | Re-run provisioning script | DevOps/Infra |
| Rustsec advisories | Weekly | Automatic on `cargo audit` | Rust maintainers |
| Semgrep rules | Quarterly | Manual commits to repo | Security team |

## Troubleshooting

### Tool not found during workflow

```bash
# Check runner has the tool
which semgrep || echo "not found"

# Check PATH
echo $PATH

# Re-provision if self-hosted
ssh runner@host sudo /path/to/provision.sh
```

### Trivy offline mode fails

```
error: error downloading DB: failed to download file
```

**Fix:**
1. Ensure `--offline-scan` is used
2. Verify cache: `ls ~/.cache/trivy/db/`
3. Re-populate: `trivy image --skip-update busybox`

### semgrep offline flag not recognized

Semgrep ≤1.49.x doesn't have `--offline`. Upgrade to ≥1.50.0.

```bash
semgrep --version  # Verify version
```

## References

- [Semgrep CLI Reference](https://semgrep.dev/docs/cli-reference/)
- [Gitleaks Documentation](https://github.com/gitleaks/gitleaks)
- [Trivy Documentation](https://aquasecurity.github.io/trivy/)
- [cargo audit Documentation](https://docs.rs/cargo-audit/)
