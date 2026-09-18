# 開発

mophila そのものを直すときの手順。使い方は README。

## 作る・確認する

リポジトリの一番上で `make`。個別に回すなら次のとおり。

| コマンド | すること |
|---|---|
| `make check` | `cargo test` と、下の 2 つ |
| `make docs` | ドキュメント一式。`site/src/data.json`、`llms.txt`、`llms-full.txt`、`site/dist` がこれだけで揃う |
| `make dev` | ドキュメントページをその場で見る |
| `make media` | サンプルの動画も作り直す (GPU が要る。時間がかかる) |
| `make install-skill` | 書き方の skill を `~/.claude/skills/mophila` に複製する (`scripts/install-skill.sh --uninstall` で外す) |
| `make bump` | バージョンを上げて、コミットして、タグを打つ (一番下の桁。`make bump-minor` / `make bump-major` も) |

```
python3 scripts/check_examples.py   # examples が動き、error.moph が書いてある種別で止まる
python3 scripts/check_attrs.py      # 宣言した図形の属性が、本当に絵に効く
```

## ドキュメントの作られ方

ドキュメントページは `site/` (bun + SolidJS + soluid)。builtin の説明は `mophila doc` が出す JSON から、module の説明はソースの `##` コメントから `scripts/docgen.py` が作る。`site/public/llms.txt` と `llms-full.txt` も build が作るので、直接編集しない。

`docs/skills/mophila/` には型や関数の**名前を書かない** (写すと古くなる)。名前は `mophila doc` とドキュメントページで引く。

## 標準ライブラリ

`src/stdlib/`。`import math` は Rust、他は `.moph` を `include_str!` で実行ファイルに埋め込んでいる (`src/stdlib/mod.rs` の `SCRIPTS`)。別に配るファイルは無い。

## バージョン

`Cargo.toml` の version が正。`Cargo.lock` と `editors/vscode/package.json` も同じ値に揃える。

```
make bump                         # 0.1.5 -> 0.1.6
make bump-minor                   # 0.1.5 -> 0.2.0
make bump-major                   # 0.1.5 -> 1.0.0
scripts/bump-version.sh 0.3.0     # 版を直に書く
scripts/bump-version.sh --show    # いまの版
```

書き換えて、コミットして、`vX.Y.Z` のタグを打つところまでやる。push はしない。

ほかに変更があるときは何もしない (版だけのコミットにするため)。3 つのファイルが今の版で揃っていること、そのタグがまだ無いことも先に確かめ、1 つでも駄目なら何も書かずに止まる。
