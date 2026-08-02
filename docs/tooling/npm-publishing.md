# npm + crates.io publishing setup

`.github/workflows/release-cli.yml` publishes everything this repo ships on every `vX.Y.Z` tag
push (never on a manual `workflow_dispatch` — none of these publishes can be deleted and retried
like a GitHub Release, so a throwaway test tag must never reach a registry):

- **Every workspace crate** (`publish-crates` job) — topo-sorted `cargo publish` via
  `just release-publish real`, same recipe a human would run locally.
- **`@cratestack/cli`** (`publish-npm` job, `packages/cratestack-cli-npm/`) — fetches the
  prebuilt binary from the matching GitHub Release at install time.
- **The `@cratestack/api` family** (`publish-npm-api-family` job) — 10 hand-written packages, each
  shipping its own compiled `dist/` in the tarball: `@cratestack/api` (`packages/cratestack-api/`,
  a compat re-export shim) plus the packages it was split into —
  `@cratestack/ts-types`, `@cratestack/link-batch`, `@cratestack/link-logger`,
  `@cratestack/runtime-fetch`, `@cratestack/runtime-axios`, `@cratestack/validator-zod`,
  `@cratestack/validator-yup`, `@cratestack/adapter-tanstack-query`, `@cratestack/adapter-rtk`.

`publish-crates` soft-skips (logs a warning, exits 0, without failing the rest of the release)
when `CARGO_REGISTRY_TOKEN` isn't set. The npm jobs are a **hard cutover, not a soft-skip**: they
authenticate via npm's OIDC Trusted Publishing (see below), which has no repo secret to check for
absence — until the Trusted Publisher is configured on npmjs.com for each package, these jobs just
fail rather than quietly no-op'ing. This tag is normally produced by the **"Prepare Release"** →
**"Cut Release Tag"** pipeline described in [`RELEASE.md`](../../RELEASE.md), not pushed by hand.

## npm one-time setup: Trusted Publishing (needs `@cratestack` npm org access)

Every npm package authenticates with npm's [Trusted Publishing](https://docs.npmjs.com/trusted-publishers)
(OIDC, GA since npm CLI 11.5.1 / Node 22.14+) instead of a long-lived token — no `NPM_TOKEN` at
all, no rotation, no EOTP failure mode (see the superseded section below for what that used to
look like).

For **each** of the 11 packages — this is a one-per-package setting, not shared, and not inherited
by a new package just because a sibling already has one configured:

- `@cratestack/cli`
- `@cratestack/api`, `@cratestack/ts-types`, `@cratestack/link-batch`, `@cratestack/link-logger`,
  `@cratestack/runtime-fetch`, `@cratestack/runtime-axios`, `@cratestack/validator-zod`,
  `@cratestack/validator-yup`, `@cratestack/adapter-tanstack-query`, `@cratestack/adapter-rtk`

1. On npmjs.com, sign in as a member of the `@cratestack` org with publish rights, and open the
   package's Settings page (`npmjs.com/package/@cratestack/<name>/access`) — the package must
   already exist on the registry (an initial manual `npm publish` from a maintainer's machine, or
   just reserving the name) before its Trusted Publisher can be configured.
2. Find the **Trusted Publisher** section and add a GitHub Actions trusted publisher with:
   * **Organization or user:** `cratestack`
   * **Repository:** `cratestack`
   * **Workflow filename:** `release-cli.yml` (filename only — this matches every publish job in
     the file, since npm's trust check is scoped to org/repo/workflow file, not to the individual
     job or git ref within it)
   * **Environment:** leave blank (not used here)
   * **Allowed actions:** `npm publish`

Once a package has a Trusted Publisher configured, the next tag push publishes it — no GitHub
secret to add at all. Configuration changes take effect immediately for the *next* publish. A
package with no Trusted Publisher configured yet just makes its own `npm publish` step in
`publish-npm-api-family` fail — it does not block the other packages in the same job, since each
runs as a separate loop iteration, but the job as a whole still exits non-zero.

**Superseded: the old `NPM_TOKEN` PAT setup.** Before this, the npm jobs read an npmjs.com
Automation-type access token from an `NPM_TOKEN` repo secret — CI no longer reads this secret (the
env var was removed from every job), so it can be deleted once Trusted Publishing is confirmed
working, or just left inert. Kept here for history: `NPM_TOKEN` had to specifically be an
**Automation**-type token, since a regular token from a 2FA-on-publish account fails with `npm
error code EOTP` / `npm error This operation requires a one-time password from your
authenticator.` — npm has no way to satisfy an OTP challenge from unattended CI. Confirmed the hard
way on `v0.4.15` (both npm publishes failed with `EOTP`); rotating to an Automation token fixed it
for `v0.4.16`. Trusted Publishing sidesteps this whole class of failure — there's no stored
credential to be the wrong type.

## crates.io one-time setup

1. On crates.io, sign in as an account with publish rights on every `cratestack-*` crate.
2. Create a new API token (Account Settings → API Tokens → New Token), scoped at minimum to
   `publish-new` and `publish-update`.
3. Add it as a repo secret named `CARGO_REGISTRY_TOKEN` (repo Settings → Secrets and variables →
   Actions) — `cargo publish` reads this env var automatically, no extra config needed.

Once the secret exists, the next tag push publishes every workspace crate via `publish-crates`
(idempotent — already-published versions are skipped, so a re-run after a partial failure, e.g. a
transient crates.io index lag, is safe).

## Release-tag one-time setup (`RELEASE_PAT`)

`.github/workflows/cut-release-tag.yml` creates and pushes the `vX.Y.Z` tag once a "Prepare
Release" bump PR merges — but GitHub does not fire other workflows' triggers for a push made with
the default `GITHUB_TOKEN` (anti-recursion protection), so without this secret the tag gets created
correctly but **`release-cli.yml` never runs and nothing actually gets published** — confirmed the
hard way on `v0.4.14`'s first real release through this pipeline. `cut-release-tag.yml` logs a
loud `::warning::` when this secret is missing, precisely so that failure mode isn't silent again.

1. On GitHub, create a **personal access token** with `contents: write` permission on
   `cratestack/cratestack` (a fine-grained PAT scoped to just this repo is preferred over a classic
   PAT with the broader `repo` scope, but either works).
2. Add it as a repo secret named `RELEASE_PAT` (same Settings → Secrets and variables → Actions
   page as `CARGO_REGISTRY_TOKEN`).

Once the secret exists, the next "Prepare Release" bump PR that merges will have its auto-created
tag genuinely trigger `release-cli.yml` — no manual `gh workflow run`/tag recreation needed.
Confirmed working on `v0.4.15` and `v0.4.16`: both releases' `release-cli.yml` runs show
`event: "push"` (not `workflow_dispatch`), i.e. the tag push genuinely cascaded.

## Known limitation: this repo cannot fully self-serve PR creation

Separately from the three secrets above, "Prepare Release" (`mode: real`) itself cannot currently
open its own bump PR — the `gh pr create` call in its "Open release PR" step fails with `GitHub
Actions is not permitted to create or approve pull requests`. This is an org-level GitHub setting
(Settings → Actions → General → Workflow permissions → "Allow GitHub Actions to create and approve
pull requests" is off), confirmed to also reject being flipped via the API (409: "The organization
does not allow GitHub Actions to create or approve pull requests"). No repo secret fixes this — it
is a standing, unresolved limitation with a manual workaround (the bump commit and branch push
still succeed; a human opens the PR by hand for the pushed branch). See
[`RELEASE.md`'s Troubleshooting section](../../RELEASE.md#pr-creation-fails-github-actions-is-not-permitted-to-create-or-approve-pull-requests)
for the exact recovery commands.

## Provenance

Both publish steps pass `npm publish --provenance`, which attaches a
[Sigstore-signed provenance attestation](https://docs.npmjs.com/generating-provenance-statements)
linking the published tarball back to this exact GitHub Actions run and commit. This needs:

- **A public repository** — provenance publishing is rejected for private repos. Already satisfied.
- **`id-token: write` permission** — set at the job level on `publish-npm` and
  `publish-npm-api-family` (not workflow-wide, since the other jobs in this file don't need it).
  This is the same permission Trusted Publishing's OIDC exchange uses, so both features share one
  job-level setting.
- **npm >= 9.5.0** — both jobs pin `node-version: 24` and additionally run `npm install -g
  npm@latest` before publishing, since Trusted Publishing's own >= 11.5.1 requirement is stricter
  than provenance's and isn't guaranteed by whatever npm version happens to ship bundled with a
  given Node release.

No additional GitHub secret is needed for provenance — it's purely a CI-side capability enabled by
the permission and the flag, on top of whatever auth method (Trusted Publishing, here) gets the
publish itself authorized.
