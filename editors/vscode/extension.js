const vscode = require("vscode");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

// GUI から起動した VS Code は PATH にシェルの設定を含まないことがあるので、
// 名前で見つからなければ cargo の既定の置き場も見る
function resolveServer() {
  const configured = vscode.workspace.getConfiguration("mophila").get("serverPath");
  if (configured) {
    return configured;
  }
  const candidates = [path.join(os.homedir(), ".cargo", "bin", "mophila"), "/usr/local/bin/mophila", "/opt/homebrew/bin/mophila"];
  for (const p of candidates) {
    if (fs.existsSync(p)) {
      return p;
    }
  }
  return "mophila";
}

async function activate(context) {
  const command = resolveServer();
  const serverOptions = { command, args: ["lsp"], transport: TransportKind.stdio };
  const clientOptions = { documentSelector: [{ scheme: "file", language: "mophila" }] };
  client = new LanguageClient("mophila", "mophila language server", serverOptions, clientOptions);
  context.subscriptions.push(client);
  try {
    await client.start();
  } catch (e) {
    // 黙って死なせない。補完もホバーも出ない理由がわかるようにする
    vscode.window.showErrorMessage(
      `mophila: 言語サーバ "${command}" を起動できませんでした。設定 mophila.serverPath に mophila コマンドの絶対パスを入れてください (${e.message})`,
    );
  }
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
