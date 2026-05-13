#!/usr/bin/env node
// Wrapper around `wasm-build.mjs` for the react-nextjs-daisyui layout,
// where the wasm crate lives at `../wasm` (sibling to `web/`) and the
// generated bundle must land in `web/public/pkg/` so Next.js serves it.
//
// Pass `--dev` to forward `--dev` to wasm-pack, otherwise builds release.

import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const scriptDir = dirname(__filename);

const passthrough = process.argv.slice(2);
const dev = passthrough.includes('--dev');

const args = [
  scriptDir + '/wasm-build.mjs',
  '--crate',
  '../wasm',
  '--target',
  'web',
  '--out-dir',
  '../web/public/pkg',
  '--no-typescript',
];
if (dev) args.push('--dev');

// `--no-typescript` skips wasm-pack's auto-generated d.ts that gets stale
// against the Rust source; the protocol.ts file in the worker is the
// stable contract from the JS side's perspective. Drop the flag if you
// prefer to consume the generated types directly.

const result = spawnSync('node', args, { stdio: 'inherit' });
process.exit(result.status ?? 1);
