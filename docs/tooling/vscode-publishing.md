# VS Code extension publishing setup

`.github/workflows/release-vscode.yml` publishes `packages/cratestack-vscode` to both the
Visual Studio Marketplace and Open VSX on every `vX.Y.Z` tag push, alongside the crates.io/npm/
GitHub-Release publishes in `release-cli.yml` (never on a manual `workflow_dispatch` — neither
publish can be cleanly deleted and retried, so a throwaway test tag must never reach either
registry).

The extension bundles a native `cratestack-lsp` binary per platform (see `server-path.js`), so the
`build` job builds `cratestack-lsp` on a native runner per target and packages a platform-specific
vsix via `vsce package --target <target>` for each of
`darwin-x64`/`darwin-arm64`/`linux-x64`/`linux-arm64`/`win32-x64` — the same way the Marketplace
itself distributes per-platform extensions. `publish-marketplace` and `publish-openvsx` each run
once (on `ubuntu-latest`, after `build` finishes) against every vsix `build` produced — publishing a
pre-built vsix doesn't need the native OS, so there's no reason to repeat the publish step per
platform. Both publish jobs soft-skip (log a warning, exit 0, without failing the rest of the run)
when their credentials aren't set, so the workflow succeeds even before everything below is
configured.

## VS Code Marketplace one-time setup

The publisher ID is `cratestack` (`packages/cratestack-vscode/package.json`'s `publisher` field —
this can't be changed after the publisher is created, so it has to match exactly).

This uses **Microsoft Entra ID workload identity federation** (a user-assigned managed identity,
no client secret and no PAT) rather than a Personal Access Token — Microsoft is retiring
organization-wide "global" Marketplace PATs on **December 1, 2026**, and this sidesteps that
entirely by never having one to retire. It needs an Azure subscription; if you don't already have
one, [azure.microsoft.com/free](https://azure.microsoft.com/free) covers this (a managed identity
itself is free — you're not standing up billable compute).

Two authentication approaches exist for this: `vsce publish --oidc` ("trusted publishing," no Azure
resources at all — just a policy on the Marketplace publisher page) sounds simpler, but as of this
writing there's no confirmed report of it actually working against the live Marketplace and an open
[microsoft/vsmarketplace#1422](https://github.com/microsoft/vsmarketplace/issues/1422) tracking the
request — it may not be reliably live yet. `--azure-credential` (used below) is the version with
current, confirmed-working real-world setups.

1. **Create a user-assigned managed identity** (needs the Azure CLI and an existing resource
   group — reuse one or `az group create --name cratestack-release --location <region>`):
   ```bash
   az identity create --name cratestack-vsce-publish --resource-group cratestack-release
   az identity show --name cratestack-vsce-publish --resource-group cratestack-release \
     --query "{clientId:clientId, principalId:principalId, id:id}" -o json
   ```
   Record `clientId` and `id` (the full ARM resource ID) — both are needed below. Also record your
   tenant ID (`az account show --query tenantId -o tsv`) and subscription ID (`az account show
   --query id -o tsv`).

   **Must be a managed identity, not an App Registration.** An App Registration authenticates fine
   but then fails at actual publish time with `InvalidAccessException: The requested operation is
   not allowed` — the Marketplace resolves identity through Azure DevOps' own internal profile
   record, which only recognizes managed identities here, not arbitrary Entra apps. Confirmed the
   hard way in a July 2026 real-world writeup; see
   [this post](https://www.emrecodes.net/posts/2026/07/10/vscode-marketplace-managed-identity.html)
   for the full account.

2. **Create the GitHub Environment.** In the GitHub repo (`cratestack/cratestack`) → Settings →
   Environments → **New environment**, name it exactly `vscode-marketplace` (matches
   `release-vscode.yml`'s `publish-marketplace` job). No protection rules are required, but you can
   add required reviewers later if you want a manual gate on publishes.

   Environment-scoped OIDC subjects are used (not a branch/tag ref) specifically because Azure
   federated credentials require an *exact* subject match with no wildcards — a `refs/tags/vX.Y.Z`
   subject would be different for every release and could never match. A GitHub Environment gives a
   stable subject (`repo:cratestack/cratestack:environment:vscode-marketplace`) regardless of which
   tag triggered the run.

3. **Add a federated credential to the managed identity**, trusting that environment:
   ```bash
   az identity federated-credential create \
     --name github-actions-vscode-marketplace \
     --identity-name cratestack-vsce-publish \
     --resource-group cratestack-release \
     --issuer "https://token.actions.githubusercontent.com" \
     --subject "repo:cratestack/cratestack:environment:vscode-marketplace" \
     --audiences "api://AzureADTokenExchange"
   ```

4. **Create the publisher** at
   [marketplace.visualstudio.com/manage/createpublisher](https://marketplace.visualstudio.com/manage/createpublisher)
   if it doesn't already exist. **ID** must be `cratestack`; **Name** can be anything (e.g.
   "CrateStack").

5. **Authorize the managed identity on the publisher** — this is the step that actually grants
   publish rights (the federated credential above only proves identity to Azure, not to the
   Marketplace). On the publisher's management page, add the managed identity as a member using the
   `id` (full ARM resource ID) recorded in step 1, and assign it the **Contributor** role.

6. In the GitHub repo → Settings → Environments → `vscode-marketplace` → **Environment secrets**,
   add:
   * `AZURE_CLIENT_ID` — the `clientId` from step 1
   * `AZURE_TENANT_ID` — your tenant ID
   * `AZURE_SUBSCRIPTION_ID` — your subscription ID

Once these exist, the next tag push publishes the extension to the Marketplace — no other change
needed. No client secret is stored anywhere; `azure/login`'s OIDC exchange (gated by the job's
`id-token: write` permission) is what proves the workflow run is genuinely this repo's
`vscode-marketplace` environment.

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
