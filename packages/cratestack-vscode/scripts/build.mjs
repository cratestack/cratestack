// Bundles the extension entry point (and its local `server-path.js` helper)
// into a single CommonJS file under `dist/`.
//
// This is load-bearing, not a size optimization. `.vscodeignore` excludes
// `node_modules/**` and the packaging scripts run `vsce --no-dependencies`
// (pnpm's symlinked layout is why — vsce's npm-style production dependency
// discovery does not follow it), so an *unbundled* entry point resolves
// nothing at activation time: every VSIX shipped before this script existed
// died with `Cannot find module 'vscode-languageclient/node'`, leaving users
// with the TextMate grammar and no language server at all. Inlining the
// dependency is what makes `--no-dependencies` a true statement.
//
// `test/bundle.test.js` is the regression guard and reproduces exactly that
// environment. `vscode` stays external — the host injects it at runtime and it
// is not installable from npm.

import path from "node:path";
import { fileURLToPath } from "node:url";

import esbuild from "esbuild";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(scriptDir, "..");

await esbuild.build({
  entryPoints: [path.join(extensionRoot, "extension.js")],
  outfile: path.join(extensionRoot, "dist", "extension.js"),
  bundle: true,
  platform: "node",
  format: "cjs",
  // VS Code 1.91 ships Electron with Node 20; `engines.vscode` in
  // package.json is the source of truth for that floor.
  target: "node20",
  external: ["vscode"],
  logLevel: "info",
});

console.log(`bundled ${path.join(extensionRoot, "dist", "extension.js")}`);
