# 開発

mophila そのものを直すときの手順。使い方は README。

## 作る・確認する

リポジトリの一番上で `bun run all`。入口は `package.json` の scripts だけで、中身は cargo / scripts / site に任せている。
個別に回すなら次のとおり。後ろに書いたものは、そのまま中のコマンドに渡る。

| コマンド | すること |
|---|---|
| `bun run check` | `cargo build` `cargo test` と、下の 2 つ |
| `bun run docs` | ドキュメント一式。`site/src/data.json`、`llms.txt`、`llms-full.txt`、`site/dist` がこれだけで揃う |
| `bun run dev` | ドキュメントページをその場で見る |
| `bun run media` | サンプルの動画も作り直す (GPU が要る。時間がかかる) |
| `bun run install-skill` | 書き方の skill を `~/.claude/skills/mophila` に複製する (`scripts/install-skill.sh --uninstall` で外す) |
| `bun run install-vscode` | VS Code の拡張を組み立てて入れる (`scripts/install-vscode.sh --uninstall` で外す) |
| `bun run bump 桁` | バージョンを上げて、コミットして、タグを打つ |

```
python3 scripts/check_examples.py   # examples が動き、error.moph が書いてある種別で止まる
python3 scripts/check_attrs.py      # 宣言した図形の属性が、本当に絵に効く
```

## ドキュメントの作られ方

ドキュメントページは `site/` (bun + SolidJS + soluid)。builtin の説明は `mophila doc` が出す JSON から、module の説明はソースの `##` コメントから `scripts/docgen.py` が作る。`site/public/llms.txt` と `llms-full.txt` も build が作るので、直接編集しない。

VS Code の色付けが知っている型名も、`scripts/docgen.py` が `mophila doc` の表からその 1 行を書き換える。

`docs/skills/mophila/` には型や関数の**名前を書かない** (写すと古くなる)。名前は `mophila doc` とドキュメントページで引く。

## 遅いところを見つける

`MOPHILA_TIMING=1` を付けると、段階ごとの合計と平均、**いちばん遅かったコマとその時刻**を終わりに出す。

```
MOPHILA_TIMING=1 mophila render a.moph -o a.mp4     # eval / scene / render / readback / encode
MOPHILA_TIMING=1 mophila preview a.moph             # eval / scene / render / present と、出すのが間に合わなかったコマ
```

preview は実時間で出すので、描くのが間に合わなければそのぶんゆっくりになる (コマは飛ばさない)。
render は時間で進まないので、同じ場面が重くても出力のコマは変わらない。
preview で詰まったものが出力の不具合かどうかは、両方に付けて worst の時刻を見比べる。

`MOPHILA_WGSL=1` を付けると、Shader の `color` から組み立てた WGSL をそのまま stderr に出す (`.moph` からの変換を確かめる用)。

## 標準ライブラリ

`src/stdlib/`。`import math` `import space3d` `import bignum` は Rust、他は `.moph` を `include_str!` で実行ファイルに埋め込んでいる (`src/stdlib/mod.rs` の `FILES`)。別に配るファイルは無い。

## ロゴとアイコン

`site/public/logo.png` (横長) と `site/public/icon.png` (正方形) が元。README とサイトの favicon はここを見る。
VS Code の拡張は自分のフォルダに置く決まりなので、`editors/vscode/icon.png` に 128px に縮めた同じ絵を置いている。

## バージョン

`Cargo.toml` の version が正。`Cargo.lock` と `editors/vscode/package.json` も同じ値に揃える。

```
bun run bump patch                # 0.1.5 -> 0.1.6
bun run bump minor                # 0.1.5 -> 0.2.0
bun run bump major                # 0.1.5 -> 1.0.0
bun run bump 0.3.0                # 版を直に書く
bun run bump                      # いまの版を出すだけ (桁を書かなければ上げない)
```

書き換えて、コミットして、`vX.Y.Z` のタグを打つところまでやる。push はしない。

ほかに変更があるときは何もしない (版だけのコミットにするため)。3 つのファイルが今の版で揃っていること、そのタグがまだ無いことも先に確かめ、1 つでも駄目なら何も書かずに止まる。
