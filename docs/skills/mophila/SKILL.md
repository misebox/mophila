---
name: mophila
description: Write or debug mophila (.moph) motion graphics scripts in this repo. Syntax summary, semantics that differ from other languages, patterns for slides/arrows/groups, and how to check a script.
---

# mophila を書く

実装はインタプリタ。`mophila render` はソースを毎回解釈する。

正は `docs/mophila-spec.md` (意味) と `examples/*.moph` (構文の見本)。形式文法は `docs/mophila.gbnf`。迷ったらそれらを読む。

## 確認コマンド

```
cargo run -- run a.moph                              # 描画せず実行。log と型エラーの確認
cargo run -- render a.moph -o out.png --at 2.5s --size 480x270   # 1 フレーム
cargo run -- render a.moph -o out.mp4 --size 360p    # 動画 (fps 既定 10)。-o 省略時は output.mp4 (--at 付きなら output.png)
cargo run -- render a.moph -o part.mp4 --trim 00:15..00:30   # 区間だけ (00:15 は 15 秒以降、..01:30 は最初から)
cargo run -- preview a.moph [--loop] [--at 1s]       # ウィンドウで再生 (--at はその時刻で一時停止して開く)。
cargo run -- timeline a.moph [--filter k=v]           # 変化の一覧をテキストで
cargo run -- lsp                                      # Language Server (VS Code 拡張は editors/vscode/)
cargo run -- sheet a.moph -o sheet.png --times 3s,12s # 場面の格子画像
終わると最後の場面で止まる。Space で一時停止、← → / h l で 10 秒 (停止中 1 秒)、Shift で 10%
```

release ビルドは作らない。計測は小さいサイズと短い尺で。

## 最小の形

```
import math
let v = new View { box: vector!(16, 9) }          # 座標系。ピクセルは持たない
let c = new Circle { position: apos!(:center, 8, 4.5), radius: 1, fill: #e04040 }
v.place(c)
v.addTrack(context c as o {
  motion (t) {
    0s: o.radius = 1
    2s: o.radius = 2 :ease
  }
})
output v
```

## 他の言語と違うところ

- `(1, 2)` は Tuple。座標の型は Vector (`vector!(1, 2)`)、位置は AnchoredPosition (`apos!(:center, 1, 2)`)。ただし型の決まった場所 (属性など) では `box: (4, 3)` `position: (:center, 1, 2)` と素の Tuple で書けて、その型に変換される。図形の位置は `position` 属性
- 列挙値は `:center` `:topLeft` `:left` のように `:name`
- `1/3` は演算。`10s` `500ms` `1m23s` `01:23` は Duration。`25%` は Number (0.25) であって Duration ではない
- 文字列に埋め込みは無い。`"x = {x}".format(x)` (位置で置き換え)
- コメントは `# ` (空白必須)。`#e04040` は色
- `context obj as o { ... }` は式で、最後の式が値。`if` も式
- `new T { a: 1 }` は属性を型で検査する。無い属性は `NameError.UndefinedAttribute`
- 図形: Circle(position, radius) / Rect(position, w, h, radius) / Line(from, to) / Polygon(points) / TextArea(text, position, w, fontSize, font, align)。共通: fill, stroke, strokeWidth, opacity
- `math.sin` などは `import math` が要る。組み込みは `log` `type_of` と specific tuple だけ
- `import .other` で同じ場所の `other.moph` を読み、`other` に束縛する (`as m` で別名)。`import { a, b } from .other` で名前を直接持ち込む。見えるのは `export let` / `export func` した名前と `other.output` (output した View) だけ。同じファイルは 1 度しか実行されない (実体は共有)。`import ..parent` は文法上通るが、ディレクトリをまたぐ設計は避ける
- 文字列は `+` で連結。`\"` `\n` のエスケープあり

## 時間

- `motion (t) { 時刻: o.attr = 式 }` が Timeline。時刻は Duration か 0..1 の実数 (`50%` 可)。1 つの motion で混ぜない
- 実数時刻の Timeline は `tl.duration = 8s` が必須。`duration` を変えると全体が比例して伸縮する
- `0..1: o.position = apos!(..., math.cos(math.TAU * t), ...)` のように範囲を書くと、その区間は補間せず毎フレーム式を評価する。円運動・振動はこれで書く。行を刻んで近似しない
- 行末の修飾子はその行に入る区間に効く: `:ease` (加減速) `:ease_in` `:ease_out` `:linear`。`:fade` は最初の区間なら 0→1、最後の区間なら 1→0。途中は `o.opacity` の値で書く
- `tl.reverse()` で逆再生
- 入れ物: `let track = new Timeline {}` に `track.place(tl, at: 3s, fadeIn: 1s)`。`v.addTrack(track)`。Timeline は終わった後も最後の状態を保つ。始まる前は何もしない
- 後から place した Timeline が同じ属性を書けば勝つ
- 動画の長さは View に置いた Timeline の終わり

## よく使う型

スライドの出し入れ:

```
func show(objs, at, dur) {
  for o in objs {
    let tl = context o as x { motion (t) { 0s: x.opacity = 0 \  0.4s: x.opacity = 1 } }
    track.place(tl, at: at)
    let out = context o as x { motion (t) { 0s: x.opacity = 1 \  0.4s: x.opacity = 0 } }
    track.place(out, at: at + dur)
  }
}
```
(上の `\` は改行の意味。実際は行を分ける)

矢印: `Line` と 3 点の `Polygon`。角度は `math.atan2` が無いので、向きを `(dx, dy)` から計算する。

View の入れ子: `v.place(sub, at: apos!(:topLeft, x, y), w: 7)` で別の View を比率を保って置く。`track.place(sub, at: 0s)` でその View の Timeline を動かす。`sub.opacity` で全体をフェードできる。samples/grid.moph が例。

部品が互いに依存して動くもの (多関節、木): 範囲行 `0..1: child.from = parent.to` は毎フレーム評価されるので、親の属性を読む行を書けば子が追従する。Timeline を置いた順に当たるので、親の行を先に置く。samples/fractal.moph が例 (枝が親の先端を読み、再帰で 2^n 本作る。Vector は `+` `-` `* Number` が使える)。多関節の人物は samples/walker.moph (腰 → 腿 → 脛 → 足を親の先端で順につなぎ、角度は歩行周期の sin。奥 → 胴 → 手前の順に Timeline を置く)。`import { make, speed, floor } from .walker` の `make(near, far, long)` で「その場で long の間歩く人」の View (箱 6 x 6) を作れる。置く側が position.x を動かし、幅 w のとき 1 秒に `speed * w / 6` 進めると足が滑らない。samples/crowd.moph が奥行きを付けて並べた例。

ピクセル単位の絵 (グラデーション、模様、フラクタル): 図形の `fill` に `new Shader { color: func (x, y, t) { ... Color } }` を入れる。x, y は箱の座標、t は秒。関数は GPU で全ピクセル分走るので数値の計算だけ (文字列・図形・Dict は不可、再帰不可)。外側の List (Number か Color だけ) と関数は使える。時間で変える値は t から計算するか、`args: [..]` を motion の行で変える。`samples: 4` で 2x2 のアンチエイリアス (計算は 4 倍)。複素数は Vector で書ける (`.x` `.y`、`+`、Number との `*`)。

標準ライブラリは名前で import する。`transition` (`fade_in(o, duration)` / `fade_out` / `fade_to` / `slide_in(o, dx, dy)` / `slide_out` / `move_by` / `show(track, objs, at, end)` が Timeline を返すので `track.place(tl, at:)` に置く)、`color` (`mix` `lighten` `darken` `alpha` `hsl` `gray`)、`shape` (Polygon の points: `regular_polygon` `star` `arrow`)、`layout` (`grid` `cell` `along` `fit`)。自分で opacity の motion を書く前にこれらを使う。エスケープタイム系フラクタル (Mandelbrot、Julia、Burning Ship、Multibrot) は標準ライブラリ fractal を使う: `import { escape_time, mandelbrot, julia, perturbation, plain, smooth } from fractal` して `escape_time(formula:, precision:, coloring:, center:, span:, zoom:, max_iter:, samples:)` が Shader を返す。反復式は Dict (seed / step / start / delta / degree)、色付けは `func (mu, z) -> Color` で、どちらも自作できる。深く寄るなら `perturbation` (基準軌道をスクリプト側の 64 bit で計算し GPU は差分だけ。10 兆倍あたりまで)。samples/mandelbrot.moph、julia.moph、burning_ship.moph が例。

音声: `import "bgm.m4a" as bgm` して `track.place(bgm, at: 0s, loop: true, volume: 0.6, fadeOut: 3s)` (samples/mophila_intro/main.moph)。`loop: true` は動画の終わりまで繰り返す。`duration:` で切る。preview でも鳴る。

字幕: `new Subtitle { text: s, duration: }` を `track.place(sub, at:)`。画面には描かれず、動画の字幕トラックになる (プレイヤーで表示)。preview と sheet では下に重ねて見える。字幕を TextArea で描かない。examples/media.moph が例。


## 見やすい動画にする (説明的な動画の tips)

情報を出す動画は「読める時間」を確保する。目安:

- 日本語の字幕は 4 文字/秒。30 文字なら 7〜8 秒。英語は 15 文字/秒
- コードや式は 2.5 文字/秒 (読むより「照合する」ので遅い)。`10s + 500ms → 10.5s` なら 6〜8 秒
- 4 文字程度のラベル (箱の名前など) は 1〜2 秒
- 文字数に空白は数えない。同時に出すもの (式とその説明など) は時間を足さず、長い方に合わせる
- どんなに短くても 1.5 秒より短くしない。フェードの 0.4 秒は読める時間に含めない
- 複数の項目は 1 つずつ出し、前の項目は消さない。最後の項目が出てから全体を見返す 3 秒を置く
- 1 画面に同時に出すのは 1 つの新情報だけ。図形が動く場面と、文章を読ませる場面を重ねない
- 場面の切り替えの前に 1 秒の間を置く

時間の決まり (フェード、ずらし、章の見出し、見返し、間、読む速さ) はファイルの先頭に変数でまとめ、部品はそれを参照する。テンポを変えるときはそこだけ触る:

```
let fade_time = 0.4s
let stagger = 0.08s
let chapter_time = 4s
let review = 3s
let pause = 1s
let read_rate = 0.25
let code_rate = 0.3
let min_dwell = 1.5
```

文字数から時間を計算する関数を作り、`at` を積み上げる:

```
func dwell(s) { math.max(1.5, s.replace(" ", "").len() * 0.25) * 1s }   # 字幕。空白は数えない
func dwell_code(s) { math.max(1.5, s.replace(" ", "").len() * 0.3) * 1s }  # コード
let clock = S_start
for line in lines {
  show([...], clock, ...)
  clock = clock + dwell_code(line)
}
```

## 説明する動画の作り方 (プレゼンのノウハウ)

コードを書く前に、場面ごとの表を書く。動画は「読ませる」より「見せる」もの。

| 場面 | 伝えたいこと (1 つ) | 見せるもの (1 つ) | 字幕 (1 文) | 長さ |
|---|---|---|---|---|

- **1 場面 1 メッセージ**。伝えたいことが 2 つあれば場面を分ける
- **見せてから言う**。図形の動きで因果を見せ、字幕はその補足。字幕だけの場面を作らない
- **位置を固定する**。見出しは上、本文は中央の決まった枠、字幕は下。場面が変わっても同じ種類の情報は同じ場所に出す
- **同じものは同じ場所に残す**。構文解析で作った木を評価の場面でもそのまま使う、のように。消して出し直すと別物に見える
- **順に出す**。一度に出すのは 1 つ。前のものは消さず、最後に全体を見返す時間を置く
- **切り替えの意味を揃える**。話題が変わるときはフェード、同じものが変化するときは移動。章の見出しは「話題が変わる」の印
- **強調は 1 色**。アクセント色は「今見るべきもの」にだけ使う
- **画面に同時に出す項目は 3 つまで**。表なら行を順に出す
- **文字は 1 行**。ナレーションが無いので字幕が全情報になる。長くなるなら場面を分ける
- **矩形の中に文字を置くなら余白**。文字幅は `fontSize × 文字数 × 0.6` (英数) / `× 1.0` (日本語) が目安。箱の幅を超えるなら fontSize を下げる
- **時間は文字量から決める** (上の「見やすい動画にする」)。切り替えの前に 1 秒、章の見出しは 4 秒

## 場面を組む部品 (samples/mophila_intro/ の型)

intro は `main.moph` (章の順番と output) / `theme.moph` (画面、色、時間の決まり) / `slides.moph` (部品) / 場面ごとのファイル (`export func run(now)` が場面を組んで次の開始時刻を返す) に分かれている。新しく書くときは theme と slides を写す。場面の基本形は「左にコード、右にそのコードが実際に動く箱、下にナレーションの字幕」:

- `place(o)` — View に置いて返す。図形は `opacity: 0` で作る
- `fade(o, at, from, to)` / `show(objs, at, end)` — 出し入れ。`show` は順に 0.08 秒ずらす
- `text(s, x, y, size, color, anchor)` / `heading(s)` / `chapter(title, at)` — 文字、上の見出し、章の見出し
- `card(lines, x, y, w, size)` — コードのカード。行数から高さが決まる。1 行は幅に収まる長さに折り返す (幅 7.6、文字 0.28 なら 40 字程度)
- `outline(x, y, w, h)` — 線だけの枠。先に置いた View を隠さない
- `scene()` / `add(sc, objs, at)` / `say(sc, s, at)` / `close(sc, end)` — 場面に出すものと字幕を溜め、終わりが決まったら一斉に置く。`close` は次の場面の開始時刻 (`end + pause`) を返す
- `say(sc, "……です。", at)` — ナレーション 1 行 (ですます体)。字幕トラックに置き、読む時間 + 0.6 秒後の「次の行を出せる時刻」を返す。`t = say(sc, "...", t)` で台本を書き、図形は `add(sc, objs, t)` でその行に合わせて出す。画面で見せてから言う。見せるものが動き終わる時刻と行の終わりは `longer(t, t1 + 4.5s)` で合わせる
- `demo(sub, x, y, w, at, end)` — 別の View を箱として置き、at からその Timeline を動かす。`loop_demo(sub, x, y, w, at, every, end)` は every ごとに置き直して繰り返す。`repeat(tl, at, every, end)` は Timeline を繰り返し置く
- `demo_cut(sub, x, y, w, at, len)` — 同じ View (import した output など) を 2 か所の場面で使うなら、先の場面はこれにする。duration 付きの Timeline に入れて置くので、中の Timeline が最後まで進まない。切らないと終わった値 (フェードアウト後の opacity 0 など) を書き続けて、あとの場面で真っ黒になる。位置と大きさは場面ごとに motion で置き直す
- 字幕は同時に 1 つ。次の行が始まるまで、または場面の終わりまで出す。字幕の帯は画面下 (y > 7.0 相当) を覆うので、そこに図形や文字を置かない。字幕は 1 行 36 文字まで (2 行になると帯が y 6.6 まで広がる)。長い文は `say` を 2 回に分ける
- 右の箱の動きは `loop_demo` / `repeat` で場面の最初から最後まで繰り返す。ナレーションの間に止まった画を見せない
- 時刻は `now` を積み上げる。`clock` は import したモジュール名と衝突するので使わない

## 書いた後の確認

0. 言語や部品を変えたら `scripts/check_examples.py` (examples / samples が全部 `run` でき、error.moph の各行が期待の種別で止まる)。`cargo test` は本体用
1. `mophila timeline a.moph` — 何が、いつ、どう変わるかのテキスト。`--filter kind=TextArea` `--filter text=字幕` `--filter from=30s --filter to=60s` で絞る。末尾の「text visibility」に、テキストごとの表示区間と、文字数から見て短すぎるもの (`SHORT`) が出る。「subtitles and audio」に字幕と音声の区間 (字幕にも `SHORT`)
2. `mophila sheet a.moph -o sheet.png --times 3s,12s,45s` — 指定した時刻 (または `--every 10s`) のフレームを格子にした画像。構成とテンポを一目で見直す
3. 1 と 2 を、書いていない側 (別の agent か人) に渡して「伝わるか / テンポは合っているか / 被りは無いか」を指摘させ、直す
4. 最後に `preview` で通して見て、切り替えの間と動きの速さを確かめる

## エラー

`種別.細目: line N: message`。種別のツリーは `examples/error.moph`。よくあるもの:
- `TypeError.ArgumentType: Circle.position expects AnchoredPosition, found Tuple` → `apos!` を忘れている
- `ValueError.DurationRequired` → 実数時刻の Timeline に duration が無い、または時刻の単位が混在
- `NameError.UndefinedVariable: "math" is not defined; add "import math"`
