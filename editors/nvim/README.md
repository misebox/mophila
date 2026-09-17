# mophila for Neovim

`.moph` の構文ハイライトと、`mophila lsp` による診断・補完・ホバー・定義へ移動など。

色は `syntax/mophila.vim` が付ける。`mophila lsp` は semantic tokens を返さないので、LSP だけでは色が付かない。

## 入れる

どれか 1 つ。`<repo>` はこのリポジトリの場所。

**pack に置く** (プラグインマネージャを使わない場合)

```
mkdir -p ~/.config/nvim/pack/mophila/start
ln -s <repo>/editors/nvim ~/.config/nvim/pack/mophila/start/mophila
```

**lazy.nvim**

```lua
{ dir = "<repo>/editors/nvim", ft = "mophila" }
```

**ファイルを写す**

```
cp -r <repo>/editors/nvim/{syntax,ftdetect,ftplugin} ~/.config/nvim/
```

`*.moph` を開くと filetype が `mophila` になり、色が付く。`:set filetype?` で確かめられる。

## LSP を繋ぐ

`mophila` を PATH に入れてから (`cargo install --path <repo>`)、`init.lua` に書く。

```lua
vim.api.nvim_create_autocmd("FileType", {
  pattern = "mophila",
  callback = function(args)
    vim.lsp.start({
      name = "mophila",
      cmd = { "mophila", "lsp" },
      root_dir = vim.fs.root(args.buf, { "mophila.yaml", ".git" }),
    })
  end,
})
```

できることは [../vscode/README.md](../vscode/README.md) の「できること」と同じ。診断は入力中 (構文) と保存時 (実行)。

## 中身

| ファイル | すること |
|---|---|
| `ftdetect/mophila.vim` | `*.moph` を filetype `mophila` にする |
| `syntax/mophila.vim` | 色を付ける。語の一覧は `src/lang/lexer.rs` と `mophila doc` の型名に合わせる |
| `ftplugin/mophila.vim` | `commentstring` (`# %s`)、インデント幅 2、`gf` 用の `suffixesadd` |

色の対応は `Keyword` `Type` `Number` `String` `Constant` `Function` `Comment` などの標準グループに link してあるので、配色テーマにそのまま従う。`#e04040` のような色リテラルは `Constant`、`## ` で始まるドキュメントコメントは `SpecialComment`、その中の `@param` などは `Identifier`。

`Line(from = ...)` の `from` は属性なので色を変えない。`import { a } from mod` の `from` だけキーワードとして扱う。
