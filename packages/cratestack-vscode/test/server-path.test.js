const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { bundledServerPath, resolveServerCommand } = require('../server-path');

test('resolveServerCommand prefers an explicit configured path', () => {
  const command = resolveServerCommand('/tmp/cratestack-vscode', '/custom/cratestack-lsp');
  assert.equal(command, '/custom/cratestack-lsp');
});

test('bundledServerPath returns staged platform binary when present', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cratestack-vscode-'));
  const serverDir = path.join(root, 'server', process.platform);
  const executable = process.platform === 'win32' ? 'cratestack-lsp.exe' : 'cratestack-lsp';
  const binary = path.join(serverDir, executable);

  fs.mkdirSync(serverDir, { recursive: true });
  fs.writeFileSync(binary, '');

  assert.equal(bundledServerPath(root), binary);
  assert.equal(resolveServerCommand(root, 'cratestack-lsp'), binary);
});
