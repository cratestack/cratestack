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
  across a 5-target matrix (same targets as the `cratestack-cli` binary matrix above) and published
  as **6 separate npm packages** — the main `@cratestack/cbor-node` plus one auto-generated
  platform-specific subpackage per target (see below).

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

For **each** of the 19 packages — this is a one-per-package setting, not shared, and not inherited
by a new package just because a sibling already has one configured:

- `@cratestack/cli`
- `@cratestack/api`, `@cratestack/ts-types`, `@cratestack/link-batch`, `@cratestack/link-logger`,
  `@cratestack/runtime-fetch`, `@cratestack/runtime-axios`, `@cratestack/validator-zod`,
  `@cratestack/validator-yup`, `@cratestack/adapter-tanstack-query`, `@cratestack/adapter-rtk`
- `@cratestack/cbor`, `@cratestack/cbor-web`
- `@cratestack/cbor-node` **plus 5 auto-generated platform subpackages, one per `napi.targets`
  entry** in `packages/cratestack-cbor-node/package.json` — `napi prepublish` names them
  `<packageName>-<platform>` (scope preserved): `@cratestack/cbor-node-darwin-x64`,
  `@cratestack/cbor-node-darwin-arm64`, `@cratestack/cbor-node-linux-x64-gnu`,
  `@cratestack/cbor-node-linux-arm64-gnu`, `@cratestack/cbor-node-win32-x64-msvc`. Each is a
  genuinely separate npm package name needing its own Trusted Publisher entry, same as every
  package above — but since none of these 6 names have ever been published, step 1 below (name
  must already exist to open its Settings page) needs a real bootstrap, not just "reserve the
  name". None of the CI machinery in `release-cli.yml` can do this first publish itself — Trusted
  Publishing categorically cannot bootstrap a brand-new package name (npm requires the name to
  already exist before you can attach a Trusted Publisher to it), so this is unavoidably manual,
  from a maintainer's own machine, using real `npm login` credentials, once per name.

  **Verified procedure** (confirmed by hand, not from `@napi-rs/cli` docs alone — an earlier
  version of this doc assumed `napi prepublish` alone would scaffold everything it needs; it
  doesn't):

  1. Build the native addon for your own host platform:
     `cd packages/cratestack-cbor-node && pnpm install && pnpm run build:napi && pnpm exec tsc -p tsconfig.json`.
  2. **You don't have to build all 5 platforms to publish something real.** To bootstrap with only
     your host platform (e.g. `aarch64-apple-darwin` on Apple Silicon) and let CI publish the rest
     once their binaries are built there, temporarily narrow `napi.targets` in `package.json` down
     to just your platform — **do not commit this**, it's a local-only edit for this one publish:
     ```bash
     node -e '
       const fs = require("fs");
       const p = JSON.parse(fs.readFileSync("package.json"));
       p.napi.targets = ["aarch64-apple-darwin"]; // your platform only
       fs.writeFileSync("package.json", JSON.stringify(p, null, 2) + "\n");
     '
     ```
     `napi artifacts`/`napi prepublish` both validate that a binary exists for **every** target
     currently listed in `napi.targets` before touching any of them (confirmed: no partial runs) —
     narrowing the list to what you actually built is what makes a single-platform bootstrap
     possible instead of a hard failure.
  3. Scaffold the per-platform npm package directories and drop your binary into the matching one
     (`napi create-npm-dirs` does **not** run automatically inside `napi prepublish` — confirmed by
     hand, it hard-fails with `Release package directory does not exist` without this step first):
     ```bash
     pnpm exec napi create-npm-dirs
     cp cratestack-cbor-node.darwin-arm64.node npm/darwin-arm64/   # match your actual binary filename
     ```
  4. `npm login`, then sanity-check before publishing for real:
     `pnpm exec napi prepublish -t npm --dry-run` — should complete with no error and no missing-
     target complaint now that `napi.targets` only lists what you built.
  5. Publish for real — this is `npm publish`'s own `prepublishOnly` hook running `napi prepublish
     -t npm` for you, which creates+publishes the platform subpackage(s) present, then npm packs and
     publishes the main package with `optionalDependencies` scoped to only those:
     ```bash
     npm publish --access public
     ```
     **No `--provenance` / `npm config set provenance true` here** — confirmed by hand: provenance
     needs npm to detect a supported CI OIDC provider (GitHub/GitLab Actions), and errors with
     `EUSAGE: Automatic provenance generation not supported for provider: null` on a plain local
     `npm publish` — it doesn't silently skip, it aborts the whole publish before anything uploads.
     Provenance is for `publish-npm-cbor-node`'s real CI run only, never this manual bootstrap.
  6. **Revert the local `napi.targets` edit** (`git checkout packages/cratestack-cbor-node/package.json`)
     so the committed config keeps declaring all 5 targets — that's what the next real tag push's
     `build-cbor-node` matrix + `publish-npm-cbor-node` job need to build and publish the remaining
     platforms.

  The remaining 4 subpackages still each need this same one-time manual-publish bootstrap before
  Trusted Publishing can cover them — CI can build their binaries (via `workflow_dispatch` against
  an existing tag, no code changes needed), but **cannot** do their first publish, for the same
  "name must already exist" reason. Until all 5 have been through this once, `publish-npm-cbor-node`
  will keep failing on whichever platform subpackages haven't been bootstrapped yet — it's one
  `npm publish` step publishing everything `napi.targets` declares, not a per-subpackage loop that
  tolerates individual failures (unlike `publish-npm-api-family`'s bash loop). Downloading all 5
  `workflow_dispatch`-built binaries to one machine and repeating steps 2-4 per platform (narrowing
  `napi.targets` to each one in turn, or to several at once if you have binaries for them) is the
  fastest way to close this out without needing physical access to each OS.

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
package with no Trusted Publisher configured yet makes its own `npm publish` step in
`publish-npm-api-family` fail, and that **does** block the packages after it: the loop runs
`exit 1` on a failed publish, which aborts the whole script, so later packages in the same job are
never attempted. `publish-npm-cbor-node`
has no such per-package isolation — its single `npm publish` invocation drives `napi prepublish`'s
internal per-subpackage publishes as one step, so the first missing Trusted Publisher (main package
or any of the 5 subpackages) fails that whole step immediately, before later subpackages are
attempted.

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

Because publishing is idempotent this way, the recovery for a half-landed release is simply to fix
the underlying cause and re-run the failed jobs against the same tag. There is no need to bump to a
fresh version just to get a clean run.

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
- **`id-token: write` permission** — set at the job level on `publish-npm`, `publish-npm-api-family`,
  `publish-npm-cbor-node`, `publish-npm-cbor-web`, and `publish-npm-cbor` (not workflow-wide, since
  the other jobs in this file don't need it). This is the same permission Trusted Publishing's OIDC
  exchange uses, so both features share one job-level setting.
- **npm >= 9.5.0** — every publish-npm* job pins `node-version: 24` and additionally runs
  `npm install -g npm@latest` before publishing, since Trusted Publishing's own >= 11.5.1
  requirement is stricter than provenance's and isn't guaranteed by whatever npm version happens to
  ship bundled with a given Node release.
- `publish-npm-cbor-node` is the one exception to the `--provenance` flag itself: `napi
  prepublish`'s internal per-subpackage `npm publish` calls don't see a flag passed to the *outer*
  command, so that job does `npm config set provenance true` instead — equivalent, but set globally
  so it also covers the subpackage publishes, not just the main package's.

No additional GitHub secret is needed for provenance — it's purely a CI-side capability enabled by
the permission and the flag, on top of whatever auth method (Trusted Publishing, here) gets the
publish itself authorized.
