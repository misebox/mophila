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
| `mophila render a.moph -o a.mp4 --size fhd --fps 30` | 動画を書く。形式は拡張子で決まる (下の表)。`--trim 00:15..00:30` で区間だけ (`00:15` は 15 秒以降、`..01:30` は 1 分 30 秒まで) |
| `mophila render a.moph -o tall.mp4 --size 1080x1920 --crop height --align left` | 横長の絵から縦長を切り出す。`--crop` は切り取る大きさ (`height` / `width` / `0.25,1`)、`--align` は余りのどこに寄せるか (`left` `center` …、`30%,50%` も可)、`--pad '#000000'` は帯の色 |
| `mophila render a.moph -o a.mp4 --gpu-budget 2GB` | GPU のメモリが足りているか見ながら書く。書き終わりに一番要ったコマと内訳が出る。予算を超えたコマは警告 |
| `mophila preview a.moph` | ウィンドウで実時間再生。重い場面では fps が落ちる。Space で一時停止、← → で移動、`[` `]` で速度、? で操作一覧、q か ⌘W で終了 |
| `mophila run a.moph` | 描画せずに実行する (`log` の確認) |
| `mophila timeline a.moph` | 何が、いつ、どう変わるかをテキストで出す |
| `mophila sheet a.moph -o sheet.png --times 1s,5s,10s` | 指定した時刻のコマを 1 枚に並べる |
| `mophila fonts` | この機械で使えるフォント名を並べる (`TextArea` の `font` に書ける名前) |
| `mophila bundle a.moph -o a` | スクリプトを埋め込んだ実行ファイルを作る |
| `mophila lsp` | Language Server (エディタから起動する) |

### 出力の形式

`-o` の拡張子で決まる。`--codec` と `--pix-fmt` を書けばそれが勝つ。

| 拡張子 | コーデック | 音声・字幕 | 備考 |
|---|---|---|---|
| `.mp4` `.mov` `.mkv` | h264 / yuv420p | 入る | 配る用 |
| `.webm` | vp9 / yuv420p | 入る | 音声は opus |
| `.gif` | gif | 入らない | 256 色。使う色を決めてから割り当てる |
| `.apng` | apng | 入らない | 色を落とさない代わりに大きい |
| `.png` `.jpg` `.webp` `.tiff` | それぞれ | 入らない | `--at <時刻>` が要る。1 枚だけ書く |

`.webp` は ffmpeg が libwebp 付きで作られている必要がある。

### 並列に組む

`--jobs N` で、フレームの組み立てを N 本のスレッドに分ける。1 フレームの状態は前のフレームに依存しないので、スレッドごとにスクリプトを実行して別々に組める。

既定は 1 本。**組み立てが GPU より重い絵でだけ速くなる**。軽い絵ではスレッドを立てる分だけ遅くなる。`MOPHILA_TIMING=1` を付けて `eval` と `scene` の合計が `render` + `readback` を超えていれば、増やす価値がある。

Shader の塗りは描画命令を組む時点で GPU を使うので、その絵は常に 1 本で組む。

## プロジェクト設定

`mophila.yaml` を置くと、実行したディレクトリから自動で読む。台本の代わりにそのフォルダか `mophila.yaml` を渡しても読む (`mophila render project/`)。`-f` は設定と台本が別の場所にあるとき。

```yaml
entry: scenes/main.moph      # スクリプトを省略したときに使う
root: .                      # @/ と @. が指す場所
aliases:
  parts: src/parts           # @parts. が指す場所
config:
  width: 16                  # import config で読める値
  title: "mophila"
  debug: false
```

設定の中に書いたパスは設定ファイルからの相対。コマンドラインのパスは、いま居るディレクトリから。`config` の値は `MOPHILA_WIDTH=32` か `--set width=32` で上書きできる。動く例は `examples/project/`。

## エディタ

VS Code 拡張は `editors/vscode/README.md`、Neovim の設定は `editors/nvim/README.md`。構文ハイライト、診断、補完、定義へ移動など。

## 文書

- <https://misebox.github.io/mophila/> — 仕様、型と module のリファレンス、動く例
- `docs/mophila-spec.md` — 言語仕様
- `docs/mophila.gbnf` — 文法 (GBNF)
- `docs/skills/mophila/` — 書き方 (`SKILL.md` が入口)。`scripts/install-skill.sh` で Claude Code の skills に入る
- `examples/syntax/` — 構文ごとの短い例
- `examples/gallery/` — 動画の例。`examples/gallery/mophila_intro/main.moph` がこの言語の紹介動画
- `mophila doc` — その実行ファイルが持っている型・属性・メソッドの一覧 (JSON)

## ライセンス

MIT。LICENSE を参照。
