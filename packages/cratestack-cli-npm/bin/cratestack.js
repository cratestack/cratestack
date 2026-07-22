#!/usr/bin/env node
// Thin shim that execs the binary scripts/install.js downloaded into
// ./bin/ at install time. Set CRATESTACK_CLI_BINARY_PATH to point at a
// manually provided binary instead (e.g. a vendored/offline install).

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const binName = process.platform === "win32" ? "cratestack.exe" : "cratestack";
const binaryPath =
  process.env.CRATESTACK_CLI_BINARY_PATH || path.join(__dirname, binName);

if (!fs.existsSync(binaryPath)) {
  console.error(
    `cratestack binary not found at ${binaryPath}.\n` +
      "The postinstall download may have failed or been skipped " +
      "(CRATESTACK_CLI_SKIP_DOWNLOAD, --ignore-scripts). Reinstall with:\n" +
      "  npm install @cratestack/cli\n" +
      "or fall back to:\n" +
      "  cargo binstall cratestack-cli",
  );
  process.exit(1);
}

const result = spawnSync(binaryPath, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`failed to run ${binaryPath}: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status === null ? 1 : result.status);
