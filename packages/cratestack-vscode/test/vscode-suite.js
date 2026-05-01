const assert = require('node:assert/strict');
const path = require('node:path');
const vscode = require('vscode');

async function waitFor(predicate, timeoutMs = 15000, intervalMs = 200) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const value = await predicate();
    if (value) {
      return value;
    }
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
  }
  throw new Error('Timed out waiting for condition');
}

async function run() {
  const repoRoot = path.resolve(__dirname, '..', '..', '..');
  const serverBinary = path.join(repoRoot, 'target', 'debug', process.platform === 'win32' ? 'cratestack-lsp.exe' : 'cratestack-lsp');
  await vscode.workspace.getConfiguration('cratestack').update('lsp.path', serverBinary, vscode.ConfigurationTarget.Global);

  const extension = vscode.extensions.getExtension('vaam-store.cratestack-vscode');
  assert.ok(extension, 'extension should be discoverable');
  await extension.activate();

  const fixture = path.join(__dirname, 'fixtures', 'invalid-relation.cstack');
  const uri = vscode.Uri.file(fixture);
  const document = await vscode.workspace.openTextDocument(uri);
  await vscode.window.showTextDocument(document);

  assert.equal(document.languageId, 'cstack');

  const diagnostics = await waitFor(() => {
    const values = vscode.languages.getDiagnostics(uri);
    return values.length > 0 ? values : undefined;
  });

  assert.ok(
    diagnostics.some((diagnostic) => diagnostic.message.includes('unknown local field `ownerId`')),
    'expected relation diagnostic to be published',
  );
  assert.ok(
    diagnostics.some((diagnostic) => document.getText(diagnostic.range) === 'ownerId'),
    'expected diagnostic range to target ownerId precisely',
  );
}

module.exports = { run };
