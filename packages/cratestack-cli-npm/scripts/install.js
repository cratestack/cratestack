#!/usr/bin/env node
// Downloads the cratestack-cli binary matching this package's version and
// the host platform from the GitHub Release the release-cli.yml workflow
// publishes, verifies its checksum, and extracts it to ./bin/.
//
// Archive naming and layout here must stay in sync with:
//   - .github/workflows/release-cli.yml ("Package archive" step)
//   - crates/cratestack-cli/Cargo.toml   ([package.metadata.binstall])

"use strict";

const fs = require("node:fs");
const https = require("node:https");
const os = require("node:os");
const path = require("node:path");
const crypto = require("node:crypto");
const { execFileSync } = require("node:child_process");

const pkg = require("../package.json");

const REPO = "https://github.com/cratestack/cratestack";
const BIN_DIR = path.join(__dirname, "..", "bin");

function resolveTarget() {
  const platform = process.platform;
  const arch = process.arch;
  const targets = {
    "darwin:x64": "x86_64-apple-darwin",
    "darwin:arm64": "aarch64-apple-darwin",
    "linux:x64": "x86_64-unknown-linux-gnu",
    "linux:arm64": "aarch64-unknown-linux-gnu",
    "win32:x64": "x86_64-pc-windows-msvc",
  };
  const target = targets[`${platform}:${arch}`];
  if (!target) {
    throw new Error(
      `@cratestack/cli has no prebuilt binary for ${platform}/${arch}. ` +
        `Supported: ${Object.keys(targets).join(", ")}. ` +
        "Install from source instead: cargo install cratestack-cli",
    );
  }
  return target;
}

function fetchBuffer(url, redirectsLeft = 5) {
  return new Promise((resolve, reject) => {
    https
      .get(url, { headers: { "User-Agent": "cratestack-cli-npm-installer" } }, (res) => {
        if (
          res.statusCode >= 300 &&
          res.statusCode < 400 &&
          res.headers.location &&
          redirectsLeft > 0
        ) {
          res.resume();
          resolve(fetchBuffer(res.headers.location, redirectsLeft - 1));
          return;
        }
        if (res.statusCode !== 200) {
          reject(new Error(`GET ${url} failed: HTTP ${res.statusCode}`));
          res.resume();
          return;
        }
        const chunks = [];
        res.on("data", (chunk) => chunks.push(chunk));
        res.on("end", () => resolve(Buffer.concat(chunks)));
        res.on("error", reject);
      })
      .on("error", reject);
  });
}

function verifyChecksum(assetName, archiveBuf, sha256Buf) {
  const expected = sha256Buf.toString("utf8").trim().split(/\s+/)[0];
  const actual = crypto.createHash("sha256").update(archiveBuf).digest("hex");
  if (expected !== actual) {
    throw new Error(
      `checksum mismatch for ${assetName}: expected ${expected}, got ${actual}`,
    );
  }
}

function extract(archivePath, ext, destDir) {
  fs.mkdirSync(destDir, { recursive: true });
  if (ext === "zip") {
    execFileSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        `Expand-Archive -Path '${archivePath}' -DestinationPath '${destDir}' -Force`,
      ],
      { stdio: "inherit" },
    );
  } else {
    execFileSync("tar", ["-xzf", archivePath, "-C", destDir], { stdio: "inherit" });
  }
}

async function main() {
  if (process.env.CRATESTACK_CLI_SKIP_DOWNLOAD) {
    console.log("CRATESTACK_CLI_SKIP_DOWNLOAD set; skipping binary download.");
    return;
  }

  const target = resolveTarget();
  const version = pkg.version;
  const ext = process.platform === "win32" ? "zip" : "tar.gz";
  const assetName = `cratestack-${target}-v${version}.${ext}`;
  const assetUrl = `${REPO}/releases/download/v${version}/${assetName}`;

  console.log(`Downloading ${assetName} ...`);
  const [archiveBuf, checksumBuf] = await Promise.all([
    fetchBuffer(assetUrl),
    fetchBuffer(`${assetUrl}.sha256`),
  ]);
  verifyChecksum(assetName, archiveBuf, checksumBuf);

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "cratestack-cli-"));
  const archivePath = path.join(tmpDir, assetName);
  fs.writeFileSync(archivePath, archiveBuf);

  extract(archivePath, ext, tmpDir);

  const binName = process.platform === "win32" ? "cratestack.exe" : "cratestack";
  const extracted = path.join(tmpDir, binName);
  if (!fs.existsSync(extracted)) {
    throw new Error(`expected ${binName} inside ${assetName}, but it was not found`);
  }

  fs.mkdirSync(BIN_DIR, { recursive: true });
  const installed = path.join(BIN_DIR, binName);
  fs.copyFileSync(extracted, installed);
  if (process.platform !== "win32") {
    fs.chmodSync(installed, 0o755);
  }
  fs.rmSync(tmpDir, { recursive: true, force: true });

  console.log(`cratestack-cli ${version} (${target}) installed to ${installed}`);
}

main().catch((err) => {
  console.error(`@cratestack/cli install failed: ${err.message}`);
  console.error(
    "Fall back to: cargo binstall cratestack-cli   or   cargo install cratestack-cli",
  );
  process.exit(1);
});
