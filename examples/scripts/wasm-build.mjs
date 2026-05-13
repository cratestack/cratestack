#!/usr/bin/env node
// Cross-example helper: runs `wasm-pack build` from the parent directory of
// the calling `web/` folder, ensuring `CC` points at a wasm-capable clang.
//
// Why this exists: `sqlite-wasm-rs` compiles SQLite's C source to wasm via
// `cc-rs`. Apple's stock Xcode clang doesn't include the wasm32 backend, so
// we need to point `CC` at Homebrew's LLVM (or any wasm-capable clang) on
// macOS. On Linux, Clang 14+ from the distro packages works out of the box.
//
// Set `CRATESTACK_CC` to override the detected compiler. Otherwise this
// script picks the first of:
//   - /opt/homebrew/opt/llvm/bin/clang  (macOS Apple Silicon)
//   - /usr/local/opt/llvm/bin/clang     (macOS Intel)
//   - whatever `CC` already is in the environment
//   - `clang` from `PATH`
//
// Arguments after `--` are forwarded verbatim to wasm-pack.

import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const candidates = [
  process.env.CRATESTACK_CC,
  '/opt/homebrew/opt/llvm/bin/clang',
  '/usr/local/opt/llvm/bin/clang',
  process.env.CC,
  'clang',
].filter(Boolean);

function pickClang() {
  for (const candidate of candidates) {
    if (!candidate) continue;
    // Absolute path: only use if it exists. Bare `clang`: trust PATH.
    if (candidate.includes('/')) {
      if (existsSync(candidate)) return candidate;
    } else {
      return candidate;
    }
  }
  return 'clang';
}

const cc = pickClang();
const env = {
  ...process.env,
  CC: cc,
  CC_wasm32_unknown_unknown: cc,
};

// `web/` is the cwd this npm script runs in; wasm-pack expects to run in the
// parent (the Cargo crate root).
const __filename = fileURLToPath(import.meta.url);
const scriptDir = dirname(__filename); // .../examples/scripts
const crateDir = resolve(process.cwd(), '..'); // .../examples/<name>

const args = process.argv.slice(2);

console.log(`[wasm-build] CC=${cc}`);
console.log(`[wasm-build] crate=${crateDir}`);
console.log(`[wasm-build] running wasm-pack build ${args.join(' ')}`);

const result = spawnSync('wasm-pack', ['build', ...args], {
  cwd: crateDir,
  env,
  stdio: 'inherit',
});

if (result.error) {
  console.error(`[wasm-build] failed to launch wasm-pack: ${result.error.message}`);
  console.error(`[wasm-build] is wasm-pack on PATH? Install via:`);
  console.error(`            cargo install wasm-pack`);
  process.exit(1);
}

process.exit(result.status ?? 1);

// Avoid 'unused' lint on scriptDir — kept for future use (e.g. resolving
// shared TS templates).
void scriptDir;
