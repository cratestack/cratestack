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
- **The `@cratestack/cbor` family** (issue #285) — `@cratestack/cbor-web`
  (`publish-npm-cbor-web` job, wasm-bindgen), `@cratestack/cbor` (`publish-npm-cbor` job, the pure-TS
  umbrella), and `@cratestack/cbor-node` (`build-cbor-node` + `publish-npm-cbor-node` jobs), which is
  different from every other package here: it's a real napi-rs native addon, built once per platform
  across a 7-target matrix (the same targets as the `cratestack-cli` binary matrix above, plus the
  two Linux musl/Alpine targets added by cratestack#850) and published as **8 separate npm
  packages** — the main `@cratestack/cbor-node` plus one auto-generated platform-specific
  subpackage per target (see below).
- **`@cratestack/refine`** (`publish-npm-refine` job, `packages/cratestack-refine/`, issue #571) — a
  refine.dev dataProvider over the generated TypeScript REST client. Plain TS shipping its own
  compiled `dist/`, same shape as the API family, but its own job rather than an entry in that job's
  loop: it has no `@cratestack/*` dependency (only a `@refinedev/core` peer), and a missing Trusted
  Publisher entry on the newest name shouldn't abort the loop for the ten packages beside it.

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

For **each** of the 22 packages — this is a one-per-package setting, not shared, and not inherited
by a new package just because a sibling already has one configured:

- `@cratestack/cli`
- `@cratestack/api`, `@cratestack/ts-types`, `@cratestack/link-batch`, `@cratestack/link-logger`,
  `@cratestack/runtime-fetch`, `@cratestack/runtime-axios`, `@cratestack/validator-zod`,
  `@cratestack/validator-yup`, `@cratestack/adapter-tanstack-query`, `@cratestack/adapter-rtk`
- `@cratestack/cbor`, `@cratestack/cbor-web`
- `@cratestack/refine`
- `@cratestack/cbor-node` **plus 7 auto-generated platform subpackages, one per `napi.targets`
  entry** in `packages/cratestack-cbor-node/package.json` (that list and the `build-cbor-node`
  matrix in `release-cli.yml` must stay identical; `just verify-napi-targets` /
  `.ci/napi-targets-check.sh` is the CI gate that compares them, since nothing else notices a
  mismatch until a tag is already pushed) — `napi prepublish` names them
  `<packageName>-<platform>` (scope preserved): `@cratestack/cbor-node-darwin-x64`,
  `@cratestack/cbor-node-darwin-arm64`, `@cratestack/cbor-node-linux-x64-gnu`,
  `@cratestack/cbor-node-linux-arm64-gnu`, `@cratestack/cbor-node-linux-x64-musl`,
  `@cratestack/cbor-node-linux-arm64-musl`, `@cratestack/cbor-node-win32-x64-msvc`. Each is a
  genuinely separate npm package name needing its own Trusted Publisher entry, same as every
  package above — and for a name that has never been published, step 1 below (name
  must already exist to open its Settings page) needs a real bootstrap, not just "reserve the
  name". None of the CI machinery in `release-cli.yml` can do this first publish itself — Trusted
  Publishing categorically cannot bootstrap a brand-new package name (npm requires the name to
  already exist before you can attach a Trusted Publisher to it), so this is unavoidably manual,
  from a maintainer's own machine, using real `npm login` credentials, once per name.

  **Verified procedure.** Rewritten for cratestack#850: the platform subpackages are now published
  **directly, one directory at a time**, never through the root package. Before #850 this section
  told you to run `npm publish` at the package root and let the `prepublishOnly` hook
  (`napi prepublish -t npm`) create and publish the subpackages as a side effect. That hook now
  carries `--skip-optional-publish` — because leaving it in charge of publishing is what let one
  un-bootstrapped name abort the whole release — so a root `npm publish` would publish **nothing**
  for the platform package you are trying to bootstrap.

  What makes the direct path work: `npm/<platform>/` is already a complete, self-contained package
  — its own `package.json` (with `cpu`/`os`/`libc` set, and **no `scripts` key**, so no lifecycle
  hook runs) plus the `.node` binary and a README. Confirmed with `npm pack --dry-run --json`
  inside `npm/linux-x64-musl/`: a 3-entry tarball,
  `@cratestack/cbor-node-linux-x64-musl@0.10.1`, built without consulting `napi.targets` at all.
  That is also why the old "temporarily narrow `napi.targets`" dance is **gone**: it existed only
  to satisfy the all-targets validation in `napi artifacts`/`napi prepublish`, and this path never
  invokes either.

  1. **Get the binary.** You do **not** need a Rust or zig toolchain locally — take the artifact CI
     already built, from a rehearsal run or any tag build:
     ```bash
     gh run download <run-id> -n cbor-node-binary-x86_64-unknown-linux-musl
     ```
     (Artifact names are `cbor-node-binary-<target-triple>`; a rehearsal —
     `gh workflow run release-cli.yml --ref <branch> -f rehearsal=true` — builds every leg and
     publishes nothing, so it is the cheapest way to produce one.) Building locally instead is
     fine where the toolchain is easy: `cd packages/cratestack-cbor-node && pnpm install && pnpm run build:napi`.
  2. **Scaffold the platform directories.** `napi prepublish` does *not* do this for you — it
     hard-fails with `Release package directory does not exist` if the directory is missing:
     ```bash
     cd packages/cratestack-cbor-node
     pnpm exec napi create-npm-dirs
     ```
     This writes `npm/<platform>/package.json` for every entry in `napi.targets`. Scaffolding all
     of them is harmless; you publish only the one you are bootstrapping.
  3. **Drop the binary into its directory**, matching the filename exactly:
     ```bash
     cp cratestack-cbor-node.linux-x64-musl.node npm/linux-x64-musl/
     ```
  4. **Sanity-check the tarball** before touching the registry — this is a pure local pack, no
     network write:
     ```bash
     cd npm/linux-x64-musl && npm pack --dry-run
     ```
     Expect exactly three entries (`package.json`, the `.node`, `README.md`) and the package id
     `@cratestack/cbor-node-linux-x64-musl@<version>`. A missing `.node` here means step 3's
     filename didn't match.
  5. **Publish that directory, and only that directory:**
     ```bash
     npm login          # once
     cd npm/linux-x64-musl && npm publish --access public
     ```
     **No `--provenance` / `npm config set provenance true` here** — confirmed by hand: provenance
     needs npm to detect a supported CI OIDC provider (GitHub/GitLab Actions), and errors with
     `EUSAGE: Automatic provenance generation not supported for provider: null` on a plain local
     `npm publish` — it doesn't silently skip, it aborts the whole publish before anything uploads.
     Provenance is for `publish-npm-cbor-node`'s real CI run only, never this manual bootstrap.

     Repeat steps 3-5 per platform name you are bootstrapping. Never run `npm publish` at the
     package root as part of this procedure: the root package is CI's job, and at an
     already-released version it would just fail with "cannot publish over".
  6. **Add the Trusted Publisher entry for each name you just created** — this is the whole point of
     the bootstrap, and it is *not* automatic. Publishing the name only makes it *possible* to
     attach a publisher; go configure `@cratestack/cbor-node-<platform>` on npmjs.com now, with the
     same org/repo/workflow-filename values as every other package in the list at the top of this
     section. Until you do, the name exists but CI still cannot publish its *next* version, and
     `publish-npm-cbor-node` keeps failing on it — which looks identical to not having bootstrapped
     at all.

  > **✅ DONE — the two musl names were bootstrapped on 2026-09-02 (cratestack#850).**
  > `@cratestack/cbor-node-linux-x64-musl` and `@cratestack/cbor-node-linux-arm64-musl` are live
  > (`npm view @cratestack/cbor-node-linux-x64-musl version` → `0.10.1`, `libc` → `musl`), by
  > exactly the procedure above. Nothing is outstanding; this note is kept because the procedure is
  > the recipe for the **next** target added to `napi.targets`, and because what it worked around
  > is permanent: npm Trusted Publishing categorically cannot create a name (npm/cli#8544), so the
  > first publish of every future platform package is manual too.
  >
  > What #850 also changed, and what makes the *next* one cheap: an un-bootstrapped name no longer
  > takes the release with it. The platform packages are published by an explicit loop that
  > attempts *every* name and only then exits non-zero, and the main package publishes regardless.
  > Before that change, the `prepublishOnly` hook published subpackages sequentially from inside
  > the root `npm publish`, so the first 404 aborted the hook: earlier platform packages live,
  > later ones skipped, main package never published, and the tag's version number burned.
  >
  > **Consumers are never blocked on a bootstrap.** The main package's tarball bundles every
  > `.node` binary (`files: ["*.node"]`, and `napi artifacts` copies each one to the package root
  > as well as into `npm/<platform>/`), and the generated `native.mjs` tries the bundled
  > `./cratestack-cbor-node.<platform>.node` *before* the `@cratestack/cbor-node-<platform>`
  > subpackage — so a platform works from the main package alone even before its subpackage exists.
  >
  > For the next target, no Rust or zig toolchain is required: run a rehearsal
  > (`gh workflow run release-cli.yml --ref <branch> -f rehearsal=true`, which builds every leg and
  > publishes nothing), `gh run download <run-id> -n cbor-node-binary-<target-triple>`, then follow
  > **steps 2-5** of the procedure above, repeating **steps 3-5 per name** — the publish is
  > `cd npm/<platform> && npm publish --access public`, from the platform directory, never the
  > root. Finish with **step 6** for each name: its Trusted Publisher entry, without which CI still
  > cannot publish that name's *next* version.

  **The original 5 platform subpackages have been bootstrapped** and publish from CI on every tag —
  verified against the registry on 2026-08-13, all six `cbor-node*` names at `0.7.15`. The procedure
  above is kept because it is the recipe for the *next* napi target added to `napi.targets`, which
  will need exactly this treatment before its first tag. As of cratestack#850,
  `publish-npm-cbor-node` publishes the platform subpackages in its own bash loop through
  `npm-publish.sh` (the same shape as `publish-npm-api-family`, except this loop attempts every
  name before failing rather than aborting on the first), and publishes the main package whether or
  not that loop succeeded — so an un-bootstrapped platform name now costs a red job rather than the
  whole release. CI can build a new platform's binary (via `workflow_dispatch` against an existing
  tag, no code changes needed) but **cannot** do its first publish, for the same "name must already
  exist" reason. Downloading the CI-built binaries to one machine and repeating steps 3-5 per
  platform — each one a direct `npm publish` from inside `npm/<platform>/` — then step 6 per name,
  closes it out without needing physical access to each OS.

  Note the one place "all targets or nothing" still holds, because it is a *build*-completeness
  gate rather than a publish gate: `napi artifacts` and `napi prepublish` both validate that a
  binary exists for **every** entry in `napi.targets` before touching any of them. That is what
  makes a missing matrix leg fail loudly instead of shipping a release with a platform quietly
  absent. It is also why the bootstrap above never runs either command: publishing
  `npm/<platform>/` directly sidesteps the all-targets validation entirely, which is what replaced
  the old "temporarily narrow `napi.targets`" workaround.

### Bootstrapping a brand-new package name (applies to every package, not just `cbor-node`)

**Trusted Publishing categorically cannot create a package name.** npm requires a name to already
exist before you can attach a Trusted Publisher to it, so the *first* publish of any new package is
unavoidably a manual `npm publish` from a maintainer's machine. The napi-specific dance above is
only extra work `cbor-node` needs on top of this; a plain-TS package is otherwise four steps — and
step 3 is not optional, see below.

```bash
# 1. install
pnpm install --frozen-lockfile

# 2. BUILD. `npm publish` will not do this for you.
pnpm turbo run build --filter='./packages/cratestack-<name>'

# 3. VERIFY THE TARBALL before publishing anything.
cd packages/cratestack-<name>
npm pack --dry-run

# 4. publish (add --otp=<code> if 2FA prompts, or complete the browser flow)
npm publish --access public
```

#### Step 3 is the one that bites: `npm publish` ships an empty package in silence

Every package here declares `"files": ["dist", …]` and has **no `prepublishOnly` build hook**. npm
packs whatever `dist/` happens to be on disk. If you skipped step 2 — fresh clone, cleaned tree,
different worktree — there is no `dist/`, and `npm publish` does not warn, error, or exit non-zero.
It uploads `package.json`, `README.md`, `LICENSE` and calls it a success. The result is a published
version whose own `main`/`types` point at files that aren't in the tarball, so
`import … from "@cratestack/<name>"` fails to resolve for every consumer.

This is not hypothetical: `@cratestack/refine@0.7.14` was bootstrapped this way on 2026-08-13 and
shipped exactly three files.

`npm pack --dry-run` prints the file list and a `total files:` count. **Read it.** The signature of
the failure is a count of 3:

```text
npm notice total files: 3      # BROKEN — dist/ is missing, go back to step 2
npm notice total files: 23     # what a real plain-TS package looks like
```

A published version cannot be replaced. `npm unpublish` on a package's *only* version deletes the
name outright, blocks recreating it for 24 hours, and takes the Trusted Publisher configuration with
it — so the recovery for a bad bootstrap is usually not recovery at all: leave the broken version in
place and let the next tag publish a correct one over it. Thirty seconds reading `npm pack
--dry-run` avoids the whole question.

#### Which version to publish

Publish the version **already committed in `package.json`** — do not hand-bump it to the next
release. Publishing `0.7.15` by hand and then tagging `v0.7.15` makes the CI job hit
`EPUBLISHCONFLICT` on an already-live version, and the npm jobs are a hard failure, not a skip.

#### No provenance, and the 2FA step can't be automated

No `--provenance` and no `npm config set provenance true` on this manual bootstrap: provenance needs
npm to detect a supported CI OIDC provider and aborts the whole publish with `EUSAGE: Automatic
provenance generation not supported for provider: null` on a local run. Provenance is for the CI
jobs only.

If the publishing account has 2FA set to "required for write actions" — this org's does — `npm
publish` exits `EOTP` and prints a browser URL to authenticate. **That step can't be delegated or
scripted**: it needs the maintainer's own terminal and second factor. Steps 1-3 can be prepared by
anyone; only step 4 needs the maintainer.

Then, the per-package Trusted Publisher setup:

1. On npmjs.com, sign in as a member of the `@cratestack` org with publish rights, and open the
   package's Settings page (`npmjs.com/package/@cratestack/<name>/access`) — the package must
   already exist on the registry (the manual bootstrap publish above, or just reserving the name)
   before its Trusted Publisher can be configured.
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
package with no Trusted Publisher configured yet makes its own `npm publish` step in
`publish-npm-api-family` fail, and that **does** block the packages after it: the loop runs
`exit 1` on a failed publish, which aborts the whole script, so later packages in the same job are
never attempted. `publish-npm-cbor-node`
used to have the same problem in a worse form — its single `npm publish` drove `napi prepublish`'s
per-subpackage publishes from inside a `prepublishOnly` hook, so the first missing Trusted Publisher
aborted the hook *and* prevented the main package from publishing at all. Since cratestack#850 the
hook runs with `--skip-optional-publish` and the job publishes each `npm/<platform>` package itself,
in a loop that attempts every name, reports all the failures, publishes the main package regardless,
and only then exits non-zero.

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

## Re-running a partially-published release

Every `npm publish` in `release-cli.yml` goes through `.github/scripts/npm-publish.sh` rather than
being called directly. That wrapper handles the two ways a publish fails for reasons that aren't
"the publish is broken":

- **Already published → skipped as success.** Re-running a release re-executes every publish job
  against the same tag, including ones that already succeeded. Without this, a single
  already-published package fails its job — and in `publish-npm-api-family` it aborts the loop, so
  the packages that genuinely still need publishing never get attempted. This is not hypothetical:
  it is exactly what stranded `v0.7.5`.
- **Sigstore transparency-log 409 → retried with backoff.** npm's internal retry to Rekor can race
  its own already-landed tlog write and get back `409 an equivalent entry already exists in the
  transparency log`; sigstore-js defaults to `fetchOnConflict: false`, so that benign duplicate
  surfaces as a fatal `TLOG_CREATE_ENTRY_ERROR`. A fresh `npm publish` re-signs with a new ephemeral
  cert and clears it. Tracked upstream as
  [sigstore/sigstore-js#1708](https://github.com/sigstore/sigstore-js/issues/1708); once its fix
  ([#1709](https://github.com/sigstore/sigstore-js/pull/1709)) ships in an npm release, the retry
  can be reconsidered.

Any other failure is surfaced immediately and never retried — the wrapper must not mask a real
problem such as bad auth, a missing Trusted Publisher entry, or a broken tarball.

Because publishing is idempotent this way, a half-landed release can be recovered **when the failure
was transient** — a sigstore 409 that exhausted its retries, a network blip — by re-running the
failed jobs against the same tag. Already-published packages are skipped and the missing ones land.

**When the fix is a code or workflow change, re-running does not work, and this is the trap.** Every
publish job checks out `ref: ${{ needs.prepare.outputs.tag }}`, so it gets the repository as it was
*at the tag*, not as it is on `main` — a fix merged to `main` after the tag simply isn't there.
GitHub also replays the workflow YAML from the commit that triggered the run, so even workflow edits
don't apply. And `workflow_dispatch` is not a way around it: that path only rebuilds and re-attaches
binaries, never touching crates.io or npm.

So a release that failed for a reason needing a fix must **bump forward to a new version**. Moving
the existing tag onto the fix is only defensible if the tag's tree would differ purely in CI/docs;
once `main` carries real source changes past the tag — as it did after `v0.7.5` — re-pointing the
tag would make it claim source that crates.io never published under that version.

This is not hypothetical: `v0.7.5` failed on the sigstore 409 and a `wasm-opt` download, and was
abandoned half-published (`@cratestack/cli` and `@cratestack/ts-types` reached 0.7.5; nothing else
did). It was recovered by releasing `v0.7.6`, not by re-running.

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

`.github/workflows/prepare-release.yml`'s "Open release PR" step uses the same secret and the same
reasoning (cratestack#531): the anti-recursion rule applies to *any* GITHUB_TOKEN-raised event, not
just pushes, and a release-bump PR opened via the default token ran zero CI as a direct result —
`v0.7.12` shipped an unedited `CHANGELOG.md` seed because the gate that would have caught it never
ran. That workflow's checkout and `gh pr create` also fall back to `github.token` with a loud
`::warning::` if `RELEASE_PAT` is unset, same pattern as here.

1. On GitHub, create a **personal access token** with `contents: write` **and `pull requests:
   write`** permission on `cratestack/cratestack` (a fine-grained PAT scoped to just this repo is
   preferred over a classic PAT with the broader `repo` scope, but either works). The
   `pull requests: write` scope wasn't needed when this token was first introduced for
   `cut-release-tag.yml`'s tag push alone — it's needed now that `prepare-release.yml` also uses
   this token to open the bump PR itself (cratestack#531). If `RELEASE_PAT` already exists scoped to
   `contents` only, broaden it; `gh pr create` will otherwise fail (or silently fall back to
   `github.token`, right back to the zero-CI problem this exists to fix).
2. Add it as a repo secret named `RELEASE_PAT` (same Settings → Secrets and variables → Actions
   page as `CARGO_REGISTRY_TOKEN`).

Once the secret exists (with both scopes), the next "Prepare Release" bump PR that merges will have
its auto-created tag genuinely trigger `release-cli.yml` — no manual `gh workflow run`/tag
recreation needed. Confirmed working on `v0.4.15` and `v0.4.16`: both releases' `release-cli.yml`
runs show `event: "push"` (not `workflow_dispatch`), i.e. the tag push genuinely cascaded. The
PR-creation half (cratestack#531) has not yet been confirmed on a real `mode: real` dispatch — watch
the next release-bump PR: `gh api repos/cratestack/cratestack/commits/<head-sha>/check-runs --jq
.total_count` should be greater than zero and include the changelog and governance checks, not `0`
like PR #528.

## PR creation itself: no longer a known limitation

Earlier revisions of this doc noted that "Prepare Release" (`mode: real`) couldn't open its own bump
PR at all — the `gh pr create` call failed with `GitHub Actions is not permitted to create or
approve pull requests`, an org-level setting (Settings → Actions → General → Workflow permissions →
"Allow GitHub Actions to create and approve pull requests") that was off and rejected being flipped
via the API (409). As of cratestack#531's investigation, `gh api
repos/cratestack/cratestack/actions/permissions/workflow` reports `can_approve_pull_request_reviews:
true`, and PR #528 was in fact opened by this workflow's own `gh pr create` call — so that specific
failure mode is not currently reproducing. [`RELEASE.md`'s Troubleshooting
section](../../RELEASE.md#pr-creation-fails-github-actions-is-not-permitted-to-create-or-approve-pull-requests)
keeps the recovery procedure in case the org setting regresses, but it is a different, unrelated
problem from cratestack#531 (PR opens fine, runs zero CI) — don't conflate the two if debugging a
future release.

## Provenance

Both publish steps pass `npm publish --provenance`, which attaches a
[Sigstore-signed provenance attestation](https://docs.npmjs.com/generating-provenance-statements)
linking the published tarball back to this exact GitHub Actions run and commit. This needs:

- **A public repository** — provenance publishing is rejected for private repos. Already satisfied.
- **`id-token: write` permission** — set at the job level on `publish-npm`, `publish-npm-api-family`,
  `publish-npm-cbor-node`, `publish-npm-cbor-web`, `publish-npm-cbor`, and `publish-npm-refine` (not
  workflow-wide, since the other jobs in this file don't need it). This is the same permission
  Trusted Publishing's OIDC exchange uses, so both features share one job-level setting.
- **npm >= 9.5.0** — every publish-npm* job pins `node-version: 24` and additionally runs
  `npm install -g npm@^11` before publishing (deliberately pinned to the latest 11.x, not an
  unbounded `npm@latest` — a fresh npm major has previously regressed Trusted Publishing on this
  repo), since Trusted Publishing's own >= 11.5.1 requirement is stricter than provenance's and
  isn't guaranteed by whatever npm version happens to ship bundled with a given Node release.
- `publish-npm-cbor-node` is the one exception to the `--provenance` flag itself: `napi
  prepublish`'s internal per-subpackage `npm publish` calls don't see a flag passed to the *outer*
  command, so that job does `npm config set provenance true` instead — equivalent, but set globally
  so it also covers the subpackage publishes, not just the main package's.

No additional GitHub secret is needed for provenance — it's purely a CI-side capability enabled by
the permission and the flag, on top of whatever auth method (Trusted Publishing, here) gets the
publish itself authorized.

## When a publish is accepted but never becomes visible (v0.11.1)

`npm publish` exiting 0 means the registry *accepted* the tarball ("Your package is being processed
and may take a few minutes to become available"), not that anyone can install it. During npm's
2026-09-03 publish incident two cbor-node subpackages stayed invisible for about an hour after a
green exit (then appeared on their own), and a third was "staged" (`E409 Cannot publish over previously staged version`) after
an earlier attempt had 401'd. `publish-npm-cbor-node` therefore ends with a step that polls
`https://registry.npmjs.org/<name>/<version>` for every package it published and fails naming the
ones that are not visible after six minutes.

If that step fails: check <https://status.npmjs.org/> first. A staged or accepted version may still
appear once the incident resolves — re-check with the same `curl` before doing anything. What you
cannot do is re-run the publish jobs from CI (they are gated on the tag push); the choices are the
manual per-directory publish above for the missing names once the registry is healthy, or a new
version. `.github/scripts/npm-publish.sh`'s header documents how it classifies each failure, and
`just npm-publish-test` runs that classification against the real 0.11.1 output lines.
