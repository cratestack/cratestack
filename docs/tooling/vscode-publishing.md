# VS Code extension publishing setup

## Current status

| Channel | State | Reaches |
|---|---|---|
| GitHub Releases | **live** — has shipped every release to date | everyone, by manual download |
| Open VSX | **armed** — publishes on the next `vX.Y.Z` tag | Cursor, Windsurf, VSCodium |
| Marketplace | **armed** — publishes on the next `vX.Y.Z` tag | VS Code |

`.github/workflows/release-vscode.yml` builds `packages/cratestack-vscode` on every `vX.Y.Z` tag
push and attaches a platform-specific `.vsix` to the same GitHub Release `release-cli.yml` creates
for that tag. That remains the only channel that has actually shipped anything: **no version has
been published to either registry yet.** See [Manual install for users](#manual-install-for-users)
for what to tell someone who wants the extension right now.

* **Open VSX: configured, not yet exercised.** The `OVSX_PAT` repo secret is set and the
  `cratestack` namespace exists (both done 2026-09-01), so `publish-openvsx` will attempt a real
  publish on the next tag instead of soft-skipping. Nothing has gone through it yet — confirm the
  first one actually lands rather than assuming, per
  [Verifying a publish actually shipped](#verifying-a-publish-actually-shipped).
* **Marketplace: credentials configured, also not yet exercised.** The `cratestack` publisher, the
  `cratestack-vsce-publish` managed identity, its federated credential, the `vscode-marketplace`
  environment, and the three `AZURE_*` secrets all exist (2026-09-01). `publish-marketplace` no
  longer soft-skips — it attempts a real publish on the next tag.

  **This changes the failure mode of a release.** While the secrets were absent the job exited 0
  and a release stayed green regardless. It will now fail the run if the setup is incomplete, and
  the step most likely to be incomplete is authorizing the managed identity **on the Marketplace
  publisher** (steps 6-7 of [Marketplace setup](#vs-code-marketplace-one-time-setup)), which is
  **not yet done** — the only part that succeeds silently when skipped and surfaces at publish time
  as `InvalidAccessException: The requested operation is not allowed`.

**Open VSX does not cover VS Code, and this is the most common thing to get wrong here.** Open VSX
is the Eclipse Foundation's registry — a separate account system from the Microsoft Marketplace.
Microsoft's VS Code build is hardwired to the Marketplace and has no way to see Open VSX, so the
Open VSX listing serves [Cursor](https://www.cursor.com/), [Windsurf](https://windsurf.com/), and
[VSCodium](https://vscodium.com/) only. VS Code users stay on the manual `.vsix` download — and so
get **no auto-updates** — until the Marketplace path is finished. Overriding `product.json`'s
`extensionsGallery` to point VS Code at Open VSX is a per-user hack that no publisher can ship: it
is reverted by every VS Code update and invalidates the app's code signature on macOS. It is not a
distribution strategy.

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

No registry listing exists yet, and VS Code users will still need this after the first Open VSX
publish lands (see [Current status](#current-status)). This is what to tell someone who wants the
extension:

1. Go to the [Releases page](https://github.com/cratestack/cratestack/releases) and open the
   latest `vX.Y.Z` release.
2. Download the `.vsix` matching your platform:
   * macOS Apple Silicon: `cratestack-vscode-plugin-darwin-arm64-*.vsix`
   * macOS Intel: `cratestack-vscode-plugin-darwin-x64-*.vsix`
   * Linux x86_64: `cratestack-vscode-plugin-linux-x64-*.vsix`
   * Linux ARM64: `cratestack-vscode-plugin-linux-arm64-*.vsix`
   * Windows x86_64: `cratestack-vscode-plugin-win32-x64-*.vsix`
3. Install it. Either:
   * Command line: `code --install-extension /path/to/cratestack-vscode-plugin-<platform>-<version>.vsix`
   * VS Code UI: Extensions view (`Ctrl+Shift+X` / `Cmd+Shift+X`) → **···** menu (top-right) →
     **Install from VSIX...** → pick the downloaded file.
4. Reload the window if VS Code doesn't prompt automatically. Opening a `.cstack` file should now
   get syntax highlighting and language-server features (hover, diagnostics, etc.).

To upgrade to a newer release, repeat the same steps with the new `.vsix` — installing over an
existing version replaces it, no uninstall needed first.

## VS Code Marketplace one-time setup

> **Steps 1-5 done (2026-09-01); steps 6-7 outstanding.** The identity, its federated credential,
> the environment, the publisher and the secrets all exist. What remains is registering the identity
> with Azure DevOps and adding it to the publisher — the part that grants publish rights.
>
> This matters now rather than later: because `AZURE_CLIENT_ID` is set, `publish-marketplace` no
> longer soft-skips. It will run for real on the next tag and **fail the release** until step 7 is
> done, where previously it exited 0 and a release stayed green regardless.

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

1. **Create a user-assigned managed identity** (needs the Azure CLI). Create the resource group
   first if it doesn't exist — `az identity create` will not create one for you, and fails with
   `(ResourceGroupNotFound)`:
   ```bash
   az group create --name cratestack-release --location <region>
   az identity create --name cratestack-vsce-publish --resource-group cratestack-release \
     --location <region>
   az identity show --name cratestack-vsce-publish --resource-group cratestack-release \
     --query "{clientId:clientId, principalId:principalId, id:id}" -o json
   ```
   **`--location` is required on `az identity create`**, even though the resource group already has
   one — omitting it fails with `InvalidArgumentValue: Missing required field: --location`. Use the
   same region as the group.

   If this is the subscription's first managed identity, the CLI registers the
   `Microsoft.ManagedIdentity` resource provider automatically and says so; that's expected, not an
   error.

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

5. **Add the GitHub secrets** (moved ahead of the publisher authorization below, which now needs
   them). In the GitHub repo → Settings → Environments → `vscode-marketplace` → **Environment
   secrets**, add:
   * `AZURE_CLIENT_ID` — the `clientId` from step 1
   * `AZURE_TENANT_ID` — your tenant ID
   * `AZURE_SUBSCRIPTION_ID` — your subscription ID

   These are **environment** secrets, scoped to `vscode-marketplace` — visible only to a job
   declaring that environment, which is exactly the set of jobs entitled to them. (Repository
   secrets would also resolve, since `secrets.X` falls through to repo scope, but they would then be
   readable by every other workflow for no benefit.)

   **`OVSX_PAT` must stay a repository secret and must not be moved here.** `publish-openvsx`
   declares no `environment:`, so it cannot see environment secrets at all. Moving it would not
   raise an error — the job's `if [ -z "${OVSX_PAT:-}" ]` guard would find nothing, log a skip and
   exit 0, and Open VSX publishing would silently stop working while every release stayed green.

6. **Get the managed identity's Azure DevOps profile ID.** This is the value the Marketplace member
   field wants, and it does not exist until you ask for it. Run the `Marketplace Identity ID`
   workflow:

   ```bash
   gh workflow run "Marketplace Identity ID" --repo cratestack/cratestack
   ```

   It prints the ID to the run summary. **This cannot be done from a laptop**, and the reason is
   worth understanding rather than working around: the endpoint is
   `/profile/profiles/me`, which returns the profile of *whoever is authenticated*. The value needed
   is the managed identity's profile, and a user-assigned managed identity can only be assumed from
   inside an Azure resource or through workload identity federation — here, scoped to the exact
   subject `repo:cratestack/cratestack:environment:vscode-marketplace`. A job in that environment is
   the only caller that can authenticate as it.

   Running `az rest -u https://app.vssps.visualstudio.com/_apis/profile/profiles/me --resource
   499b84ac-1321-427f-aa17-267ca6975798` locally does not fail — it returns *your own* profile ID.
   That ID is real, the Marketplace accepts it as a member, and publishing then fails as the
   identity anyway, with the same `InvalidAccessException` as adding no member at all. There is no
   error message anywhere that points at the mix-up.

   The call also registers the identity with Azure DevOps as a side effect on first invocation,
   which is why it has to happen before step 7 rather than being a read-only lookup.

   **The identity deliberately holds no role on the subscription**, because publishing
   authenticates to Azure DevOps rather than to ARM. Both this workflow and `publish-marketplace`
   therefore pass `allow-no-subscriptions: true` to `azure/login` **and omit `subscription-id`**.
   Getting only half of that produces a second, different failure:

   | login inputs | result |
   |---|---|
   | no flag | `No subscriptions found for ***` (enumeration) |
   | flag + `subscription-id` | `The subscription of '***' doesn't exist in cloud 'AzureCloud'` (selection) |
   | flag, no `subscription-id` | works |

   Both failures land *after* a successful OIDC exchange and both advise `Double check if the
   'auth-type' is correct` — the one thing that was never wrong. Granting the identity a subscription
   role would also work, at the cost of a standing ARM permission nothing here uses.

   `AZURE_SUBSCRIPTION_ID` is consequently no longer read by any workflow. It is kept as a secret
   only because it costs nothing and a future ARM-touching job would want it.

7. **Authorize the managed identity on the publisher** — this is the step that actually grants
   publish rights; the federated credential above only proves identity to Azure, not to the
   Marketplace. At
   [the publisher's management page](https://marketplace.visualstudio.com/manage/publishers/cratestack)
   → **Members** → **Add**, paste the profile ID from step 6 and assign the **Contributor** role.

   Use that value specifically. The managed identity's `clientId`, `principalId`, `tenantId`, and
   full ARM resource ID are all rejected or silently wrong here — none of them is what Azure DevOps
   keys membership on. (This document previously said to use the ARM resource ID. That was wrong;
   see [the writeup](https://www.emrecodes.net/posts/2026/07/10/vscode-marketplace-managed-identity.html)
   this setup is based on.)

Once all seven are done, the next tag push publishes the extension to the Marketplace — no other
change needed. No client secret is stored anywhere; `azure/login`'s OIDC exchange (gated by the job's
`id-token: write` permission) is what proves the workflow run is genuinely this repo's
`vscode-marketplace` environment.

Step 7 is the only one that gives no signal when skipped: steps 1-6 all succeed without it, and the
omission surfaces only at publish time as
`InvalidAccessException: The requested operation is not allowed`.

## Open VSX one-time setup

> **Done — kept as reference.** Every step below has been completed: the `cratestack` namespace
> exists and `OVSX_PAT` is set, so the next tag publishes for real. Retained as the procedure for
> re-issuing a revoked token or onboarding another publisher. Remember this channel reaches
> Cursor/Windsurf/VSCodium but **not** VS Code — see [Current status](#current-status).

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
  five `cratestack-vscode-plugin-*.vsix` assets alongside the CLI binaries.
* Open VSX (armed — **check this after the next tag; no publish has landed here yet**):
  `https://open-vsx.org/extension/cratestack/cratestack-vscode-plugin` shows the new version. The
  API is scriptable, and a 404 means nothing shipped:
  ```bash
  curl -s -o /dev/null -w '%{http_code}\n' https://open-vsx.org/api/cratestack/cratestack-vscode-plugin
  ```
  This matters more than usual here: `publish-openvsx` exits 0 when `OVSX_PAT` is missing, so a
  green job is not evidence of a publish.
* Marketplace (armed — **check this after the next tag; no publish has landed here yet**):
  `https://marketplace.visualstudio.com/items?itemName=cratestack.cratestack-vscode-plugin` shows
  the new version. Unlike Open VSX, a failure here is loud: with `AZURE_CLIENT_ID` set the job runs
  for real, so a broken setup shows up as a red `publish (Marketplace)` job, not a silent skip.

## Display name collisions

`package.json`'s `displayName` must be unique across the **entire** Marketplace, independently of the
extension ID. `cratestack.cratestack-vscode-plugin` was accepted at v0.10.1 and the publish still
failed:

```
Publishing 'cratestack.cratestack-vscode-plugin (darwin-arm64) v0.10.1'...
##[error]This extension display name is taken. Please try a different one.
```

The name was `CrateStack`; it is now `CrateStack Schema`.

Two things make this expensive to hit, both worth knowing before changing the value again:

* **The gallery search is not a valid pre-check.** Querying `extensionquery` for `CrateStack` across
  the whole gallery — not just `Microsoft.VisualStudio.Code` — returned zero results while the name
  was demonstrably taken. Whatever holds it is unlisted, removed, or reserved internally. The only
  reliable signal is an actual publish attempt.
* **`displayName` is baked into the `.vsix` at package time**, so a change needs a rebuild, and a
  failed release is bumped past rather than re-run. Each attempt therefore costs a version.

Open VSX has no such constraint and published `CrateStack` without complaint at v0.10.1, so the two
registries can disagree about whether a given name is available.

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

## `.cstack` file icon

Distinct from the extension icon above, and a different mechanism. `icon.png` brands the *listing*;
`packages/cratestack-vscode/icons/cstack-{light,dark}.svg` brand the *file rows* in the explorer
tree, via `contributes.languages[].icon`:

```json
"icon": {
  "light": "./icons/cstack-light.svg",
  "dark": "./icons/cstack-dark.svg"
}
```

**This is a fallback, not an override.** VS Code shows a language icon only when the active file icon
theme has no icon of its own for that language or extension, and does not set
`"showLanguageModeIcons": false`. Under Seti (the default theme) `.cstack` matches nothing, so the
contributed icon renders; under a theme that already ships a `.cstack` glyph, or one that opts out,
it will not. That is the intended design — an extension cannot force a file icon into a theme the
user chose, and shipping a whole icon theme to get one would make users abandon Seti or Material to
see it.

Requires VS Code **1.64+** (microsoft/vscode#14662). `engines.vscode` is `^1.91.0`, comfortably above
that, and `test/language-icon.test.js` asserts the floor stays above it — below 1.64 the contribution
is parsed and silently ignored, so lowering the floor would un-ship the icon for exactly the users a
lower floor was widened to reach.

Constraints:

* **SVG, transparent background.** The explorer renders these at 16x16 against its own background;
  the gallery mark's `#1E222E` tile must not be carried over or it renders as a dark box on every
  theme that isn't that navy.
* **Both variants are required by convention here.** `light` is used with light colour themes,
  `dark` with dark ones. They share geometry; the light variant deepens the palette one step
  (`#F7B270`/`#E88A3A`/`#BF6A26` → `#EDA05C`/`#D97B2E`/`#A85920`) because the mark's pale top face
  washes out against a near-white explorer background at 16px.
* **Same denylist caveat as above** — verify against the built archive:

  ```
  pnpm run package:vsix && unzip -l ./*.vsix | grep -i icons/
  ```

`test/language-icon.test.js` guards the manifest half offline: the `icon` block exists with both
variants, both resolve to real files, both are SVG, and the engines floor still supports the feature.
Like `icon.test.js` it does not assert archive contents, for the same reason.

What none of this can check is whether the icon *looks* right at 16x16 in a real explorer — that
needs a human with VS Code open.
