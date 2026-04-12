const path = require("path");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

function activate(context) {
  // 워크스페이스 루트에서 빌드된 바이너리를 직접 참조
  const workspaceRoot = require("vscode").workspace.workspaceFolders?.[0]?.uri.fsPath;
  let command = "kyte";
  if (workspaceRoot) {
    const debugPath = path.join(workspaceRoot, "target", "debug", "kyte.exe");
    const releasePath = path.join(workspaceRoot, "target", "release", "kyte.exe");
    const fs = require("fs");
    if (fs.existsSync(debugPath)) {
      command = debugPath;
    } else if (fs.existsSync(releasePath)) {
      command = releasePath;
    }
  }

  const serverOptions = {
    command,
    args: ["lsp"],
    transport: TransportKind.stdio,
  };

  const clientOptions = {
    documentSelector: [{ scheme: "file", language: "kyte" }],
  };

  client = new LanguageClient(
    "kyte",
    "Kyte Language Server",
    serverOptions,
    clientOptions
  );

  client.start();
}

function deactivate() {
  if (client) return client.stop();
}

module.exports = { activate, deactivate };
