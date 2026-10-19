# 言語の使い方

正は言語仕様 <https://misebox.github.io/mophila/#/docs/spec>。ここは考え方と、書くときに引っかかる所だけ。
名前と引数は書かない。`mophila doc` かリファレンスで引く。

## 見取り図

木は 1 本で、場所と時間を同じ Node が持つ。

```
Composition ─ box, duration
 ├── "bg"     in ─── out    Rect
 ├── "badge"  in ─── out    func (t) -> Node
 └── "intro"  in ─── out    Composition
                              ├── "title" ...
                              └── "logo"  ...
```

- **Composition** は大きさ (`box`) と長さ (`duration`) と、Layer の積み重ねを持つ。`output` するのは Composition
- **Node** は描かれるものの総称。図形、Group、Composition、Audio、Narration。すべて不変
- **Layer** は Composition に置いたもの 1 つ。名前と、置いたものと、`in` と `out` を持つ。`in` と `out` は親 Composition の時間軸で数える。Layer という型は無く、`place` が作る
- 置けるものは 2 種類。Node そのものか、時刻 `t` を受けて Node を返す純粋関数 `func (t)`
- `in <= t < out` の外では、その Layer は存在しない。描かれず、音も鳴らず、何も計算しない
- 積み重ねの順は置いた順。後に置いたものが手前
- 置いた Layer は取り除けず、置き換えも、`in` と `out` の変更もできない。消えることは `out` で表す
- Layer は束ねられない。別々の時刻に出入りするものをまとめたいときは、Composition を作って 1 枚の Layer として置く

```
comp.place("badge", badge, in = 2s, out = 8s)
```

名前は String で、同じ Composition の中で一意。`in` と `out` は必須。

## 不変の Node と純粋関数

図形は不変の値。属性への代入は無い。変えたいときは `copy(...)` で別の値を作る。

```
let big = dot.copy(radius = 4vy)
```

動くものは関数で書く。`func (t)` は毎フレーム呼ばれ、その時刻の図形を新しく返す。状態を持たず、同じ `t` には同じ Node が返る。

```
func dot(t) {
  Circle(position = Vector(ease(t, 0s, 3s, 3, 13), 4.5), radius = 1, fill = #e0604a)
}
```

- **`t` は Layer の `in` を 0 とした時刻** (Duration)。同じ関数を 2 回置けば、どちらも自分の `in` から動き出す
- 各フレームはその時刻だけで決まる。前のフレームに依存しないので、preview で時間を戻しても同じ絵になる
- 動きは「`t` を式に入れる」ことで書く。「いつ何を変える」という命令の列は書かない
- 時間に依らない図形は、Node のまま置く。毎フレーム再計算されない

```
comp.place("pop-a", pop, in = 1s, out = 3s)
comp.place("pop-b", pop, in = 4s, out = 6s)     # 同じ関数。t はここでも自分の in が 0
```

## Group と transform

すべての Node が同じ transform 一式を持つ。

| 属性 | 意味 |
|---|---|
| `position` | `anchor` を置く場所。親の座標で書く |
| `anchor` | Node 自身の座標で見た基準点。`rotation` と `scale` の中心 |
| `rotation` | 回転。ラジアン、時計回りが正 |
| `scale` | 拡大率 |
| `opacity` | 不透明度 0..1 |

`position` と `anchor` は別の属性で、どちらも Vector。`position` に `anchor` が来る。

**自身の原点と `anchor` の既定は型で決まる。** 置き間違いの大半はここ。

| 型 | 自身の原点 | `anchor` の既定 |
|---|---|---|
| Rect / TextArea | 左上 | 自身の中心 |
| Circle / Ellipse | 中心 | `(0, 0)` |
| Line / Polygon / Path | 点の座標そのまま | `(0, 0)` |
| Group | 子の座標系の `(0, 0)` | `(0, 0)` |

- `Rect` の `position` は既定では中心。左上に置くなら `anchor = Vector(0, 0)`
- 背景は `Rect(position = Vector(0, 0), anchor = Vector(0, 0), w = 16, h = 9, fill = ...)`
- 棒グラフの棒のように、足元を固定して伸ばすなら、`anchor` を足元に置く

**Group** は Node をまとめた親。引数のうち属性でないものが中身で、書いた順に描く。Group も不変で、後から足せない。

- 中の座標は Group の `position` からの相対
- 子は親の `rotation` `scale` `opacity` を受ける。Group を動かせば中身ごと動き、`opacity` を下げれば中身を 1 枚として薄くできる
- 中身が先に決まらないときは List に集めて広げる (`Group(...items)`)
- `clip` に図形を渡すと、その輪郭の外を描かない。`camera` で中身の一点へ寄る

**親子の連鎖は入れ子の深さで書く。** 順序に依存して親を決める記法は無い。外側の回転が内側を動かし、内側の回転は外側を動かさない。

```
func arm(t) {
  let s = sin(tau * t / 2s)
  Group(position = Vector(50vx, 40vy), rotation = 0.5 * s,
    Rect(anchor = Vector(1vx, 0), w = 2vx, h = 20vy, fill = #3060c0),
    Group(position = Vector(0, 20vy), rotation = 0.4 - 0.4 * s,
      Rect(anchor = Vector(1vx, 0), w = 2vx, h = 20vy, fill = #3060c0)))
}
```

腕の付け根、肘の順に Group を入れ子にし、各関節の `rotation` に式を入れる。関節の先端を次の起点にする代わりに、子を親の Group の中に書く。

## 座標、Length、center、角度

- 座標は `Composition` の `box` の単位。`box = Vector(16, 9)` なら 0..16 × 0..9。x は右、y は下に増える
- **Length** は `box` に対して描画時に解決する量。`vx` は幅の 1/100、`vy` は高さの 1/100、`vmin` `vmax` はその小さい方と大きい方、`px` は出力のピクセル。`20vy` は箱の高さの 2 割
- 単位なしの数は `box` 単位。`radius = 1` と `radius = 11.1vy` (高さ 9 の箱で) は同じ
- Vector の成分は数でも Length でもよい。`Vector(50vx, 40vy)`。足し引き、数との掛け算割り算、`ease` の補間が Length に対応する
- 入れ子の Composition では、それぞれの `box` が基準になる。子の中の `20vy` は子の高さの 2 割
- `center` は描く先の Composition の中心。入れ子の中ではその Composition の中心。`center + polar(20vy, tau / 6)` のように式で位置を書く
- 角度は Number のラジアンで、時計回りが正。`rotation` も `polar` も `sin` も同じ単位。`tau` は 1 周 (`import { tau, polar } from math`)

構図を解像度に依らせないのが目的。ピクセルの数はどこにも書かず、出力の大きさは `render --size` が決める。箱と違う比で出すと余白が付き、同じ比ならぴったり収まる。

## 時間

- 時間の単位は Duration。`10s` `500ms` `1m23s` `01:23`。`25%` は Number (0.25) で Duration ではない
- Layer の `in` と `out` は親の時間軸。`func (t)` の `t` は自分の `in` が 0
- 時刻には式を書ける。長さを変数に取っておけば、テンポを変えるのは 1 か所で済む
- 動画の長さは、`output` した Composition の `duration`

**補間は `ease`。** 区間の外では端の値に止まる。

```
ease(t, t0, t1, v0, v1)     # t0 から t1 の間に v0 から v1 へ
```

`linear` `ease_in` (加速) `ease_out` (減速) も同じ形。`v0` と `v1` は Number、Vector、Length。`t` `t0` `t1` は Duration。区間の外で止まることを使えば、`if` を書かずに出入りが書ける。

```
opacity = ease(t, 0s, 0.4s, 0, 1) * ease(t, life - 0.4s, life, 1, 0)    # 出て、消える
```

**`progress` は名前付きの表。** 対象も長さも持たない。0 から 1 の数で呼ぶと、その点の値を Tuple で返す。

```
let slide = progress [:x, :y] {
  0:   2, 4.5
  0.4: _, 6 :ease
  1:  14 :ease_out, 4.5
}
func ball(t) {
  let (x, y) = slide(t / 3s)
  Circle(position = Vector(x, y), radius = 0.5, fill = #e04040)
}
```

- 行の左は 0 から 1。0 と 1 の行は必須。列の名前を先に並べ、行には値だけを書く
- `_` はその列を飛ばす。列ごとに別々の位置でキーフレームを打てる
- Easing は値の隣に書く (`:linear` 既定、`:ease_in` `:ease_out` `:ease`)
- 範囲の外は端の行の値
- 何秒かけるかは呼ぶ側が決める。`slide(t / 3s)` なら 3 秒で端から端まで。`t / 3s` は Number
- 周期的な動きは `(t / 4s) % 1` を渡す。円運動や振動は `sin` `cos` を式で書く

選び方: 2 点間の動きは `ease`、複数の値を同じ時間軸でまとめて動かすなら `progress`、周期や物理は式。

## アニメーションの組み立て

- **1 つの動きは 1 つの `func (t)` にまとめる。** 丸が動き、棒が伸び、文字が出て、最後に消える、までを 1 つの Group にして `opacity` で消せる
- **出入りの時刻は `in` と `out` で決め、出方は式で決める。** `out` を過ぎると何も計算されないので、消えた後の値を気にしなくてよい
- **順に出すのはずらしで書く。** `i * stagger` を `ease` の時刻に足す。個数が決まっていれば `for` で Layer を置く
- **同じ動きは関数にして、値を引数で渡す。** 引数で受けた値を使って `func (t)` を返す関数が書ける

```
func bar_at(i, color) {
  func (t) {
    let h = ease_out(t, i * stagger, i * stagger + 0.6s, 0, 2 + i)
    Rect(position = Vector(2 + i * 2, 8), anchor = Vector(0, h), w = 1.2, h = h, fill = color)
  }
}

for (i, color) in [#e04040, #e0a020, #40a060, #4080e0].enumerate() {
  comp.place("bar-{}".format(i), bar_at(i, color), in = 0s, out = len)
}
```

- **部品の出入りは、部品を包む関数で書く。** 関数を受けて、フェードを足した関数を返す

```
func fade_in(f, d) {
  func (t) { f(t).copy(opacity = ease(t, 0s, d, 0, 1)) }
}
comp.place("dot", fade_in(dot, 0.5s), in = 1s, out = 4s)
```

- 文字列の埋め込みは無い。名前は `"bar-{}".format(i)` で作る
- 同じ絵を別の場所や大きさで使うなら、Composition を作って `copy(position = ..., scale = ...)` で置く。Composition は不変なので何度でも置ける。中の `t` は置いた Layer の `in` から始まる

```
let inner = Composition(box = Vector(4, 3), duration = 3s)
comp.place("left", inner.copy(position = Vector(1, 1)), in = 0s, out = 3s)
comp.place("right", inner.copy(position = Vector(9, 1), scale = 2), in = 1s, out = 4s)
```

- `func (t)` の中で毎フレーム高い計算をしない。時間に依らない値は関数の外で 1 度だけ作る。`Shader` や `World` を `fill` に入れるなら、関数の外で作って渡す

## 他の言語と違うところ

- `(1, 2)` は Tuple。座標は Vector。**型の決まっている場所に書いた Tuple は、その型になる**ので `Circle(position = (8, 4.5))` と書ける。型の決まらない場所では Tuple のまま
- 決まった値しか取らない型は `:center` `:left` のような Symbol
- `1/3` は演算だが、分数のまま持つので `1/3 * 3` は `1`
- 文字列への埋め込みは無い。`"x = {}".format(x)` で位置に置き換える。`{name}` の名前は読む人向けで、置き換えには使わない (Dict を 1 つ渡したときだけ名前で引く)
- コメントは `# ` (空白が要る)。`#e04040` は色
- `if` は式。ブロックの最後の式が値になる。関数の戻り値も最後の式
- `T(a = 1)` は属性を型で検査する。無い属性は `NameError.UndefinedAttribute`
- 比較は続けて書ける。`0 <= x <= 1` は `(0 <= x) and (x <= 1)` で、真ん中は 1 度しか評価しない
- `f(...xs)` で Tuple / List / Range を引数に広げる。Dict を広げると `名前 = 値` になる
- import なしで呼べるのは `log` `type_of` `ease` 系 `center` と型名だけ。値は型名を呼んで作る (`Vector(8, 4.5)` `Color(224, 96, 74)`)
- リテラルは型名を呼ぶのと同じ。`[1, 2]` = `List(1, 2)`、`{ "k": v }` = `Dict(k = v)`、`0..5` = `Range(0, 5)`、`0..=5` = `Range(0, 6)`
- builtin の型と関数の名前は予約されている。`struct List { ... }` は `NameError.Reserved`
- 範囲 `0..5` と添字の両端は整数。`xs[1.5]` は `ValueError.OutOfRange`

## import

```
import math                        # 標準ライブラリ
import { sin, tau } from math      # 名前だけ持ち込む
import .other                      # 同じ場所の other.moph を other に束縛
import { a, b } from .other        # 名前を直接持ち込む
import "bgm.m4a" as bgm            # 音声ファイル
import "logo.png" as logo          # 画像 (png jpg gif webp bmp tiff)
import @.slides                    # mophila.yaml の root から
import @parts.box                  # mophila.yaml の aliases から
import "@/media/bgm.m4a" as bgm    # 文字列の中では "/" 区切り
import config                      # mophila.yaml の config: を読む
```

見えるのは `export` した名前と、`other.output` (その中で `output` した Composition) だけ。同じファイルは 1 度しか実行されず、実体を共有する。`import` はトップレベルにしか書けない。

## プロジェクト設定

`mophila.yaml` をコマンドを実行するディレクトリに置くと自動で読まれ、読んだことが 1 行表示される。

```yaml
entry: scenes/main.moph      # スクリプトを省略したときに使う
root: .                      # @/ と @. が指す場所
aliases:
  parts: src/parts           # @parts. が指す場所
config:
  width: 16
  title: "mophila"
  debug: false
```

```
import config
let comp = Composition(box = Vector(config.width, 9), duration = 6s)
```

- 設定に書いたパスは**設定ファイルからの相対**。コマンドラインのパスは、いま居るディレクトリから
- `config` に書けるのは数・文字列・真偽の 3 つ。色は書けないので、色相を数で持って `color` module で作る
- `MOPHILA_WIDTH=32` か `--set width=32` で上書きできる。型が合わなければエラー
- `-f other/mophila.yaml` で別の設定を使える
- `render:` に `fps` `size` `codec` などの既定を書ける。コマンドラインが勝つ
- `import { a as b }` は書けない。名前がぶつかるなら片方を Module として読む

## 自分の型を作る

トップレベルに `record` か `struct` を書く。違いはイミュータブルかどうかだけ。`record` は作った後に書き換えられず、型名とフィールドがすべて同じなら `==` で等しい。`struct` は属性を書き換えられ、`==` は同じ実体かを見る。図形や Group の部品は、型を作らず関数で返すほうが素直。

```
record Size {
  w: Number
  h: Number
  method area(self) -> Number { self.w * self.h }
}
```

- `func` は受け手なしで型名から呼ぶ (`Counter.new(10)`)。`method` は第 1 引数が受け手 (使わないなら `_`)
- `func new` が無ければ、`private` でないフィールドを宣言順に受け取る。位置で渡せるのは既定値の無いフィールドまで
- 複製は record が `copy(field = 値)`、struct が `shallowCopy()` / `deepCopy()`
- 宣言の前の行に `@immutable` `@nocopy` `@nodeepcopy` `@deprecated("説明")` を並べられる
- `alias V = Vector` はそのファイルの中だけの短い名前。`export` できない

## 図形と塗り

図形は `Circle` `Ellipse` `Rect` `Line` `Polygon` `Path` `TextArea`。
どの型がどの属性を持つかは変わるので、**書く前に `mophila doc` かリファレンスで引く**。必須の属性には印が付いている。

- 効きようがない属性は持たない。`Line` に塗りは無い。書けば `NameError.UndefinedAttribute` で止まる
- 長さを表す属性 (`radius` `w` `h` `strokeWidth` など) は Length を取る
- 線を少しずつ描き出す、文字が占める大きさを測る、といったことはメソッドでできる。線の長さを測って `dashOffset` を動かすと、線が伸びて見える

```
func draw(t) {
  let total = p.length()
  p.copy(dash = [total, total], dashOffset = ease(t, 0s, 2s, total, 0))
}
```

塗りは色のほかに、グラデーション、読み込んだ画像、Shader、`World` (3D) を入れられる。画像は図形の形に切り抜かれる。

Shader は `func (x, y, t)` を渡すと GPU で全ピクセル分走る。x, y は箱の座標、t は秒。書けるのは数値の計算だけ (文字列・図形・Dict は不可、再帰不可)。`run` では走らず、`render` `preview` `sheet` で走る。

## 標準ライブラリ

グラフ、図、文字の書式、画面の絵、背景、幕、時計、画像と地図、メーター、色、座標、並べ方 — よく要るものは module になっている。**自分で図形を並べる前に、そこに部品が無いか見る。**

- `mophila doc` — 型・属性・メソッド・エラー・math の一覧 (JSON)
- <https://misebox.github.io/mophila/#/lib> (module の関数と引数) と <https://misebox.github.io/mophila/#/builtins> (型)。どちらもソースから生成している。`llms-full.txt` はその全部を 1 ファイルにしたもの
- エディタを繋いでいれば、`.` の後や `(` の中の補完に署名と説明が出る

`import 名前` で module ごと、`import { 名前 } from module` で名前だけ持ち込む。組み合わせた例は <https://misebox.github.io/mophila/#/samples> の showcase と report。

標準 module の部品は Node (多くは Group) を返す。時間で動かすには、`func (t)` の中で `t` から進み具合 (0..1 の Number) を `ease` で計算して部品に渡す。部品を包む Group の `opacity` で出し入れできる。引数の並びと意味は module ごとに違うので、毎回引く。

## 音声と字幕

```
import "bgm.m4a" as bgm
comp.place("bgm", bgm.copy(volume = 0.6, loop = true), in = 0s, out = 30s)
comp.place("line-1", Narration(text = "……です。"), in = 2s, out = 6s)
```

- 音声は `out` で切る。`loop = true` は `out` まで繰り返し、繰り返さなければファイルより長くならない。同じ音声を何度でも置け、重なれば混ざる
- `Narration` は文だけを持ち、画面には描かない。**その文を読んだ音声が動画の音になり、同じ文が字幕になる**。字幕だけを置く型は無い。`voice` で音声合成の型を渡せる
- 字幕の表示は読み始めから読み終わりまで。長さは合成した音声から測るので、`out` の見積もりがずれても声とずれない。合成した音声が `in` から `out` より長ければ、`out` で切って警告が出る
- **字幕を TextArea で描かない**
- `run` では合成しない。`render` と `preview` で鳴る

## 組み合わせて再利用する

書いたものはそのまま部品になる。同じものを 2 度書かない。

**関数にする** — 同じ図形、同じ動きを何個も書かない。値は引数で渡す。

```
func dot(x, y, c) { Circle(position = Vector(x, y), radius = 0.3, fill = c) }
```

**ファイルを部品にする** — `output` した Composition は `import` 先から `名前.output` で取れる。Composition は不変なので、何度でも置ける。

```
import .dial
comp.place("dial-a", dial.output.copy(position = Vector(1, 1)), in = 0s, out = 4s)
comp.place("dial-b", dial.output.copy(position = Vector(9, 1)), in = 2s, out = 6s)
```

別々の値で作りたいなら、Node を返す関数を `export` して呼ぶたびに作る。

**場面を積む** — 長い動画は、章ごとにファイルを分け、`export func run(now)` が Layer を積んで次の開始時刻を返す形にする。共有のリストに `(Node か func, 始まり, 終わり)` を積み、全体の長さが決まってから、`main` が 1 つの Composition に置く。

```
# theme.moph
export let layers = []
export func put(node, at, end) { layers.push((node, at, end)) }

# scene.moph
import { put } from .theme
export func run(now) {
  put(Circle(position = center, radius = 20vy, fill = #e04040), now, now + 3s)
  now + 3s
}

# main.moph
import { layers } from .theme
import .scene
let end = scene.run(0s)
let comp = Composition(box = Vector(16, 9), duration = end)
for (i, l) in layers.enumerate() {
  let (node, at, out) = l
  comp.place("layer-{}".format(i), node, in = at, out = out)
}
output comp
```

実例は <https://misebox.github.io/mophila/#/samples> の mophila_intro。

**決まりを 1 か所に置く** — 色・大きさ・時間の定数は 1 つのファイルに `export` してまとめ、全部の場面がそれを読む。テンポや配色を変えるときはそこだけ触る。

```
# theme.moph
export let bg = #f4f1ea
export let accent = #e04040
export let pause = 1s
```

`import ..parent` は文法上は通るが、ディレクトリをまたぐ設計は避ける。

## よくある間違い

- **属性に代入する** — `c.radius = 2` は `TypeError.AttributeType: Circle is immutable`。`c.copy(radius = 2)` で別の値を作る
- **`Rect` が思った位置に出ない** — `position` は中心。左上を指すなら `anchor = Vector(0, 0)`。背景の `Rect` で忘れやすい
- **`out` を書き忘れる** — `Composition.place needs out =`。`in` と `out` は必須
- **同じ名前を 2 度置く** — `ValueError.AlreadyPlaced`。ループで置くときは `"bar-{}".format(i)` のように名前を変える
- **`func (t)` の中の誤りが `run` で出ない** — `run` は Layer に置いた関数を呼ばない。スクリプトの末尾に `log(dot(1s))` と書けば `run` で中身が走る。絵は `render --at 1s` か `sheet --times 1s,3s` で見る
- **`t` が Layer の `in` からの時刻だと忘れる** — 動画全体の時刻で `ease` を書くと、`in` が 0 でない Layer で区間がずれる。全体の時刻が要るなら、Layer の始まりの時刻を `t` に足す
- **`ease` の時刻に単位が無い** — `ease(t, 0, 1, ...)` は `ease: t0 expects Duration, found Number`。`0s` `1s` と書く
- **度で回転を書く** — `rotation` はラジアン。90 度は `tau / 4`
- **Tuple を Vector が要る場所に入れて要素数が合わない** — `Circle.position expects Vector, found Tuple`。2 つの成分に揃える
- **標準 module を import し忘れる** — `NameError.UndefinedVariable: "math" is not defined; add "import math"`
- **属性名の綴り** — `NameError.UndefinedAttribute: Circle has no attribute "width"`。`mophila doc` で引く
- **Layer を取り除こうとする** — できない。消したいなら `out` を早める。条件で出し分けるなら、Layer を分けて `in` と `out` を決める

エラーは `種別.細目: line N: message`。全部の種別は <https://misebox.github.io/mophila/#/builtins/errors> にある。
