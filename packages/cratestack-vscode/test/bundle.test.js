const test = require("node:test");
const assert = require("node:assert/strict");
const { execFileSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const pkg = require("../package.json");

const extensionRoot = path.resolve(__dirname, "..");

// Reproduces the packaged-VSIX environment without needing vsce or a network:
// `.vscodeignore` excludes `node_modules/**` and the packaging scripts run
// `vsce --no-dependencies`, so whatever `main` points at must resolve every
// dependency on its own. Only `vscode` is host-injected, so only `vscode` is
// stubbed.
//
// This is the guard for a real shipped defect: with an unbundled `main`, every
// released VSIX failed activation with
// `Cannot find module 'vscode-languageclient/node'` — users got the TextMate
// grammar and no language server. It went unnoticed because the package had no
// `test` script, so turbo skipped it in CI entirely.
function loadInSandbox() {
  const main = path.join(extensionRoot, pkg.main);
  assert.ok(
    fs.existsSync(main),
    `expected a built bundle at ${pkg.main} — run \`pnpm run build\` first`,
  );

  const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), "cratestack-vscode-bundle-"));
  fs.copyFileSync(main, path.join(sandbox, "entry.js"));

  // vscode-languageclient subclasses host classes (`code.CompletionItem` and
  // friends) at module scope, so the stub has to hand back something
  // extendable for any name — hence a Proxy rather than a fixed object.
  //
  // It reports `__esModule: true` on purpose. The library is tsc-compiled and
  // reaches `vscode` through `__importStar`, which otherwise rebuilds a plain
  // namespace object from `ownKeys(mod)` — and a Proxy with no `ownKeys` trap
  // enumerates nothing, so every binding would come back undefined. The
  // `__esModule` branch returns the module untouched, which is what keeps the
  // `get` trap in play.
  const stub = path.join(sandbox, "node_modules", "vscode");
  fs.mkdirSync(stub, { recursive: true });
  fs.writeFileSync(
    path.join(stub, "index.js"),
    [
      "const cache = new Map();",
      "module.exports = new Proxy({}, {",
      "  get(_target, prop) {",
      '    if (prop === "__esModule") return true;',
      '    if (typeof prop === "symbol") return undefined;',
      "    if (!cache.has(prop)) cache.set(prop, class Stub {});",
      "    return cache.get(prop);",
      "  },",
      "});",
      "",
    ].join("\n"),
  );

  return execFileSync(
    process.execPath,
    ["-e", "console.log(Object.keys(require('./entry.js')).sort().join(','))"],
    { cwd: sandbox, encoding: "utf8" },
  ).trim();
}

test("bundled entry point loads with no dependencies but host-injected vscode", () => {
  assert.equal(loadInSandbox(), "activate,deactivate");
});

test("package main points inside the bundled output directory", () => {
  // `.vscodeignore` excludes the unbundled sources, so a `main` that escaped
  // `dist/` would package a file that is not in the VSIX at all.
  assert.match(pkg.main, /^\.\/dist\//);
});
