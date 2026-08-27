const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");
const { resolveServerCommand } = require("./server-path");

let client;

async function activate(context) {
  const config = vscode.workspace.getConfiguration("cratestack");
  const command = resolveServerCommand(
    context.extensionPath,
    config.get("lsp.path", "cratestack-lsp"),
  );
  const args = config.get("lsp.args", []);

  const serverOptions = {
    command,
    args,
    transport: TransportKind.stdio,
  };

  const clientOptions = {
    documentSelector: [{ scheme: "file", language: "cstack" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.cstack"),
    },
  };

  client = new LanguageClient(
    "cratestack-lsp",
    "CrateStack Language Server",
    serverOptions,
    clientOptions,
  );

  // `start()` returns `Promise<void>` in vscode-languageclient 7+, not the
  // `Disposable` it returned in 6.x. Pushing its return value into
  // `subscriptions` (as this did) registered a Promise that VS Code would
  // later call `.dispose()` on, and left a rejected `start()` unhandled — so
  // a missing or unexecutable server binary failed silently, with no
  // diagnostics and nothing in the UI to explain why. The client itself is
  // the Disposable.
  context.subscriptions.push(client);

  try {
    await client.start();
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    // Name the *resolved* command: the difference between a stale
    // `cratestack.lsp.path` and a missing bundled binary is invisible
    // otherwise, and it is the first thing to check.
    vscode.window.showErrorMessage(
      `CrateStack: could not start the language server (${command}). ${detail}`,
    );
  }
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
