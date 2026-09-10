# mophila for VS Code

`.moph` の構文ハイライトと、`mophila lsp` による診断・補完・ホバー・定義へ移動など。

## 準備

1. `mophila` を入れる (リポジトリで `cargo install --path .`)

拡張は `mophila` を PATH から探し、見つからなければ `~/.cargo/bin/mophila` なども見る。それでも起動できなければエラーを出すので、設定 `mophila.serverPath` に絶対パスを書く。

> Finder や Dock から起動した VS Code は、シェルの PATH を引き継がない。`which mophila` が通るのに補完が出ないときは、これが原因のことが多い。
2. この拡張を入れる:

```
cd editors/vscode
npm install
npm run package            # mophila-0.1.0.vsix ができる
code --install-extension mophila-0.1.0.vsix
```

開発中は、このフォルダを VS Code で開いて F5 (Extension Development Host) でも動く。

## できること

- 診断: 構文エラーは入力中に、型エラーなど実行時のものは保存時に (スクリプトを描画せずに実行する)。import 先で起きたエラーはそのファイルに出る
- 補完: キーワード、型名、`.` の後の属性とメソッド、`module.` の後は import 先の export、`:` の後の列挙値、ファイル内の識別子
- ホバー: 型名の上で属性の一覧、import した名前の上でその署名とドキュメントコメント、トップレベルの変数の上で保存時に実行した値と型、メソッド名の説明
- 定義へ移動: `let` / `func` / `type` / `struct` / `record` の定義。import した名前と `module.name` は import 先のファイルへ。標準ライブラリは本体に埋め込んであるので、`~/.cache/mophila/stdlib/` に書き出したものを開く
- 参照の検索、名前の変更: ファイル内の同じ識別子 (字句で見るので、別スコープの同名も含む)
- アウトライン: `let` / `func` / `type` / `struct` / `record` の一覧
- クイックフィックス: `"math" is not defined; add "import math"` の診断に、先頭へ import を挿入する修正

## Neovim

`init.lua`:

```lua
vim.filetype.add({ extension = { moph = "mophila" } })
vim.api.nvim_create_autocmd("FileType", {
  pattern = "mophila",
  callback = function()
    vim.lsp.start({ name = "mophila", cmd = { "mophila", "lsp" }, root_dir = vim.fs.root(0, { ".git" }) })
  end,
})
```
