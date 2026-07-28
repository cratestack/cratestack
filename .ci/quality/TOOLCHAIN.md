# Quality Toolchain Manifest

This document specifies the required tools, versions, and how each one is installed for the quality pipeline.

**Architecture:** every tool is installed fresh at the start of each `quality` job run, via pinned GitHub Actions or checksum-verified downloads — see the "Install scanners" steps in `.github/workflows/quality.yml`. There is no self-hosted runner and no pre-provisioning step; the job runs on GitHub-hosted `ubuntu-latest`. "Offline" in this project means *no centralized SonarQube-style SaaS server* — it does not mean zero network access during a run. GitHub Actions, and the tool installs below, still need outbound access to fetch their own pinned releases, same as any other action in this repo.

## Required Tools

| Tool | Version | Installed via | Notes |
|------|---------|----------------|-------|
| semgrep | 1.171.0 | `actions/setup-python` + `pip install semgrep==1.171.0` | SAST analysis; ships prebuilt Linux wheels, no compilation |
| gitleaks | 8.30.1 | Checksum-verified direct download from GitHub Releases | Secrets scanning. `gitleaks/gitleaks-action` v3+ is a **commercial product** under a proprietary EULA as of this writing — not usable here. The underlying `gitleaks` CLI itself is still free/MIT-licensed |
| trivy | latest via `taiki-e/install-action` | `taiki-e/install-action` (`tool: trivy`) | Config/misconfiguration scanning. Its default `--misconfig-scanners` list is `azure-arm, cloudformation, dockerfile, helm, kubernetes, terraform, terraformplan-json, terraformplan-snapshot, ansible` — it does **not** include GitHub Actions; this repo has none of the covered IaC types today, so it currently reports "0 config files found" (accurate, not a bug) |
| actionlint | 1.7.12 | Checksum-verified direct download from GitHub Releases | GitHub Actions workflow correctness (the tool trivy doesn't cover) |
| cargo-audit | latest via `taiki-e/install-action` | `taiki-e/install-action` (`tool: cargo-audit`) | Rust security advisories |
| cargo-deny | latest via `taiki-e/install-action` | `taiki-e/install-action` (`tool: cargo-deny`) | Rust dependency/license policy (`deny.toml`); checked via `command -v cargo-deny`, not just `cargo` — cargo subcommands are separate PATH binaries |
| reviewdog | 0.20.3 | `reviewdog/action-setup` | PR check reporting |
| python3 | ≥3.8 | Preinstalled on `ubuntu-latest` | SARIF conversion scripts |
| cargo | pinned | `rust-toolchain.toml` | Rust toolchain (already used by the rest of this repo's CI) |

## Why These Install Methods

- **`taiki-e/install-action`** is already a convention in this repo (`ci.yml` uses it for `just`/`trunk`) — it installs prebuilt binaries, not source builds, and supports `trivy`, `cargo-audit`, `cargo-deny` directly.
- **`reviewdog/action-setup`** is reviewdog's own official installer.
- **semgrep** has no widely-used dedicated setup action; `actions/setup-python` + a version-pinned `pip install` is the pattern Semgrep's own docs recommend for non-SaaS CI.
- **gitleaks and actionlint** are installed via direct, checksum-verified downloads because neither has a trustworthy dedicated installer action: gitleaks's official action turned commercial, and actionlint's third-party "setup" actions on the marketplace are low-visibility, unaudited projects — a pinned-version, checksum-verified download of the tool's own official release is more auditable than depending on either.

Every install is pinned to an exact version (or commit SHA, for actions) — never a floating tag, never `@latest`.

## Known Tool Quirks (found by actually running each one)

These were all discovered by installing and running each tool against this repo directly — not by reading docs — since several looked plausible on paper but failed in practice:

- **Semgrep has no `--offline` flag.** It errors `unknown option '--offline'`. Metrics reporting only fires when `--config` pulls from the Semgrep registry or the user is logged in — since `--config` here always points at the local `.ci/rules/semgrep/` directory, no metrics call happens regardless; `--metrics=off` is set explicitly anyway rather than relying on that default.
- **Semgrep's valid `severity` values** are `ERROR, WARNING, INFO, INVENTORY, EXPERIMENT, CRITICAL, HIGH, MEDIUM, LOW` — not `NOTE`. An earlier draft of `.ci/rules/semgrep/*.yml` used `NOTE` and failed schema validation outright (`InvalidRuleSchemaError`); rules needing an informational level use `INFO`.
- **`trivy config` doesn't accept `--skip-db-update` or `--offline-scan`.** Those are vulnerability-scanning flags (`trivy image`/`trivy fs`); passing them to `trivy config` is a hard `unknown flag` error. The correct flag to skip its checks-bundle refresh is `--skip-check-update` (not used here — on an ephemeral runner with no cache, skipping it would leave zero checks loaded).
- **`gitleaks detect --source=git` is invalid.** `--source`/`-s` takes a *path* (default `.`), not a mode keyword. `gitleaks detect` scans git history by default anyway (unless `--no-git` is passed), so no `--source` flag is needed at all.
- **`cargo deny check --all` is invalid.** `all` is a positional check-selector argument, not a flag: `cargo deny check all`, not `cargo deny check --all`.
- **actionlint's `-format` flag takes a literal Go-template string, not a file path.** Its official SARIF template (vendored at `.ci/rules/actionlint/sarif.tmpl`, copied from `testdata/format/sarif_template.txt` in the actionlint repo) is read into the argument at scan time: `` -format "$(cat .ci/rules/actionlint/sarif.tmpl)" ``.

## Version Pinning Strategy

- **semgrep:** exact version pin (`1.171.0`) via `pip install`
- **gitleaks, actionlint:** exact version + sha256 checksum, verified before use
- **trivy, cargo-audit, cargo-deny:** version resolved by `taiki-e/install-action`'s own pin (update by bumping that action's own pinned SHA)
- **reviewdog:** exact version pin (`v0.20.3`) via `reviewdog_version` input
- **Semgrep rules:** version-controlled in `.ci/rules/semgrep/`
- **actionlint SARIF template:** vendored in `.ci/rules/actionlint/sarif.tmpl`, tied to the pinned actionlint version above
- **Rust toolchain:** pinned in `rust-toolchain.toml`

To update a tool version:

1. Update the version string/SHA in `.github/workflows/quality.yml`
2. If it's a checksum-verified download (gitleaks, actionlint), fetch the new release's checksums file and update the pinned sha256
3. Test the change (a `workflow_dispatch` run is sufficient — no runner provisioning needed)
4. Commit

## Troubleshooting

### A tool-install step fails

The "Verify toolchain" step (right after the install steps in `quality.yml`) checks every tool is actually on `PATH` and **fails the job** if not — unlike the old self-hosted-runner design, a missing tool here always means an install step broke, never "not provisioned yet." Check the specific install step's log for the actual error (a checksum mismatch, a changed download URL, a yanked pip version, etc.).

### Checksum mismatch on gitleaks or actionlint

The pinned sha256 no longer matches the download — this means either the pinned version string and checksum have drifted out of sync (fix: recompute the checksum for the exact pinned version from that release's own `*_checksums.txt` file) or, far less likely, something is wrong with the download itself. Never silently drop the checksum check to work around a mismatch.

### `trivy config` reports "0 config files found"

Expected today — see the trivy row above. This is not a bug; it means this repo has none of terraform/cloudformation/kubernetes/helm/dockerfile/ansible files yet.

## References

- [Semgrep CLI Reference](https://semgrep.dev/docs/cli-reference/)
- [Gitleaks Documentation](https://github.com/gitleaks/gitleaks)
- [Trivy Documentation](https://aquasecurity.github.io/trivy/)
- [cargo audit Documentation](https://docs.rs/cargo-audit/)
- [actionlint Documentation](https://github.com/rhysd/actionlint)
- [taiki-e/install-action](https://github.com/taiki-e/install-action)
