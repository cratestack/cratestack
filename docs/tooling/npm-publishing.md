# npm + crates.io publishing setup

`.github/workflows/release-cli.yml` publishes everything this repo ships on every `vX.Y.Z` tag
push (never on a manual `workflow_dispatch` — none of these publishes can be deleted and retried
like a GitHub Release, so a throwaway test tag must never reach a registry):

- **Every workspace crate** (`publish-crates` job) — topo-sorted `cargo publish` via
  `just release-publish real`, same recipe a human would run locally.
- **`@cratestack/cli`** (`publish-npm` job, `packages/cratestack-cli-npm/`) — fetches the
  prebuilt binary from the matching GitHub Release at install time.
- **`@cratestack/api`** (`publish-npm-api` job, `packages/cratestack-api/`) — hand-written, ships
  its own compiled `dist/` in the tarball.

All three jobs soft-skip (log a warning, exit 0, without failing the rest of the release) when
their respective secret isn't set — so the release still succeeds even before every secret below
is configured. This tag is normally produced by the **"Prepare Release"** →
**"Cut Release Tag"** pipeline described in [`RELEASE.md`](../../RELEASE.md), not pushed by hand.

## npm one-time setup (needs `@cratestack` npm org access)

1. On npmjs.com, sign in as a member of the `@cratestack` org with publish rights.
2. Create a new **Automation** access token (Settings → Access Tokens → Generate New Token →
   Granular Access Token or Automation, scoped to `@cratestack/cli` and `@cratestack/api`). An
   Automation token is required here — a token requiring 2FA-on-publish won't work in CI.
3. In the GitHub repo (`cratestack/cratestack`) → Settings → Secrets and variables → Actions, add
   a new repository secret named `NPM_TOKEN` with that token's value.

Once the secret exists, the next tag push publishes both npm packages — no other change needed.

## crates.io one-time setup

1. On crates.io, sign in as an account with publish rights on every `cratestack-*` crate.
2. Create a new API token (Account Settings → API Tokens → New Token), scoped at minimum to
   `publish-new` and `publish-update`.
3. Add it as a repo secret named `CARGO_REGISTRY_TOKEN` (same Settings → Secrets and variables →
   Actions page as `NPM_TOKEN`) — `cargo publish` reads this env var automatically, no extra
   config needed.

Once the secret exists, the next tag push publishes every workspace crate via `publish-crates`
(idempotent — already-published versions are skipped, so a re-run after a partial failure, e.g. a
transient crates.io index lag, is safe).

## Provenance

Both publish steps pass `npm publish --provenance`, which attaches a
[Sigstore-signed provenance attestation](https://docs.npmjs.com/generating-provenance-statements)
linking the published tarball back to this exact GitHub Actions run and commit. This needs:

- **A public repository** — provenance publishing is rejected for private repos. Already satisfied.
- **`id-token: write` permission** — set at the job level on `publish-npm` and `publish-npm-api`
  (not workflow-wide, since the other jobs in this file don't need it).
- **npm >= 9.5.0** — whatever ships with the pinned `node-version: 20` in `actions/setup-node`
  already satisfies this.

No additional secret or npmjs.com configuration is needed for provenance beyond the `NPM_TOKEN`
above — it's purely a CI-side capability enabled by the permission and the flag.
