import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import process from 'node:process';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(scriptDir, '..');
const repoRoot = path.resolve(extensionRoot, '..', '..');
const platform = process.platform;
const executable = platform === 'win32' ? 'cratestack-lsp.exe' : 'cratestack-lsp';
const source = process.argv[2]
  ? path.resolve(extensionRoot, process.argv[2])
  : path.join(repoRoot, 'target', 'release', executable);
const targetDir = path.join(extensionRoot, 'server', platform);
const target = path.join(targetDir, executable);

if (!fs.existsSync(source)) {
  console.error(`cratestack-lsp binary not found at ${source}`);
  console.error('Build it first with `cargo build --release -p cratestack-lsp`.');
  process.exit(1);
}

fs.mkdirSync(targetDir, { recursive: true });
fs.copyFileSync(source, target);

if (platform !== 'win32') {
  fs.chmodSync(target, 0o755);
}

console.log(`staged ${source} -> ${target}`);
