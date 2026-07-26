# VS Code extension publishing setup

`.github/workflows/release-vscode.yml` publishes `packages/cratestack-vscode` to both the
Visual Studio Marketplace and Open VSX on every `vX.Y.Z` tag push, alongside the crates.io/npm/
GitHub-Release publishes in `release-cli.yml` (never on a manual `workflow_dispatch` — neither
publish can be cleanly deleted and retried, so a throwaway test tag must never reach either
registry).

The extension bundles a native `cratestack-lsp` binary per platform (see `server-path.js`), so
the workflow builds `cratestack-lsp` on a native runner per target and publishes a
platform-specific vsix via `vsce package --target <target>` for each of
`darwin-x64`/`darwin-arm64`/`linux-x64`/`linux-arm64`/`win32-x64` — the same way the Marketplace
itself distributes per-platform extensions. Both publish steps soft-skip (log a warning, exit 0,
without failing the rest of the run) when their secret isn't set, so the workflow succeeds even
before every secret below is configured.

## VS Code Marketplace one-time setup

The publisher ID is `cratestack` (`packages/cratestack-vscode/package.json`'s `publisher` field —
this can't be changed after the publisher is created, so it has to match exactly).

1. Create an Azure DevOps Personal Access Token: go to
   [dev.azure.com](https://dev.azure.com), open your user settings → **Personal access tokens** →
   **New Token**. Set **Organization** to "All accessible organizations", then under **Scopes** →
   "Show all scopes" find **Marketplace** and select **Manage**. Copy the token immediately — it's
   only shown once.
2. Create the publisher at
   [marketplace.visualstudio.com/manage/createpublisher](https://marketplace.visualstudio.com/manage/createpublisher),
   signed in with the same Microsoft account used for the PAT. **ID** must be `cratestack`; **Name**
   can be anything (e.g. "CrateStack").
3. In the GitHub repo (`cratestack/cratestack`) → Settings → Secrets and variables → Actions, add a
   repository secret named `VSCE_PAT` with the token from step 1. `vsce`'s `-p`/`--pat` flag
   defaults to reading this exact env var name, which is what `release-vscode.yml` relies on.

Once the secret exists, the next tag push publishes the extension to the Marketplace — no other
change needed.

**Standing limitation to revisit before December 2026:** Microsoft is retiring these
organization-wide "global" PATs on **December 1, 2026** in favor of Microsoft Entra ID workload
identity federation (`vsce publish --azure-credential`, no PAT at all). A PAT created today keeps
working until then, but this workflow's auth will need migrating to Entra ID federation before the
cutover — that's a separate, larger piece of setup (an Azure AD app registration with a federated
credential trusting this repo's GitHub Actions OIDC issuer), not done here.

## Open VSX one-time setup

The namespace is `cratestack`, matching the same `publisher` field.

1. Register an Eclipse account at [accounts.eclipse.org](https://accounts.eclipse.org) if you don't
   have one — the GitHub username on the Eclipse account must exactly match the GitHub account used
   to sign into Open VSX.
2. Sign the Publisher Agreement: log into [open-vsx.org](https://open-vsx.org) via GitHub, go to
   profile settings, choose "Log in with Eclipse" and authorize it, then click "Show Publisher
   Agreement" and "Agree".
3. Generate an access token: open-vsx.org → Settings → **Access Tokens** → **Generate New Token**.
   Copy the value immediately — it's only shown once.
4. Create the namespace once, locally (needs Node/pnpm and the token from step 3):
   ```bash
   npx ovsx create-namespace cratestack -p <token>
   ```
5. In the GitHub repo → Settings → Secrets and variables → Actions, add a repository secret named
   `OVSX_PAT` with that same token.

Once the secret exists and the namespace has been created (step 4 is a one-time local action, not
part of CI), the next tag push publishes the extension to Open VSX — no other change needed.

## Verifying a publish actually shipped

A green `release-vscode.yml` run only proves the CLI exited 0 — for the same reason `RELEASE.md`
independently verifies crates.io/npm rather than trusting the checkmark, confirm directly:

* Marketplace: `https://marketplace.visualstudio.com/items?itemName=cratestack.cratestack-vscode`
  shows the new version.
* Open VSX: `https://open-vsx.org/extension/cratestack/cratestack-vscode` shows the new version.

## Known gap: no extension icon

Neither the Marketplace nor Open VSX require an `icon` in `package.json` to accept a publish, but
both listings look bare without one — worth adding a square PNG (`icon` field pointing at it) in a
follow-up, not a blocker for the first release.
