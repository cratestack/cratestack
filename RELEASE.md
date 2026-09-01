# Release Process

CrateStack publishes through the common public Rust and editor channels:

* Rust crates: crates.io and docs.rs
* CLI binaries and release notes: GitHub Releases
* VS Code extension: `.vsix` files attached to GitHub Releases. Open VSX publishing is
  configured and fires on the next tag; Marketplace is still pending its Azure credentials — see
  below
* Documentation site: Mintlify or equivalent static docs hosting from `docs-site/`

## Quickstart (CI-driven — preferred)

Cutting a release no longer requires a local machine with crates.io/npm
credentials. Run the **"Prepare Release"** GitHub Actions workflow
(`workflow_dispatch`, inputs: `version`, `mode`):

* `mode: dry` — bumps + validates + dry-runs crates.io and npm publishes,
  entirely inside the CI job. No git writes at all (no commit, branch, PR,
  or tag) — safe to run repeatedly against any version to rehearse.
* `mode: real` — bumps versions, commits, and pushes a `release/vX.Y.Z`
  branch, then opens a normal PR (`chore: bump workspace to vX.Y.Z`)
  against `main`, with the merged PRs since the last release listed as the
  source-of-truth. **Review and merge that PR like any other change** — this
  is the one human checkpoint in an otherwise fully automated pipeline.

  **The "Open release PR" step's `gh pr create` call itself now succeeds**
  (the org's "Allow GitHub Actions to create and approve pull requests"
  setting — Settings → Actions → General → Workflow permissions — that
  used to block it is no longer off; this section used to document that as
  a standing limitation, now stale, see the note below and
  [Troubleshooting → PR creation fails](#pr-creation-fails-github-actions-is-not-permitted-to-create-or-approve-pull-requests)
  for the recovery procedure that's still worth keeping in case that
  org setting regresses).

  **A separate, previously-undiagnosed problem (cratestack#531): the
  resulting PR ran zero CI.** GitHub does not trigger further workflow runs
  for an event (including a PR being opened) raised by the default
  `GITHUB_TOKEN` — the same anti-recursion rule `cut-release-tag.yml` hit
  for its tag push (see that file's header comment). `v0.7.12` shipped an
  unedited `CHANGELOG.md` seed as a direct result: nothing verified the
  bump PR at all, not even the changelog gate that would have caught it.
  Fixed by making "Open release PR" use the same `RELEASE_PAT` secret
  `cut-release-tag.yml` already relies on (falling back to `github.token`,
  with a loud `::warning::` if unset) — a PAT-authored PR-open is an
  ordinary external event as far as GitHub's trigger engine is concerned,
  so `ci.yml`/`governance.yml` fire on it normally, same as any other PR.
  See `RELEASE_PAT`'s setup section in `docs/tooling/npm-publishing.md` —
  it now needs `pull-requests: write` in addition to `contents: write` if
  it's a fine-grained PAT, since it's used for PR creation too, not just
  the tag push.

Once the bump PR is merged (by whichever route it got opened), everything
else happens on its own:

1. **"Cut Release Tag"** (triggers on every push to `main`) notices the new
   version in `Cargo.toml` and creates + pushes tag `vX.Y.Z` — a no-op on
   every other ordinary commit.
2. That same tag push also triggers **"Release CLI Binaries"**
   (`.github/workflows/release-cli.yml`): publishes every crate to
   crates.io (`CARGO_REGISTRY_TOKEN`), builds and attaches cross-platform
   `cratestack-cli` binaries to a GitHub Release, and publishes every npm
   package — 20 in total: `@cratestack/cli`; the 10-package `@cratestack/api`
   family (`api`, `ts-types`, `link-batch`, `link-logger`,
   `runtime-fetch`, `runtime-axios`, `validator-zod`, `validator-yup`,
   `adapter-tanstack-query`, `adapter-rtk`); the `@cratestack/cbor` family
   (`cbor`, `cbor-web`, `cbor-node` plus its 5 auto-generated platform
   subpackages); and `@cratestack/refine` — with provenance via npm's
   OIDC Trusted Publishing (no token at all). See
   [`docs/tooling/npm-publishing.md`](docs/tooling/npm-publishing.md) for
   the authoritative inventory and one-time setup.
3. ...and **"Release VS Code Extension"**
   (`.github/workflows/release-vscode.yml`): builds `cratestack-lsp` per
   platform, packages a `.vsix` per platform, and attaches all five to the
   same GitHub Release `release-cli.yml` creates for the tag — this is the
   **primary distribution path** today. Marketplace (Entra ID workload
   identity — `AZURE_CLIENT_ID`/`AZURE_TENANT_ID`/`AZURE_SUBSCRIPTION_ID` on
   a `vscode-marketplace` GitHub Environment) and Open VSX (`OVSX_PAT`)
   publish jobs also exist in this workflow. **Open VSX is now configured**
   (`OVSX_PAT` set, `cratestack` namespace created) and publishes for real on
   the next tag — nothing has gone through it yet, so verify the first one.
   **Marketplace is now configured too** (publisher, managed identity,
   federated credential, `vscode-marketplace` environment, `AZURE_*` secrets),
   so publish-marketplace no longer soft-skips — an incomplete setup now fails
   the release run instead of passing quietly. Note Open VSX reaches Cursor/Windsurf/VSCodium but
   **not** VS Code, which can only see the Microsoft Marketplace. See
   [`docs/tooling/vscode-publishing.md`](docs/tooling/vscode-publishing.md)
   for the manual-install instructions users need today, plus the one-time
   publisher/namespace + identity setup for re-enabling either registry
   later.

Step 1 only reliably cascades into step 2 because `cut-release-tag.yml`
pushes the tag using a `RELEASE_PAT` repo secret instead of the default
`GITHUB_TOKEN` — see [Why `RELEASE_PAT` exists](#why-release_pat-exists-and-why-the-bump-goes-through-a-pr-at-all)
below for the mechanism.

`release-cli.yml` also accepts a manual `workflow_dispatch` against an
**existing** tag, for re-running a failed binary build — it validates the
tag exists first and fails with a clear message otherwise; it never touches
crates.io/npm on that path (see the jobs' own comments for why an npm/crates
publish can't safely be retried against a throwaway dispatch).

**Known-good reference: `v0.4.16`** is the first release to go through the
crates.io/npm/GitHub-Release side of this pipeline end-to-end with no manual
publish steps — independently verified live against each registry, not just
a green CI checkmark. See
[Verifying a release actually shipped](#verifying-a-release-actually-shipped)
below for the exact commands. `v0.4.14` and `v0.4.15` each hit one of the two
standing/historical issues described below; both are now understood and
either documented as permanent workarounds (PR creation) or fixed and
re-verified (the `RELEASE_PAT` trigger issue, and the `NPM_TOKEN` token-type
issue).

**npm publishing switched to Trusted Publishing after `v0.4.16`**: `publish-npm` and
`publish-npm-api` no longer read `NPM_TOKEN` at all — they authenticate via npm's OIDC Trusted
Publishing instead, which needs no GitHub secret but does need a one-time Trusted Publisher
configured per package on npmjs.com (see
[`docs/tooling/npm-publishing.md`](docs/tooling/npm-publishing.md)). Unlike the old PAT, there's no
secret to check for absence, so **until that's configured, both jobs fail** rather than
soft-skipping — a deliberate hard cutover, not yet re-verified end-to-end against a real tag push.

**`release-vscode.yml` is newer, and its Open VSX job has never actually run a publish.** The
`build` job (per-platform `.vsix` packaging) and the GitHub-Release attach step are the real,
currently-shipping path and have run clean. `OVSX_PAT` and the `cratestack` Open VSX namespace now
exist, so `publish-openvsx` will attempt a genuine publish on the next tag — treat that first run as
unproven and check the registry, not the checkmark. The Marketplace Entra ID identity, the `vscode-marketplace`
Environment and the `AZURE_*` secrets now all exist as well, so `publish-marketplace` is armed on
the same terms — also never exercised, and now able to turn a release red where it previously
exited 0. See [`docs/tooling/vscode-publishing.md`](docs/tooling/vscode-publishing.md).

### Why `RELEASE_PAT` exists, and why the bump goes through a PR at all

Two separate constraints shape this pipeline:

* **`main` requires review.** Branch protection on `main` needs 1 approving
  + code-owner review, and the workflow's `GITHUB_TOKEN` is not a repo
  admin — it cannot push a bump commit straight to `main` the way a human
  admin's local `just release VERSION PUSH=1` could. So `mode: real` opens
  a normal PR instead and goes through the same review as any other change.
* **`GITHUB_TOKEN`-authored pushes don't fire other workflows' triggers.**
  This is a deliberate GitHub anti-recursion protection, not a bug: a push
  (including a tag push) made with the default `GITHUB_TOKEN` will not
  trigger another workflow's `on: push`. `cut-release-tag.yml` pushes the
  `vX.Y.Z` tag that `release-cli.yml` listens for, so if it used the
  default token, the tag would be created correctly but `release-cli.yml`
  would simply never run — silently, with no error anywhere. This is
  exactly what happened on `v0.4.14`. The fix is the `RELEASE_PAT` repo
  secret (a personal access token, `contents: write` scope): a PAT-authored
  push is an ordinary external push as far as GitHub's trigger engine is
  concerned, so it fires `release-cli.yml` normally. See
  [`docs/tooling/npm-publishing.md`](docs/tooling/npm-publishing.md) for
  setup, and `cut-release-tag.yml`'s own header comment for the full
  mechanism. Confirmed fixed on `v0.4.15` and `v0.4.16`, both of which show
  `release-cli.yml` triggered with `event: "push"`, not `workflow_dispatch`.

## Quickstart (local fallback)

The CI-driven path above wraps the same underlying `just` recipes, which
remain fully usable directly if you'd rather run a release from a local
machine with your own crates.io/npm credentials:

```sh
just release 0.3.4              # publishes for real, tags locally
PUSH=1 just release 0.3.4       # additionally pushes commit + tag to origin
just release 0.3.4 dry          # rehearsal: dry-run publishes, no tag
```

Underlying recipes you can also run individually:

* `just bump 0.3.4` — rewrite `0.x.y` → `0.3.4` across every `Cargo.toml`
  and every npm `package.json` (`packages/cratestack-cli-npm` and the
  10-package `@cratestack/api` family), and refresh `Cargo.lock`.
  Idempotent.
* `just release-check` — workspace check + workspace tests (skips
  `embedded_flutter_native`).
* `just bundle-studio-ui` — refresh `embedded-ui.tar.gz` and
  `embedded-ui-dist.tar.gz` (requires `cargo install --locked trunk` +
  `rustup target add wasm32-unknown-unknown`).
* `just release-publish [real|dry]` — publish every workspace crate in
  dependency order, with one retry-after-30s when the crates.io index
  hasn't caught up to a freshly-published dependency.
* `just publish-studio` — single-crate publish for `cratestack-studio`
  with the studio's tarball-dirty allowance.

The Rust-crate flow described in the rest of this document is the manual
fallback these recipes wrap. The VS Code extension's `.vsix` files ship on
the same tag push via `release-vscode.yml` — see
[Publish Editor Extension](#publish-editor-extension).

## Troubleshooting

Real failures seen while hardening this pipeline (`v0.4.14`–`v0.4.16`), and
what to do about each. Pattern-match the symptom first.

### PR creation fails: "GitHub Actions is not permitted to create or approve pull requests"

**Status as of cratestack#531's investigation: not currently reproducing.**
`gh api repos/cratestack/cratestack/actions/permissions/workflow` reports
`can_approve_pull_request_reviews: true`, and PR #528 (the `v0.7.12` bump
PR) was opened successfully by this workflow's own `gh pr create` call,
authored as `github-actions[bot]`. This section previously said this failure
mode was "standing and unresolved" — that was accurate when written but is
no longer the observed behavior; the org setting below was evidently
switched on since. Left in place (not deleted) as a real recovery procedure
in case it regresses — pattern-match the symptom below first.

**Symptom:** "Prepare Release" (`mode: real`) fails at the "Open release PR"
step with this exact message in the log. The bump commit and
`release/vX.Y.Z` branch push both already succeeded before this step —
only PR creation failed.

**Cause (when this does occur):** an org-level GitHub setting — Settings →
Actions → General → Workflow permissions → "Allow GitHub Actions to create
and approve pull requests" — is off. Confirmed previously to also reject
being flipped via the API (409: `"The organization does not allow GitHub
Actions to create or approve pull requests"`) — an org-wide policy, not a
per-repo setting or something the workflow YAML can work around.

**Do not confuse this with cratestack#531** (a release-bump PR that opens
fine but runs zero CI) — that's a different failure mode, caused by the
GITHUB_TOKEN anti-recursion rule, not this org setting. See the Quickstart
section above and `prepare-release.yml`'s header comment for that one.

**Fix (manual, every time):** a human (or an agent with a real
authenticated `gh`/browser session — the Actions-internal `GITHUB_TOKEN`
cannot do this) opens the PR for the branch the workflow already pushed:

```sh
# 1. The branch is already on origin — the workflow pushed it before failing.
git fetch origin release/vX.Y.Z

# 2. Reconstruct the source-of-truth PR list the workflow would have used
#    (same logic as prepare-release.yml's "Open release PR" step):
last_tag=$(git tag --list 'v*' | sort -V | tail -1)
git log "${last_tag}"..origin/release/vX.Y.Z~1 --pretty=%s \
  | grep -oE '#[0-9]+' | sort -t'#' -k2 -n -u

# 3. Write a PR body following this repo's governance template
#    (.github/PULL_REQUEST_TEMPLATE.md — Summary, Intent, Scope,
#    Verification, Screenshots/Evidence, Risk Assessment, AI Usage
#    Declaration, Reviewer Focus). Reuse the exact wording from
#    prepare-release.yml's "Open release PR" step (the heredoc building
#    /tmp/pr-body.md) — it already has all 8 sections, just substitute the
#    PR list from step 2 for the Source-of-truth bullets and the version
#    for vX.Y.Z. Save it as, e.g., /tmp/pr-body.md.

# 4. Open the PR against the pushed branch:
gh pr create --title "chore: bump workspace to vX.Y.Z" \
  --base main --head "release/vX.Y.Z" \
  --body-file /tmp/pr-body.md
```

Merge that PR like any other — everything downstream (tag, crates.io,
binaries, npm) still triggers automatically once it merges.

### `release-cli.yml` never runs after a "Cut Release Tag" push

**Symptom:** "Cut Release Tag" runs on the merge of a bump PR, logs
`created and pushed vX.Y.Z`, the tag genuinely exists on GitHub — but no
"Release CLI Binaries" run ever appears, and nothing gets published.
Silent: no error anywhere.

**Cause:** GitHub does not fire other workflows' triggers for a push
authored by the default `GITHUB_TOKEN` (anti-recursion protection).
`cut-release-tag.yml` must push using the `RELEASE_PAT` repo secret
instead. If that secret is missing, the job still succeeds (tag pushed)
but logs a `::warning::` saying exactly this — check the job's step
summary for that warning first.

**Fix:** add the `RELEASE_PAT` secret per
[`docs/tooling/npm-publishing.md`](docs/tooling/npm-publishing.md#release-tag-one-time-setup-release_pat).
If a tag was already pushed without triggering anything (as happened on
`v0.4.14`), the fastest recovery for that one tag is a manual
`gh workflow run "Release CLI Binaries" -f tag=vX.Y.Z` — but note this
dispatch path never publishes crates.io/npm (both jobs gate on
`if: github.event_name == 'push'`), only binaries. Confirmed fixed and
re-verified on `v0.4.15` and `v0.4.16` (`gh run view` on the resulting
`release-cli.yml` run shows `event: "push"`, not `workflow_dispatch`).

### npm publish fails with `EOTP` / "This operation requires a one-time password"

**Historical — no longer applicable.** Both npm jobs stopped reading `NPM_TOKEN` entirely in favor
of OIDC Trusted Publishing (see [Prerequisites](#prerequisites) and
[`docs/tooling/npm-publishing.md`](docs/tooling/npm-publishing.md)), which has no token to be the
wrong type. Kept below for anyone tracing back through old `v0.4.15` run logs.

**Symptom:** `publish-npm` and/or `publish-npm-api` fail with
`npm error code EOTP` / `npm error This operation requires a one-time
password from your authenticator.`

**Cause:** the `NPM_TOKEN` secret is a regular npmjs.com token from an
account with 2FA-on-publish enabled, not an **Automation**-type token.
Only Automation tokens (npmjs.com → Access Tokens → Generate New Token →
Automation) skip the OTP requirement for unattended/CI publishing — this
is npm's design, not something the workflow can retry around. This hit
both npm packages on `v0.4.15`, fixed by rotating to an Automation token for `v0.4.16`.

### Verifying a release actually shipped

Don't trust the CI green checkmark alone — verify each publish target
live:

```sh
# crates.io — note the User-Agent requirement: a bare `curl` with none set
# gets a misleading 403 that looks like the crate/version is missing when
# it isn't.
curl -A "cratestack-release-check" https://crates.io/api/v1/crates/cratestack-core/0.4.16

# npm — @cratestack/cli plus the @cratestack/api family (repeat per package)
curl https://registry.npmjs.org/@cratestack/cli/0.4.16
curl https://registry.npmjs.org/@cratestack/api/0.4.16
curl https://registry.npmjs.org/@cratestack/ts-types/0.4.16

# GitHub Release — expect 5 platform binaries + 5 matching .sha256 files
gh release view v0.4.16
```

`v0.4.16` is the known-good reference where all four commands above were
run and confirmed the release had genuinely and fully shipped.

### `ci.yml`'s `js` job fails on a version-bump PR

**Symptom:** the `js` job (runs on every PR, including release-bump PRs)
fails during `pnpm install` trying to download a `cratestack-cli` binary
for a version that has no GitHub Release yet (404).

**Cause:** `cratestack-cli-npm`'s postinstall script fetches a prebuilt
binary matching its own `package.json` version — which `just bump` just
wrote to the new, not-yet-released version. This is already fixed by
setting `CRATESTACK_CLI_SKIP_DOWNLOAD=1` on the relevant `pnpm install`
steps (`ci.yml`'s `js` job, `prepare-release.yml`'s dry-run rehearsal, and
`release-cli.yml`'s `publish-npm-api-family` job) — if this resurfaces, check
that a new `pnpm install` call site added elsewhere hasn't missed the same
env var.

## Prerequisites

Required credentials are intentionally read from the environment:

* `CARGO_REGISTRY_TOKEN` for crates.io
* npm's OIDC Trusted Publishing for `@cratestack/cli` and every package in the `@cratestack/api`
  family — no GitHub secret at all, a per-package Trusted Publisher configured on npmjs.com
  instead (see [`docs/tooling/npm-publishing.md`](docs/tooling/npm-publishing.md))
* No credential is required for the VS Code extension's primary path — its `.vsix` files attach to
  the GitHub Release like the CLI binaries do. `AZURE_CLIENT_ID` / `AZURE_TENANT_ID` /
  `AZURE_SUBSCRIPTION_ID` (Entra ID workload identity federation) and `OVSX_PAT` are all
  configured, so both publish jobs are live — see
  [`docs/tooling/vscode-publishing.md`](docs/tooling/vscode-publishing.md)
* GitHub permissions to push tags and create releases

See [`docs/tooling/npm-publishing.md`](docs/tooling/npm-publishing.md) and
[`docs/tooling/vscode-publishing.md`](docs/tooling/vscode-publishing.md) for one-time setup of
each of these.

## Validate

Run from the repository root:

```sh
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
cargo package -p cratestack-core --allow-dirty --no-verify
```

On the first public release, sibling crates that depend on
`cratestack-core` and each other cannot all complete `cargo package`
against crates.io until their dependencies have been published. After
the first ordered publish has populated crates.io, run package
dry-runs across the full workspace via the automation:

```sh
just release-publish dry
```

The recipe topo-sorts the workspace from `cargo metadata`, so the
order can't drift.

Run from `packages/cratestack-vscode`:

```sh
pnpm install
pnpm run test:smoke
pnpm run package:vsix
```

## Publish Rust Crates

Preferred path: `just release-publish` (or `just release VERSION`) walks
the workspace in topo-sorted order computed from `cargo metadata`. The
recipe is idempotent — already-uploaded versions are skipped — so
restarting after a partial failure is safe.

Manual fallback: print the same order with

```sh
cargo metadata --format-version=1 --no-deps | python3 -c '
import json, sys, copy
m = json.load(sys.stdin)
pkgs = {p["name"]: p for p in m["packages"]
        if p["name"].startswith("cratestack") and p.get("publish") != []}
graph = {n: {d["name"] for d in p["dependencies"]
             if d["name"] in pkgs and d["name"] != n}
         for n, p in pkgs.items()}
order, remaining = [], copy.deepcopy(graph)
while remaining:
    leaves = sorted(n for n, d in remaining.items() if not d)
    if not leaves: sys.exit(f"cycle: {remaining}")
    for n in leaves: order.append(n); del remaining[n]
    for d in remaining.values(): d.difference_update(leaves)
print("\n".join(f"cargo publish -p {n}" for n in order))'
```

and run the resulting `cargo publish` commands top-to-bottom. If
crates.io index propagation causes a dependency lookup race, wait
briefly and retry the next crate. Do *not* maintain a parallel
hand-written list in this file — past releases have stalled when the
list drifted out of sync with the actual workspace dep graph.

## Publish Editor Extension

`release-vscode.yml` handles this on every `vX.Y.Z` tag push: it builds `cratestack-lsp` per
platform, packages a `.vsix` per platform, and attaches all five to the tag's GitHub Release — no
manual step needed for the primary path. Users install with
`code --install-extension <file>.vsix` (see
[`docs/tooling/vscode-publishing.md`](docs/tooling/vscode-publishing.md#manual-install-for-users)).

Both registry jobs publish automatically on the next tag (see [Prerequisites](#prerequisites)
above); neither has been exercised yet. To publish manually from a local machine with your own
credentials:

```sh
cargo build --release -p cratestack-lsp
cd packages/cratestack-vscode
pnpm run package:vsix
pnpm run publish:vscode-marketplace   # needs a personal Marketplace PAT via `vsce login`
pnpm run publish:open-vsx             # needs OVSX_PAT
```

## Tag

```sh
git tag v0.1.0
git push origin v0.1.0
```
