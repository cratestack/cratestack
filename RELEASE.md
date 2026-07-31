# Release Process

CrateStack publishes through the common public Rust and editor channels:

* Rust crates: crates.io and docs.rs
* CLI binaries and release notes: GitHub Releases
* VS Code extension: Visual Studio Marketplace and Open VSX
* Documentation site: Mintlify or equivalent static docs hosting from `docs-site/`

## Quickstart (CI-driven — preferred)

Cutting a release no longer requires a local machine with crates.io/npm
credentials. Run the **"Prepare Release"** GitHub Actions workflow
(`workflow_dispatch`, inputs: `version`, `mode`):

* `mode: dry` — bumps + validates + dry-runs crates.io and npm publishes,
  entirely inside the CI job. No git writes at all (no commit, branch, PR,
  or tag) — safe to run repeatedly against any version to rehearse.
* `mode: real` — bumps versions, commits, and pushes a `release/vX.Y.Z`
  branch, then tries to open a normal PR
  (`chore: bump workspace to vX.Y.Z`) against `main`, with the merged PRs
  since the last release listed as the source-of-truth. **Review and merge
  that PR like any other change** — this is the one human checkpoint in an
  otherwise fully automated pipeline.

  **Standing limitation — the workflow's own PR-open step currently fails.**
  This repo's GitHub org has "Allow GitHub Actions to create and approve
  pull requests" turned off (Settings → Actions → General → Workflow
  permissions), so the final `gh pr create` call in the "Open release PR"
  step fails with `GitHub Actions is not permitted to create or approve
  pull requests`. This is an org-wide policy, not something fixable from
  this repo's own settings or from the workflow YAML — confirmed by a 409
  when attempting to flip the setting via the API. **The bump commit and
  branch push both still succeed** — only PR creation fails — so after a
  `mode: real` dispatch, check the workflow run: if it stopped at "Open
  release PR", a human opens the PR by hand for the already-pushed branch.
  See [Troubleshooting → PR creation fails](#pr-creation-fails-github-actions-is-not-permitted-to-create-or-approve-pull-requests)
  below for the exact commands and PR-body template to use.

Once the bump PR is merged (by whichever route it got opened), everything
else happens on its own:

1. **"Cut Release Tag"** (triggers on every push to `main`) notices the new
   version in `Cargo.toml` and creates + pushes tag `vX.Y.Z` — a no-op on
   every other ordinary commit.
2. That same tag push also triggers **"Release CLI Binaries"**
   (`.github/workflows/release-cli.yml`): publishes every crate to
   crates.io (`CARGO_REGISTRY_TOKEN`), builds and attaches cross-platform
   `cratestack-cli` binaries to a GitHub Release, and publishes both npm
   packages (`@cratestack/cli`, `@cratestack/api`) with provenance
   (`NPM_TOKEN`). See [`docs/tooling/npm-publishing.md`](docs/tooling/npm-publishing.md)
   for one-time secret setup.
3. ...and **"Release VS Code Extension"**
   (`.github/workflows/release-vscode.yml`): builds `cratestack-lsp` per
   platform, then publishes `packages/cratestack-vscode` to the Visual
   Studio Marketplace via Entra ID workload identity (a managed identity,
   no stored PAT — `AZURE_CLIENT_ID`/`AZURE_TENANT_ID`/
   `AZURE_SUBSCRIPTION_ID` on a `vscode-marketplace` GitHub Environment)
   and to Open VSX (`OVSX_PAT`). See
   [`docs/tooling/vscode-publishing.md`](docs/tooling/vscode-publishing.md)
   for one-time publisher/namespace + identity setup.

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

**`release-vscode.yml` is newer and not yet configured**: as of `v0.4.16`
none of the Marketplace Entra ID identity, the `vscode-marketplace`
Environment, or `OVSX_PAT` exist yet, so every tag push soft-skips both
publish jobs (the `build` job still runs and uploads vsix artifacts per
platform, proving the packaging side works). See
[`docs/tooling/vscode-publishing.md`](docs/tooling/vscode-publishing.md) for
the one-time publisher/namespace/identity setup that unblocks this.

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
  and the two npm `package.json`s (`packages/cratestack-cli-npm`,
  `packages/cratestack-api`), and refresh `Cargo.lock`. Idempotent.
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
fallback these recipes wrap. The VS Code extension still ships on its own
cadence — see [Publish Editor Extension](#publish-editor-extension).

## Troubleshooting

Real failures seen while hardening this pipeline (`v0.4.14`–`v0.4.16`), and
what to do about each. Pattern-match the symptom first.

### PR creation fails: "GitHub Actions is not permitted to create or approve pull requests"

**Symptom:** "Prepare Release" (`mode: real`) fails at the "Open release PR"
step with this exact message in the log. The bump commit and
`release/vX.Y.Z` branch push both already succeeded before this step —
only PR creation failed.

**Cause:** an org-level GitHub setting — Settings → Actions → General →
Workflow permissions → "Allow GitHub Actions to create and approve pull
requests" — is off, and (confirmed) attempting to flip it via the API
returns a 409: `"The organization does not allow GitHub Actions to create
or approve pull requests"`. This is **standing and unresolved** — it is an
org-wide policy, not a per-repo setting or something the workflow YAML can
work around. Expect every future `mode: real` dispatch to fail at this
exact step.

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

**Symptom:** `publish-npm` and/or `publish-npm-api` fail with
`npm error code EOTP` / `npm error This operation requires a one-time
password from your authenticator.`

**Cause:** the `NPM_TOKEN` secret is a regular npmjs.com token from an
account with 2FA-on-publish enabled, not an **Automation**-type token.
Only Automation tokens (npmjs.com → Access Tokens → Generate New Token →
Automation) skip the OTP requirement for unattended/CI publishing — this
is npm's design, not something the workflow can retry around. This hit
both npm packages on `v0.4.15`.

**Fix:** generate a new Automation token on npmjs.com and rotate the
`NPM_TOKEN` repo secret to it, then re-run the release (a fresh version
bump — npm publishes can't be retried against an already-attempted
version/tarball). See
[`docs/tooling/npm-publishing.md`](docs/tooling/npm-publishing.md#npm-one-time-setup-needs-cratestack-npm-org-access).
Confirmed fixed on `v0.4.16`.

### Verifying a release actually shipped

Don't trust the CI green checkmark alone — verify each publish target
live:

```sh
# crates.io — note the User-Agent requirement: a bare `curl` with none set
# gets a misleading 403 that looks like the crate/version is missing when
# it isn't.
curl -A "cratestack-release-check" https://crates.io/api/v1/crates/cratestack-core/0.4.16

# npm — both packages
curl https://registry.npmjs.org/@cratestack/cli/0.4.16
curl https://registry.npmjs.org/@cratestack/api/0.4.16

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
`release-cli.yml`'s `publish-npm-api` job) — if this resurfaces, check
that a new `pnpm install` call site added elsewhere hasn't missed the same
env var.

## Prerequisites

Required credentials are intentionally read from the environment:

* `CARGO_REGISTRY_TOKEN` for crates.io
* `AZURE_CLIENT_ID` / `AZURE_TENANT_ID` / `AZURE_SUBSCRIPTION_ID` for the Visual Studio Marketplace
  (Entra ID workload identity federation — a managed identity, no PAT; a local manual publish
  outside CI can still use `vsce login`/a personal PAT instead, see
  [`docs/tooling/vscode-publishing.md`](docs/tooling/vscode-publishing.md))
* `OVSX_PAT` for Open VSX
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

Build and stage the language server first:

```sh
cargo build --release -p cratestack-lsp
cd packages/cratestack-vscode
pnpm run package:vsix
pnpm run publish:vscode-marketplace
pnpm run publish:open-vsx
```

## Tag

```sh
git tag v0.1.0
git push origin v0.1.0
```
