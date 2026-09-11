# 言語の使い方

正は言語仕様 <https://misebox.github.io/mophila/#/docs/spec>。ここは書くときに引っかかる所だけ。

## 他の言語と違うところ

- `(1, 2)` は Tuple。座標は `Vector(1, 2)`、位置は `Pos(1, 2)`。Vector や Pos が要る場所に素の Tuple は書けない。図形の位置は `position` 属性
- 決まった値しか取らない型は `:center` `:topLeft` のような Symbol
- `1/3` は演算だが、分数のまま持つので `1/3 * 3` は `1`。`10s` `500ms` `1m23s` `01:23` は Duration。`25%` は Number (0.25) で Duration ではない
- 文字列への埋め込みは無い。`"x = {}".format(x)` で位置に置き換える。`{name}` の名前は読む人向けで、置き換えには使わない (Dict を 1 つ渡したときだけ名前で引く)
- コメントは `# ` (空白が要る)。`#e04040` は色
- `if` と `context` は式。ブロックの最後の式が値になる
- `T(a = 1)` は属性を型で検査する。無い属性は `NameError.UndefinedAttribute`
- 比較は続けて書ける。`0 <= x <= 1` は `(0 <= x) and (x <= 1)` で、真ん中は 1 度しか評価しない
- `f(...xs)` で Tuple / List / Range を引数に広げる。Dict を広げると `名前 = 値` になる
- import なしで呼べるのは `log` と `type_of` だけ。値は型名を呼んで作る (`Vector(8, 4.5)` `Pos(8, 4.5)` `Color(224, 96, 74)`)
- リテラルは型名を呼ぶのと同じ。`[1, 2]` = `List(1, 2)`、`{ "k": v }` = `Dict(k = v)`、`(1, 2)` = `Tuple(1, 2)`、`0..5` = `Range(0, 5)`、`0..=5` = `Range(0, 6)`
- builtin の型と関数の名前は予約されている。`struct List { ... }` は `NameError.Reserved`
- 文字列は `+` で連結。`\"` `\n` のエスケープがある

## import

```
import math                        # 標準ライブラリ
import .other                      # 同じ場所の other.moph を other に束縛
import .other as m                 # 別名
import { a, b } from .other        # 名前を直接持ち込む
import "bgm.m4a" as bgm            # 音声ファイル
```

見えるのは `export` した名前と、`other.output` (その中で `output` した View) だけ。同じファイルは 1 度しか実行されず、実体を共有する。`import` はトップレベルにしか書けない。

## 自分の型を作る

トップレベルに `record` か `struct` を書く。違いはイミュータブルかどうかだけ。`record` は作った後に書き換えられず、型名とフィールドがすべて同じなら `==` で等しい。`struct` は属性を書き換えられ、`==` は同じ実体かを見る。

```
record Size {
  w: Number
  h: Number
  method area(self) -> Number { self.w * self.h }
}

struct Counter {
  n: Number = 0
  private step: Number = 1
  func new(start: Number) -> Counter { Counter(n = start) }
  method next(self) -> Number {
    self.n = self.n + self.step
    self.n
  }
}
```

- `func` は受け手なしで型名から呼ぶ (`Counter.new(10)`)。`method` は第 1 引数が受け手 (使わないなら `_`)
- `func new` が無ければ、`private` でないフィールドを宣言順に受け取る。位置で渡せるのは既定値の無いフィールドまでで、それ以外は `名前 = 値`
- 複製は record が `copy(field = 値)`、struct が `shallowCopy()` / `deepCopy()`。`func new` を書いた型では、フィールドを変える `copy` はできない (`new` を通らないため)
- 宣言の前の行に `@immutable` `@nocopy` `@nodeepcopy` `@deprecated("説明")` を並べられる
- `alias P = Pos` はそのファイルの中だけの短い名前。`export` できない

## 時間

```
motion (t) { 0s: c.radius = 1
             2s: c.radius = 3 :ease }     # Timeline
motion (t, r) { 0s: 1
                2s: 3 }                   # Motion (対象を持たない値の表)
motion c [:position, :radius] { ... }      # 対象と属性を先に並べる形
```

- 時刻は Duration か 0..1 の実数 (`50%` も可)。1 つの motion で混ぜない。0..1 で書いたら `duration` が要る
- `duration` は長さ。代入しても行の時刻は動かない (短くすればその先は使われない)。時刻ごと動かすのは `scale(k)` `fit(3s)` `trim(from =, to =)`。どれも新しい Timeline を返す
- `0..1: o.position = Pos(math.cos(math.TAU * t), ...)` のように範囲を書くと、その区間は補間せず毎フレーム式を評価する。円運動・振動はこれで書く。行を刻んで近似しない
- 行末の修飾子はその行に入る区間に効く。`:ease` (加減速) `:ease_in` (加速) `:ease_out` (減速) `:linear` (既定)
- 出し入れは `o.opacity` を書くか、`animation` の `fade_in` / `fade_out`
- `tl.reverse()` で逆再生
- 入れ物: `let track = Timeline()` に `track.place(tl, at = 3s, fadeIn = 1s)`。`v.addTrack(track)`。Timeline は終わった後も最後の状態を保ち、始まる前は何もしない
- 後から place した Timeline が同じ属性を書けば勝つ
- 動画の長さは、View に置いたものの終わりの最大

## 図形と塗り

| 型 | 位置と形 |
|---|---|
| Circle | `position` `radius` |
| Ellipse | `position` `rx` `ry` |
| Rect | `position` `w` `h` `radius` (角の丸み) |
| Line | `from` `to` |
| Polygon | `points` |
| Path | `from` `segments` `closed` |
| TextArea | `position` `text` `w` (折り返す幅) `fontSize` `font` `align` |

共通: `fill` `stroke` `strokeWidth` `strokeCap` `strokeJoin` `dash` `dashOffset` `opacity` `rotation` `pivot` `blend`。効きようがないものは持たない (Line に `fill` は無い)。

- 回すのは `rotation` (度、時計回り)。中心は外接矩形の中心で、`pivot` で変えられる。頂点を計算し直さない
- `dashOffset` を motion で動かすと破線が流れる
- 塗りは Color のほか `Gradient(from =, to =, stops = [...])` (`kind = :radial` なら from を中心に radius まで)
- ピクセル単位の絵は `fill` に `Shader(color = func (x, y, t) { ... Color })`。x, y は箱の座標、t は秒。GPU で全ピクセル分走るので数値の計算だけ (文字列・図形・Dict は不可、再帰不可)。時間で変える値は t から計算するか、`args` を motion の行で変える。`samples: 4` で 2x2 のアンチエイリアス。複素数は Vector で書ける

## 標準ライブラリ

自分で opacity の motion を書く前に、これらを見る。

| module | 中身 |
|---|---|
| math | `PI` `TAU` `E`、三角関数、丸め、平方根、対数、最大最小 |
| color | `mix` `lighten` `darken` `alpha` `hsl` `gray` |
| shape | Polygon の points = `regular_polygon` `star` `arrow` |
| layout | `grid` `cell` `along` `fit` |
| animation | `fade_in` `fade_out` `fade_to` `slide_in` `slide_out` `move_by` `show` が Timeline を返す |
| fractal | `escape_time(formula =, precision =, coloring =, ...)` が Shader を返す |

## 音声と字幕

```
import "bgm.m4a" as bgm
track.place(bgm, at = 0s, loop = true, volume = 0.6, fadeOut = 3s)
track.place(Subtitle(text = "……です。", duration = 4s), at = 2s)
```

`loop = true` は動画の終わりまで繰り返す (動画は延びない)。字幕は画面に描かれず、動画の字幕トラックになる。preview と sheet では下に重ねて見える。**字幕を TextArea で描かない**。

## View の入れ子

`v.place(sub, at = Pos(x, y, anchor = :topLeft), w = 7)` で別の View を比率を保って置く。中の Timeline は親と同じ時間軸で動く。`track.place(sub, at = 3s)` で置けば、その時刻から動き出す。`sub.opacity` で全体をまとめてフェードできる。

## 組み合わせて再利用する

書いたものはそのまま部品になる。同じものを 2 度書かない。

**ファイルを部品にする** — `output` した View は `import` 先から `名前.output` で取れる。

```
import .clock                                  # clock.moph の output
v.place(clock.output, at = Pos(1, 1, anchor = :topLeft), w = 6)
track.place(clock.output, at = 2s)             # 2s からその中の Timeline が動く
```

同じファイルは 1 度しか実行されないので、2 か所に置くと**同じ実体**が 2 か所に出る。別々に動かしたいなら、View を返す関数を `export` して呼ぶたびに作る。

```
# walker.moph
export func make(near, far, long) -> View { ... }

# 使う側
import { make } from .walker
let a = make(1, 0, 8s)
let b = make(0.6, 3, 8s)     # 別の実体
```

**Timeline を使い回す** — 1 つの Timeline を何度でも置ける。時刻をずらすだけで繰り返しになる。

```
let pop = motion (t) { 0s: c.radius = 0.5
                       1s: c.radius = 1 }
track.place(pop, at = 0s)
track.place(pop, at = 3s)
track.place(pop.reverse(), at = 5s)      # 逆再生
track.place(pop.scale(2), at = 7s)       # 倍の時間をかけて
track.place(pop.fit(0.4s), at = 11s)     # 0.4s に収めて
```

`reverse` `scale` `fit` `trim` は新しい Timeline を返すので、元はそのまま残る。

**形を関数にする** — 同じ図形を何個も書かない。

```
func dot(x, y, c) { Circle(position = Pos(x, y), radius = 0.3, fill = c) }
for (i, c) in [#e04040, #40a040, #4040e0].enumerate() {
  v.place(dot(1 + i * 2, 2, c))
}
```

**決まりを 1 か所に置く** — 色・大きさ・時間の定数は 1 つのファイルに `export` してまとめ、全部の場面がそれを読む。テンポや配色を変えるときはそこだけ触る。

```
# theme.moph
export let bg = #f4f1ea
export let accent = #e04040
export let pause = 1s
```

### 気をつけること

- 同じ View を 2 つの場面で使うなら、先の場面は duration を付けた Timeline に入れて切る。切らないと、終わった Timeline が最後の値 (フェードアウト後の `opacity = 0` など) を書き続けて、あとの場面で見えなくなる
- 後に置いた Timeline が同じ属性を書けば勝つ。重ねるときは置く順で決まる
- `import ..parent` は文法上は通るが、ディレクトリをまたぐ設計は避ける

## 部品が互いに追従する動き

範囲行 `0..1: child.from = parent.to` は毎フレーム評価されるので、親の属性を読む行を書けば子が追従する。Timeline を置いた順に当たるので、親の行を先に置く。

動く例は <https://misebox.github.io/mophila/#/samples> にある。`fractal` は枝が親の先端を読んで再帰で 2^n 本、`walker` は腰 → 腿 → 脛 → 足を親の先端でつなぐ、`crowd` は walker を奥行きを付けて並べる。

## エラー

`種別.細目: line N: message`。よくあるもの:

- `TypeError.ArgumentType: Circle.position expects Pos, found Tuple` → 要素の数が Pos に合っていない
- `ValueError.DurationRequired` → 0..1 で書いたのに duration が無い、または時刻の単位が混在
- `NameError.UndefinedVariable: "math" is not defined; add "import math"`
- `NameError.UndefinedAttribute: Circle has no attribute "width"` → 属性名の綴り

全部の種別は <https://misebox.github.io/mophila/#/builtins/errors> にある。
