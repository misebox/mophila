# mophila 言語 下書き

Phase 1 の検討メモ。すべて仮。

## 方針

決定済みの内容は mophila-spec.md に移した。ここには未決と、たたき台だけを残す。

## 型

| 型 | 中身 | 作り方 | duration |
|---|---|---|---|
| Motion | 値の時間変化だけ。対象なし | `motion (t, a, b) { ... }` | 既定は最後の時刻。`m1.duration = 10s` で上書き可 |
| Timeline | 対象オブジェクト + 属性への割り当て。Timeline の中に Timeline を置ける | `m1.apply(c1, f)` / `context c1 as o { motion (t) { 時刻: o.attr = ... } }` | 同上 |

Timeline は `track.place(tl, at: 3s)` で置ける。Motion は単独では置けない。

## たたき台

```
# comment
let v = new View { box: vector!(4, 3) }
let redCircle = new Circle { fill: #e04040 }
let greenRect = new Rect   { fill: #40a040 }

v.place(redCircle, center: (1, 1.5), w: 1/3)      # サイズ記法は未決
v.place(greenRect, topLeft: (2, 0.5), w: 1, h: 1) # 基準点の書き方は未決

let track = new Timeline { duration: 10s }        # 入れ物。中に Timeline を置く

# Motion: 値の表。t = その行の時刻 (秒)、a = 1 列目、b = 2 列目 (左の列を参照)
let m1 = motion (t, a, b) {
  1s: t*3, a*1, b*3      # 3, 3, 9
  2s: t*4, a*2, b*4      # 8, 16, 64
  5s: t*3, a*3, b*5      # 15, 45, 225
}
# m1.duration は 5s。m1.duration = 10s で延ばせる

let c1 = new Circle { fill: #a0a0a0 }

# Timeline: Motion を対象に当てる。apply は組み込みメソッド、列を受け取る関数を渡す
let calc = func (c1, t, [x, r, _]) {
  context c1 as o {
    o.center, o.radius = vector!(x, 1.5), r / 10
  }
}
let tl = m1.apply(c1, calc)

# context で囲んで属性に代入する形。戻り値は Timeline
context c1 as o {
  let tl2 = motion (t) {
    1s: o.radius = t*3
    2s: o.radius = t*4
    5s: o.radius = t*3
  }
}

track.place(tl, at: 3s)
v.addTrack(track)

output v
```

## CLI (Phase 3 で詰める)

- 描画せず `log` の出力だけ見るオプションが要る (スクリプトの確認用)
- `render --trim 00:15..00:30` で区間だけ出す (実装済み)。`00:15` は 15 秒以降、`..01:30` は最初から。音声と字幕もその区間に合わせてずらして切る
- `render` と `sheet` は進捗を stderr に出す (実装済み)。割合はフレーム数ではなく時間: 直近のフレームの所要時間から残りを見積もり、経過 / (経過 + 残り)。同じ行を書き換え続け (0.2 秒に 1 回まで)、終わったら 1 行残す
- 画像出力は `--at <Duration>` で時刻を指定する (実装済み)。`render` の `-o` 省略時は `output.mp4`、`--at` 付きなら `output.png`。埋め込みバイナリは `-o` 省略時にウィンドウで再生
- `lsp`: 実装済み。stdio の Language Server。診断 (構文は入力中、実行時のエラーは保存時。import 先のエラーはそのファイルへ)、補完、ホバー (型の属性、保存時の変数の値)、定義へ移動 (import 先も)、参照、名前の変更、アウトライン、import 不足のクイックフィックス。VS Code 拡張は `editors/vscode/`、Neovim は同 README の設定
- `timeline`: 実装済み。何が、いつ、どう変わるかを時刻順にテキストで出す。`--filter kind= / attr= / text= / from= / to=`。テキストごとの表示区間と、文字数から見て短いものも出す。字幕と音声の区間も出す (`kind=Subtitle` / `kind=Audio`)
- サンプル: `samples/mophila_intro/` が紹介動画 (作れるもの / 組み合わせ / 時間の書き方 / LLM と書く / 使う。他のサンプルを import して見せる)。`samples/fractal.moph` (親の先端を読む枝)、`samples/walker.moph` (関節を親の先端でつないだ歩く人。`make()` で部品として import できる)、`samples/crowd.moph` (walker を奥行きを付けて並べた夕暮れの通り)、`samples/mandelbrot.moph` / `julia.moph` / `burning_ship.moph` (標準ライブラリ fractal のエスケープタイム系の部品で描く)。標準ライブラリは `src/stdlib/`: math は Rust、fractal は本体に埋め込んだ `.moph` (`include_str!`) で、`import fractal` のように名前で読む
- 音声と字幕: 実装済み (仕様 3.8)。`render` が ffmpeg で音声トラック (aac / opus) と字幕トラック (mov_text / webvtt / srt) を付ける。`preview` は起動時に ffmpeg で PCM にしてメモリに持ち、cpal で鳴らす (一時停止・シークに追従)。字幕は preview と sheet で画面の下に重ねて出す (動画には入らない)。焼き込み (映像に描く) は未実装
- `sheet`: 実装済み。指定時刻 (`--times`) か等間隔 (`--every`) のフレームを格子画像にする
- `preview`: 実装済み。ウィンドウを開いて実時間で再生し、最後まで行ったら最後の場面で止まる (Space で先頭から再生)。描画が間に合わなければコマを飛ばす。Space で一時停止・再開、← → / h l で 10 秒 (一時停止中は 1 秒)、Shift 付きで全体の 10% 移動。タイトルバーに再生位置を出す。埋め込みバイナリを引数なしで起動したときも同じ
- `bundle a.moph -o a`: 実装済み。mophila 本体のコピーに、ソースと import で辿れるファイルを埋め込む (ファイル単位で必要なものだけ)。起動時に解釈するのでコンパイルではない。動的リンクのスタブ版は未実装

## 未決

0. コンパイル — 今はインタプリタ。実測ではキーフレームの式の評価はフレームの 2〜7% で、ネイティブ化しても全体は 5% 程度しか縮まない。LLVM IR を出す案はあるが、目的 (速さ以外) が決まってから

1. サイズ記法 — `1/3` のような分数で、幅基準か高さ基準かが分かり、縦横比が保たれる書き方
3. 箱の名前 — view / viewbox / area
4. viewBox の比率と出力サイズの比率が違うときの扱い — 今は「余白を付けて収める (中央寄せ)」で実装。View の入れ子も同じ規則
5. timeline の duration — 必須か、省略時は自動 (最後の配置の終わり) か
6. transition の指定方法 — 配置の属性 (`place(tl, at:, fadeIn:, fadeOut:)`) は実装済み。行末の修飾子は「その行に入る区間」に付く (先頭行に書いた場合は最初の区間)。`reverse()` は区間ごと対応させ、ease_in ↔ ease_out、fade_in ↔ fade_out を入れ替える
7. 修飾子は `:linear` / `:ease` / `:fade` を基本にする (実装済み)。`:fade` は最初の区間なら 0→1、最後の区間なら 1→0、途中は opacity の値で書く。非対称の加減速のために `:ease_in` / `:ease_out` は残している
8. Tuple の展開構文 — `apos!(:center, ...pos)` や `f(...args)` のように Tuple / List を引数に展開する記法が要るかもしれない (`...expr` 案)
9. キーフレームの時刻は Duration か、0..1 の実数 (`50%` も可)。実数の Timeline は `duration` が必須。`duration` を変えると全キーフレームが比例して伸縮する。行の中の `t` は書いた時刻のまま (実装済み)
10. 時間の関数で書く区間 — 円軌道などをキーフレームで書くと多角形になる。`0s..8s: b.position = apos!(...)` のように範囲を時刻にした行を「その区間は式を毎フレーム評価する」と定義する案
