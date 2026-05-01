const vscode = require('vscode');
const {
  LanguageClient,
  TransportKind,
} = require('vscode-languageclient/node');
const { resolveServerCommand } = require('./server-path');

let client;

function activate(context) {
  const config = vscode.workspace.getConfiguration('cratestack');
  const command = resolveServerCommand(
    context.extensionPath,
    config.get('lsp.path', 'cratestack-lsp'),
  );
  const args = config.get('lsp.args', []);

  const serverOptions = {
    command,
    args,
    transport: TransportKind.stdio,
  };

  const clientOptions = {
    documentSelector: [{ scheme: 'file', language: 'cstack' }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher('**/*.cstack'),
    },
  };

  client = new LanguageClient(
    'cratestack-lsp',
    'CrateStack Language Server',
    serverOptions,
    clientOptions,
  );

  context.subscriptions.push(client.start());
}
function deactivate() {
  if (!client) {
    return undefined;
  }

  return client.stop();
}

module.exports = {
  activate,
  deactivate,
};
