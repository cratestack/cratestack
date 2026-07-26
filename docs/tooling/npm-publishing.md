# npm publishing setup

This repo ships two npm packages, both published by `.github/workflows/release-cli.yml`'s
`publish-npm` and `publish-npm-api` jobs on every `vX.Y.Z` tag push (never on a manual
`workflow_dispatch` — an npm publish can't be deleted and retried like a GitHub Release, so a
throwaway test tag must never reach the registry):

- **`@cratestack/cli`** (`packages/cratestack-cli-npm/`) — fetches the prebuilt binary from the
  matching GitHub Release at install time.
- **`@cratestack/api`** (`packages/cratestack-api/`) — hand-written, ships its own compiled
  `dist/` in the tarball.

Both jobs soft-skip (log a warning, exit 0, without failing the release) when the `NPM_TOKEN`
repo secret isn't set — so the rest of the release still succeeds even before this is configured.

## One-time setup (needs `@cratestack` npm org access)

1. On npmjs.com, sign in as a member of the `@cratestack` org with publish rights.
2. Create a new **Automation** access token (Settings → Access Tokens → Generate New Token →
   Granular Access Token or Automation, scoped to `@cratestack/cli` and `@cratestack/api`). An
   Automation token is required here — a token requiring 2FA-on-publish won't work in CI.
3. In the GitHub repo (`cratestack/cratestack`) → Settings → Secrets and variables → Actions, add
   a new repository secret named `NPM_TOKEN` with that token's value.

Once the secret exists, the next tag push publishes both packages — no other change needed.

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
