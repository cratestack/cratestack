const path = require('node:path');
const os = require('node:os');
const fs = require('node:fs');
const { runTests } = require('@vscode/test-electron');

async function main() {
  const extensionDevelopmentPath = path.resolve(__dirname, '..');
  const extensionTestsPath = path.resolve(__dirname, 'vscode-suite.js');
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'cool-vscode-'));
  const userDataDir = path.join(tempRoot, 'u');
  const extensionsDir = path.join(tempRoot, 'e');

  await runTests({
    extensionDevelopmentPath,
    extensionTestsPath,
    launchArgs: [
      path.resolve(__dirname, 'fixtures'),
      `--user-data-dir=${userDataDir}`,
      `--extensions-dir=${extensionsDir}`
    ],
  });
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
