const path = require("path");
const fs = require("fs");
const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;
let statusBar;

function findKyteBinary(workspaceRoot) {
  const configured = vscode.workspace.getConfiguration("kyte").get("serverPath");
  if (configured && configured.trim()) return configured.trim();

  if (workspaceRoot) {
    const candidates = [
      path.join(workspaceRoot, "target", "debug",   "kyte.exe"),
      path.join(workspaceRoot, "target", "debug",   "kyte"),
      path.join(workspaceRoot, "target", "release", "kyte.exe"),
      path.join(workspaceRoot, "target", "release", "kyte"),
    ];
    for (const p of candidates) {
      if (fs.existsSync(p)) return p;
    }
  }

  return "kyte";
}

function startClient(context) {
  const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  const command = findKyteBinary(workspaceRoot);

  const serverOptions = {
    command,
    args: ["lsp"],
    transport: TransportKind.stdio,
  };

  const clientOptions = {
    documentSelector: [{ scheme: "file", language: "kyte" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.ky"),
    },
    traceOutputChannel: vscode.window.createOutputChannel("Kyte LSP Trace"),
  };

  client = new LanguageClient("kyte", "Kyte Language Server", serverOptions, clientOptions);

  statusBar.text = "$(loading~spin) Kyte";
  statusBar.tooltip = "Kyte Language Server — starting…";

  client.start().then(() => {
    statusBar.text = "$(check) Kyte";
    statusBar.tooltip = `Kyte Language Server — running\n${command}`;
  }).catch((err) => {
    statusBar.text = "$(error) Kyte";
    statusBar.tooltip = `Kyte Language Server — failed to start`;
    vscode.window.showErrorMessage(
      `Kyte LSP failed to start: ${err.message}`,
      "Open Settings",
      "Retry"
    ).then((sel) => {
      if (sel === "Open Settings") {
        vscode.commands.executeCommand("workbench.action.openSettings", "kyte.serverPath");
      } else if (sel === "Retry") {
        vscode.commands.executeCommand("kyte.restartServer");
      }
    });
  });
}

function activate(context) {
  statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 10);
  statusBar.text = "$(loading~spin) Kyte";
  statusBar.tooltip = "Kyte Language Server";
  statusBar.command = "kyte.restartServer";
  statusBar.show();
  context.subscriptions.push(statusBar);

  const restartCmd = vscode.commands.registerCommand("kyte.restartServer", async () => {
    if (client) {
      statusBar.text = "$(loading~spin) Kyte";
      statusBar.tooltip = "Kyte Language Server — restarting…";
      await client.stop();
      client = undefined;
    }
    startClient(context);
  });
  context.subscriptions.push(restartCmd);

  startClient(context);
}

function deactivate() {
  statusBar?.dispose();
  if (client) return client.stop();
}

module.exports = { activate, deactivate };
