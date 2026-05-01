const fs = require('fs');
const path = require('path');

function resolveServerCommand(extensionPath, configuredPath) {
  if (configuredPath && configuredPath !== 'cratestack-lsp') {
    return configuredPath;
  }

  const bundled = bundledServerPath(extensionPath);
  if (bundled) {
    return bundled;
  }

  return configuredPath || 'cratestack-lsp';
}

function bundledServerPath(extensionPath, platform = process.platform) {
  const executable = platform === 'win32' ? 'cratestack-lsp.exe' : 'cratestack-lsp';
  const candidate = path.join(extensionPath, 'server', platform, executable);
  if (fs.existsSync(candidate)) {
    return candidate;
  }
  return undefined;
}

module.exports = {
  bundledServerPath,
  resolveServerCommand,
};
