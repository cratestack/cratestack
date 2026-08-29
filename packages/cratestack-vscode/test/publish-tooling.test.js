const test = require("node:test");
const assert = require("node:assert/strict");

const pkg = require("../package.json");

// cratestack#811. The publish scripts invoke their tooling through `npx`,
// which resolves a locally installed binary when one exists and otherwise
// downloads the package from the registry at runtime. Those two outcomes
// are indistinguishable from reading the script, and the second one is a
// supply-chain hole: `publish:open-vsx` and the `publish (Open VSX)` job
// in release-vscode.yml both run with a live `OVSX_PAT` in the
// environment, so an undeclared tool means an unpinned, unreviewed
// download executing next to a publish token.
//
// `ovsx` was exactly that until this test existed — declared in neither
// `devDependencies` nor `pnpm-lock.yaml`, while `@vscode/vsce` beside it
// was pinned and resolved locally. Nothing failed: the job would have
// gone green on whatever the registry served that day. An Open VSX or
// Marketplace publish cannot be cleanly deleted and retried, so "it
// worked last time" is not a recoverable position to discover otherwise
// from.
//
// Guarding the general rule rather than the one package that broke it:
// any tool a script reaches for via `npx` must be a declared dependency,
// so `pnpm install --frozen-lockfile` is what puts it on disk and the
// lockfile is what pins it.

const declared = new Set([
  ...Object.keys(pkg.dependencies ?? {}),
  ...Object.keys(pkg.devDependencies ?? {}),
]);

// `npx` accepts flags before the package name (`npx --offline foo`), and
// package names may be scoped (`@vscode/vsce`). Take the first token that
// is not a flag.
const NPX_INVOCATION = /\bnpx\s+((?:-{1,2}[^\s]+\s+)*)(@?[\w.@/-]+)/g;

function npxToolsIn(script) {
  return [...script.matchAll(NPX_INVOCATION)].map((m) => m[2]);
}

test("every tool invoked via npx is a declared dependency", () => {
  const undeclared = [];

  for (const [name, script] of Object.entries(pkg.scripts ?? {})) {
    for (const tool of npxToolsIn(script)) {
      if (!declared.has(tool)) {
        undeclared.push({ script: name, tool });
      }
    }
  }

  assert.deepEqual(
    undeclared,
    [],
    `these scripts invoke a tool via npx that is not in dependencies or devDependencies:\n` +
      undeclared.map((u) => `  ${u.script}: npx ${u.tool}`).join("\n") +
      `\n\nAn undeclared tool is fetched from the registry at run time, unpinned. ` +
      `The publish scripts run with a live registry token in the environment, so add it ` +
      `to devDependencies (which also lands it in pnpm-lock.yaml) rather than letting npx ` +
      `resolve it over the network.`,
  );
});

// A guard that finds nothing because it looked nowhere is worse than no
// guard: if the regex above stops matching (a script is reworded, `npx`
// is swapped for `pnpm exec`), the test above passes vacuously and the
// protection is silently gone. Pin the fact that it is still looking at
// something.
test("the npx scan actually matches the publish scripts", () => {
  const found = Object.values(pkg.scripts ?? {}).flatMap(npxToolsIn);

  assert.ok(
    found.includes("ovsx"),
    "expected to find `npx ovsx` in the scripts — if the Open VSX publish script was " +
      "renamed or changed shape, update this test rather than deleting it",
  );
  assert.ok(
    found.includes("@vscode/vsce"),
    "expected to find `npx @vscode/vsce` in the scripts — the scoped-name case is the " +
      "one most likely to break the scan's regex",
  );
});
