#!/usr/bin/env bash
# editors/vscode の拡張を組み立てて VS Code に入れる。
#
#   scripts/install-vscode.sh              # 入れる。既に入っていれば入れ直す
#   scripts/install-vscode.sh --uninstall  # 外す
#
# code コマンドが要る。無ければ VS Code のコマンドパレットで
# "Shell Command: Install 'code' command in PATH" を実行する。
# 拡張は LSP に mophila の実行ファイルを使うので、別に cargo install --path . をしておく
set -euo pipefail

ID=mophila.mophila
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="$ROOT/editors/vscode"
mode=install

for arg in "$@"; do
  case "$arg" in
    --uninstall) mode=uninstall ;;
    -h|--help) sed -n '2,9p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

# PATH に無ければ、macOS の既定の置き場も見る
code=code
if ! command -v code >/dev/null 2>&1; then
  bundled="/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
  if [ ! -x "$bundled" ]; then
    echo "code コマンドが見つからない。VS Code のコマンドパレットで \"Shell Command: Install 'code' command in PATH\" を実行する" >&2
    exit 1
  fi
  code="$bundled"
fi

if [ "$mode" = uninstall ]; then
  "$code" --uninstall-extension "$ID"
  exit 0
fi

if ! command -v npm >/dev/null 2>&1; then
  echo "npm が見つからない。Node.js を入れる" >&2
  exit 1
fi

cd "$DIR"
# 組み立てに @vscode/vsce が要る。1 度入れたら次からは飛ばす
[ -d node_modules ] || npm install
version="$(sed -n 's/^[[:space:]]*"version": "\(.*\)",$/\1/p' package.json | head -1)"
npm run package
"$code" --install-extension "mophila-$version.vsix" --force

echo "installed $ID $version"
if ! command -v mophila >/dev/null 2>&1; then
  echo "mophila が PATH に無い。診断や補完を使うには cargo install --path . を実行する"
fi
echo "VS Code を開き直すと効く"
