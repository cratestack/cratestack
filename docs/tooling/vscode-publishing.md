# VS Code extension publishing setup

## Current status: GitHub Releases only

`.github/workflows/release-vscode.yml` builds `packages/cratestack-vscode` on every `vX.Y.Z` tag
push and attaches a platform-specific `.vsix` to the same GitHub Release
`release-cli.yml` creates for that tag — **this is the only channel that actually ships the
extension today.** There is no Marketplace or Open VSX listing yet. See
[Manual install for users](#manual-install-for-users) below for what to tell someone who wants the
extension right now.

The Marketplace and Open VSX publish jobs described in the rest of this document exist in the
workflow and are fully implemented, but are currently **dormant**: each checks for its own
credential/secret and soft-skips (logs why, doesn't fail the run) when it isn't set. Re-enabling
either is a matter of adding the credential described in its section below — no workflow rewrite
needed.

* **Marketplace: blocked indefinitely.** The maintainer's Microsoft account is stuck in a 2FA
  block/unblock loop, so the Entra ID managed-identity setup this depends on (needs a working
  Azure sign-in) cannot currently be completed. See [Marketplace setup](#vs-code-marketplace-one-time-setup)
  below for what would need to happen if that unblocks.
* **Open VSX: not blocked at all, and worth doing.** Open VSX is run by the **Eclipse
  Foundation** — a separate, independent registry from the Microsoft Marketplace, with its own
  account system (an Eclipse account, not a Microsoft one). It's the registry
  [VSCodium](https://vscodium.com/), [Cursor](https://www.cursor.com/), and
  [Windsurf](https://windsurf.com/) use for extensions, since none of them can point at the
  Microsoft-licensed Marketplace. Nothing about the Microsoft 2FA problem affects this path — see
  [Open VSX one-time setup](#open-vsx-one-time-setup) below.

---

The extension bundles a native `cratestack-lsp` binary per platform (see `server-path.js`), so the
`build` job builds `cratestack-lsp` on a native runner per target and packages a platform-specific
vsix via `vsce package --target <target>` for each of
`darwin-x64`/`darwin-arm64`/`linux-x64`/`linux-arm64`/`win32-x64` — the same way the Marketplace
itself distributes per-platform extensions. `attach-github-release` runs once (on `ubuntu-latest`,
after `build` finishes) and attaches every platform's vsix to the tag's GitHub Release, the same
way `release-cli.yml`'s own `release` job attaches the `cratestack-cli` archives — same action
(`softprops/action-gh-release`), same upsert-by-tag behavior, so whichever of the two workflows'
release job runs first creates the Release and the other appends to it.

`publish-marketplace` and `publish-openvsx` each also run
once (on `ubuntu-latest`, after `build` finishes) against every vsix `build` produced — publishing a
pre-built vsix doesn't need the native OS, so there's no reason to repeat the publish step per
platform. Both publish jobs soft-skip (log a warning, exit 0, without failing the rest of the run)
when their credentials aren't set, so the workflow succeeds even before everything below is
configured.

## Manual install for users

Since there's no Marketplace or Open VSX listing yet, this is what to tell someone who wants the
extension:

1. Go to the [Releases page](https://github.com/cratestack/cratestack/releases) and open the
   latest `vX.Y.Z` release.
2. Download the `.vsix` matching your platform:
   * macOS Apple Silicon: `cratestack-vscode-darwin-arm64-*.vsix`
   * macOS Intel: `cratestack-vscode-darwin-x64-*.vsix`
   * Linux x86_64: `cratestack-vscode-linux-x64-*.vsix`
   * Linux ARM64: `cratestack-vscode-linux-arm64-*.vsix`
   * Windows x86_64: `cratestack-vscode-win32-x64-*.vsix`
3. Install it. Either:
   * Command line: `code --install-extension /path/to/cratestack-vscode-<platform>-<version>.vsix`
   * VS Code UI: Extensions view (`Ctrl+Shift+X` / `Cmd+Shift+X`) → **···** menu (top-right) →
     **Install from VSIX...** → pick the downloaded file.
4. Reload the window if VS Code doesn't prompt automatically. Opening a `.cstack` file should now
   get syntax highlighting and language-server features (hover, diagnostics, etc.).

To upgrade to a newer release, repeat the same steps with the new `.vsix` — installing over an
existing version replaces it, no uninstall needed first.

## VS Code Marketplace one-time setup

> **Currently blocked.** The maintainer's Microsoft account is stuck in a 2FA block/unblock loop,
> which blocks the Azure sign-in step 1 below needs. This section is left in place, accurate and
> ready to follow, for whenever that unblocks — nothing about the setup itself has changed.

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

> **Not blocked by anything above.** Open VSX is the Eclipse Foundation's registry — a completely
> separate account system from the Microsoft Marketplace (an Eclipse account, not a Microsoft
> one), so the 2FA issue blocking the Marketplace section above has no bearing here. This is a
> genuinely available path to a real, install-from-the-editor registry today, not just the manual
> `.vsix` download above — worth doing independently of when (or whether) the Marketplace path
> unblocks.

The namespace is `cratestack`, matching the same `publisher` field.

1. Register an Eclipse account at [accounts.eclipse.org](https://accounts.eclipse.org) if you don't
   have one — the GitHub username on the Eclipse account must exactly match the GitHub account used
   to sign into Open VSX.
2. Sign the Publisher Agreement: log into [open-vsx.org](https://open-vsx.org) via GitHub, go to
   profile settings, choose "Log in with Eclipse" and authorize it, then click "Show Publisher
   Agreement" and "Agree".
3. Generate an access token: open-vsx.org → Settings → **Access Tokens** → **Generate New Token**.
   Copy the value immediately — it's only shown once.
4. Create the namespace once, locally (needs Node/pnpm and the token from step 3). Run it from
   `packages/cratestack-vscode` after `pnpm install`, so `npx` resolves the `ovsx` pinned in that
   package's `devDependencies` rather than downloading whatever the registry currently serves —
   this command carries the token on its command line:
   ```bash
   cd packages/cratestack-vscode && pnpm install --frozen-lockfile
   npx ovsx create-namespace cratestack -p <token>
   ```
5. In the GitHub repo → Settings → Secrets and variables → Actions, add a repository secret named
   `OVSX_PAT` with that same token.

Once the secret exists and the namespace has been created (step 4 is a one-time local action, not
part of CI), the next tag push publishes the extension to Open VSX — no other change needed.

## Verifying a publish actually shipped

A green `release-vscode.yml` run only proves the CLI exited 0 — for the same reason `RELEASE.md`
independently verifies crates.io/npm rather than trusting the checkmark, confirm directly:

* GitHub Release (the primary path — check this one first): `gh release view vX.Y.Z` should list
  five `cratestack-vscode-*.vsix` assets alongside the CLI binaries.
* Marketplace (dormant until configured, see above):
  `https://marketplace.visualstudio.com/items?itemName=cratestack.cratestack-vscode` shows the new
  version.
* Open VSX (dormant until configured, see above):
  `https://open-vsx.org/extension/cratestack/cratestack-vscode` shows the new version.

## Extension icon

`packages/cratestack-vscode/icon.png` — a 256x256 PNG, referenced by `package.json`'s `icon` field
and paired with a `galleryBanner` (`#1E222E`, dark theme). It ships inside every `.vsix`: the field
is platform-independent, so all five `vsce_target` builds carry it without per-target work.

This closed cratestack#782. Neither registry *requires* an icon to accept a publish, which is why the
extension went without one for its first releases — but both listings, and the in-editor Extensions
sidebar after a manual `.vsix` install, fall back to a generic grey placeholder without it.

Constraints worth knowing before changing it, because a Marketplace publish cannot be cleanly deleted
and retried:

* **PNG only.** SVG is rejected.
* **Square, at least 128x128.** 256 is the source size here, for HiDPI listing rendering.
* **`.vscodeignore` is a denylist**, so a top-level asset is included by default — but that also means
  a future entry could silently exclude it. Verify against the built archive, not the source tree:

  ```
  pnpm run package:vsix && unzip -l ./*.vsix | grep -i icon
  ```

  (`package:vsix` runs `stage-server` first, which needs `cargo build --release -p cratestack-lsp`.)

`test/icon.test.js` guards the manifest half offline — that the field exists, that it resolves to a
real file, and that the file is a square PNG of at least 128x128, reading the dimensions straight out
of the PNG IHDR chunk. It deliberately does *not* assert the archive contents; the source tree is the
wrong place to detect a `.vscodeignore` exclusion, hence the `unzip` check above.
