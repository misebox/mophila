# mophila

動画をコードで書く言語。`.moph` のスクリプトを解釈し、GPU (Vello) で描いて、ffmpeg で動画か画像にする。

![育つ木の前を人が歩く](docs/readme.gif)

上の 8 秒は `examples/gallery/passerby.moph`。木 (`fractal.moph`) と歩く人 (`walker.moph`) を import して置いただけ。

```
let v = View(box = (16, 9))
let c = Circle(position = Pos(8, 4.5), radius = 1, fill = #ffb454)
v.place(c)

let grow = context c as o {
  motion (t) {
    0s: o.radius = 1
    3s: o.radius = 3 :ease
  }
}
let track = Timeline()
track.place(grow, at = 1s)
v.addTrack(track)
output v
```

```
mophila render circle.moph -o circle.mp4 --size fhd --fps 30
```

## 特徴

- 座標は箱 (View) の中の値で書く。ピクセルの大きさは出力のときに決める
- 変化は時刻と値の表 (`motion`) で書く。間は補間され、`duration` で全体が伸び縮みする。`0..1:` の行は毎フレーム式を評価する
- 作った View は部品。別の View に、別の大きさで置ける。ファイルも `import` で部品になる
- どの時刻の絵も、その時刻だけから決まる。途中へ飛んでも同じ絵になる
- 音声と字幕も同じ Timeline に置く。`Shader` で位置と時刻から色を決める塗りが書ける

## 必要なもの

- Rust (edition 2024)
- ffmpeg と ffprobe (動画の出力、音声の読み込み)
- GPU (wgpu が使えるもの)

## インストール

```
cargo install --path .
```

## コマンド

| コマンド | 内容 |
|---|---|
| `mophila render a.moph -o a.mp4 --size fhd --fps 30` | 動画を出す。`-o a.png --at 2.5s` で画像。`--trim 00:15..00:30` で区間だけ (`00:15` は 15 秒以降、`..01:30` は 1 分 30 秒まで) |
| `mophila preview a.moph` | ウィンドウで実時間再生。Space で一時停止、← → で移動 |
| `mophila run a.moph` | 描画せずに実行する (`log` の確認) |
| `mophila timeline a.moph` | 何が、いつ、どう変わるかをテキストで出す |
| `mophila sheet a.moph -o sheet.png --times 1s,5s,10s` | 指定した時刻のコマを 1 枚に並べる |
| `mophila bundle a.moph -o a` | スクリプトを埋め込んだ実行ファイルを作る |
| `mophila lsp` | Language Server (エディタから起動する) |

## エディタ

VS Code 拡張と Neovim の設定は `editors/vscode/README.md`。構文ハイライト、診断、補完、定義へ移動など。

## 文書

- ドキュメントページ (`site/`、bun + SolidJS + soluid)。サンプルの動画、組み込みとライブラリの説明、仕様の写し。`bun run docs` でその場で見る (要 bun)。元データ `site/src/data.json` と動画は `bun run docs:gen` (= `scripts/docgen.py --media`) で作り直す。ライブラリの説明はソースの `##` コメントから
- `docs/mophila-spec.md` — 言語仕様
- `docs/mophila.gbnf` — 文法 (GBNF)
- `docs/skills/mophila/SKILL.md` — 見やすい動画にするための書き方 (LLM に渡す)
- `examples/syntax/` — 構文ごとの短い例
- `examples/gallery/` — 動画の例。`examples/gallery/mophila_intro/main.moph` がこの言語の紹介動画
- `src/stdlib/` — 標準ライブラリ。`import math` (Rust) と、本体に埋め込んだ .moph の `color` `shape` `layout` `animation` `fractal`

## 確認

```
cargo test
python3 scripts/check_examples.py   # examples が動き、error.moph が書いてある種別で止まる
python3 scripts/check_attrs.py      # 宣言した図形の属性が、本当に絵に効く
```

## ライセンス

MIT。LICENSE を参照。
