# mophila 言語仕様

言語の意味と規則。できるだけ少ない部品の組み合わせで構成する。

## 1. 字句

```
# これはコメント     # "# " から行末まで
#4080e0            # # の直後に文字が続けば Color リテラル
name_1             # 識別子は英字・数字・_。先頭は英字か _
:center            # Symbol
20vy               # 数の直後に単位が続けば Length リテラル
```

文の区切りは改行で、`{ }` の中も同じ。リテラルは 3 を参照。

`##` で始まる行もコメント。`export` の直前に置くとその項目の説明になる。1 行目が要約で、`@param 名前 説明` `@returns 説明` `@example` が続く。

## 2. 演算子と優先順位

高い順。同じ行は同じ優先順位。

| 優先 | 形 | 結合 | 意味 |
|---|---|---|---|
| 1 | `expr.name` | 左 | 属性参照 |
| 1 | `expr[expr]` | 左 | 添字 |
| 1 | `expr(args)` | 左 | 呼び出し。`Name(args)` なら型の生成 |
| 2 | `-expr` | 右 | 符号反転 |
| 2 | `not expr` | 右 | 否定 |
| 3 | `expr ^ expr` | 右 | べき |
| 4 | `expr * expr` `expr / expr` `expr % expr` | 左 | 乗除、剰余 |
| 5 | `expr + expr` `expr - expr` | 左 | 加減 |
| 6 | `expr .. expr` `expr ..= expr` | 左 | 範囲 |
| 7 | `expr < expr` `expr <= expr` `expr > expr` `expr >= expr` `expr == expr` `expr != expr` | 連鎖 | 比較 |
| 8 | `expr and expr` | 左 | 論理積 |
| 9 | `expr or expr` | 左 | 論理和 |

`args` は `expr, ...`。名前を付けるときは `name = expr`。

添字と `Range` の両端は整数でなければならない。`xs[1.5]` や `0.5..2.5` は `ValueError.OutOfRange` で、黙って切り捨てることはしない。

`%` の符号は割られる数に従う。`-48 % 5` は `-3`、`48 % -5` は `3`。整数でなくてもよく、`7.5 % 2` は `1.5`。0 以上に寄せたいなら `((a % n) + n) % n` と書く。

比較は続けて書ける。`1 < 2 <= 2` は `(1 < 2) and (2 <= 2)` で、真ん中は 1 度しか評価しない。偽が出たらそこで止まる。`2 == 2 == 2` も `true`。

括弧 `( expr )` は優先順位を変える。リテラル (`( )` `[ ]` `{ }`) と `func` `if` `progress` は優先順位を持たない。3 と 4 を参照。

`-expr` は優先 2、`expr - expr` は優先 5。だから `-2 ^ 2` は `(-2) ^ 2` で 4 になる。改行は文の区切りなので、行頭の `- x` は引き算ではなく新しい文。

## 3. 型

### 3.1 基本型

| 型 | リテラル | 備考 |
|---|---|---|
| Number | `1` `1.5` `1/3` `25%` `1e-9` | 整数と実数を区別しない。`100%` = 1.0。`1e-9` は指数表記。3.2 を参照 |
| Duration | `83s` `1m23s` `500ms` `01:23` `01:23:45.678` | 単位は ms / s / m / h。コロン形式は mm:ss と hh:mm:ss、小数秒可 |
| Length | `20vy` `50vx` `3px` | `box` に対して描画時に解決する量。単位は vx / vy / vmin / vmax / px。3.15 を参照 |
| Color | `#rgb` `#rgba` `#rrggbb` `#rrggbbaa` `Color(r, g, b)` `Color(r, g, b, a)` | r/g/b は 0..255、a は 0..1。3/4 桁は各桁を重ねて 6/8 桁に広げる |
| String | `"hello"` `"say \"hi\""` | エスケープは `\"` `\\` `\n` `\t`。`+` で連結。埋め込みはしない。`"n = {n}".format(n)` が `{...}` を順に置き換える (中の名前は説明用)。Dict を渡すと `"{x} {y}".format({x, y})` のように名前で置き換える |
| Bool | `true` `false` | |
| Symbol | `:center` `:linear` | 名前そのものが値。同じ名前どうしだけが等しい |
| Tuple | `(1, 1.5)` | 要素ごとに型が違ってよい。長さは書いたときに決まる。要素 1 つは `(1,)`、空は `()` |
| List | `[1, 2, 3]` | 値を順に並べたもの。要素の型は検査しない。メソッドの署名の `T` は要素の型、`U` は変換後の型 |
| Array | `Array(1, 2, 3)` `Array(list)` | Number だけを詰めた配列。要素ごとの値を持たないので、Rust で作った大きな並びを Rust の関数や `Shader.args` にそのまま渡せる (同じ Array である間は GPU に送り直さない)。作ったら変えない。添字・範囲・for・len は List と同じ |
| Dict | `{ "k": v }` `{ x, y }` | キーは String。挿入順を保つ。`{ x, y }` は `{ "x": x, "y": y }` の省略形 |
| Range | `0..5` `0..=5` | 整数の範囲。`..` は末尾を含まない。両端が整数でなければ `ValueError.OutOfRange` |
| Func | `func (x) { x }` | 型表記は `(引数) -> 戻り値` |
| Type | `Circle` | 型そのもの。呼ぶとその型の値を作る |
| Nothing | | 値を返さなかった関数の戻り値。書き方は無い |

### 3.2 分数

Number は、分数で表せるあいだ分数のまま持つ。実数に落ちるのは、分数で表せない計算をしたときだけ。

```
1/3                    # 1/3
1/3 + 1/3 + 1/3        # 1
1/3 * 3                # 1
2/6                    # 1/3。約分される
0.1 + 0.2 == 0.3       # true。小数も分数として持つ
25%                    # 0.25
```

`+ - * /` と、指数が整数の `^` は分数のまま計算する。`math.sqrt` のように分数で表せない計算が来ると、そこで実数になる。桁が溢れたときも実数になる。

表示は 10 進で書けるならその形。`1/2` は `0.5`、`1/3` は `1/3`。

### 3.3 Func の型表記

型は宣言と同じ形で書く。`func (a: Number, b: Number) -> Number` の型は `(Number, Number) -> Number`。

```
let double: (Number) -> Number = func (x) { x * 2 }
let now:    () -> Duration     = func () { 3s }
let apply:  ((Number) -> Number, Number) -> Number = func (f, x) { f(x) }
```

引数を書かずに `Func` とだけ書いてもよい。可変長は型の後ろに `...` (`(Number...) -> Number`)。builtin の `log` と `type_of` はどの型でも受け取るので、型を書けない。

### 3.4 リテラルと型名

型は大文字で始まる。型名を呼ぶとその型の値ができる (`Vector(8, 4.5)`)。よく使う型にはリテラルがあり、同じ型名の呼び出しと同じ値になる。

| リテラル | 同じもの |
|---|---|
| `()` | `Tuple()` |
| `(1,)` | `Tuple(1)` |
| `(1, 2)` | `Tuple(1, 2)` |
| `[1, 2, 3]` | `List(1, 2, 3)` |
| `{ "k": v }` | `Dict(k = v)` (名前にできないキーは `{ }` だけ) |
| `0..5` | `Range(0, 5)` |
| `0..=5` | `Range(0, 6)` |
| `#4080e0` | `Color(64, 128, 224)` |

同じキーを 2 回書いたら、どちらの書き方でも後が勝つ。

`Number` `Duration` `Length` `Bool` `String` `Symbol` `Func` はリテラルだけで、呼んでは作れない。呼ぶと `TypeError.ArgumentType` で書き方を返す。

builtin の型の名前は予約されていて、`struct` / `record` / `type` で宣言し直すと `NameError.Reserved`。

### 3.5 Union 型

既存の型を `|` で結んだ名前。`type Fill = Color | Gradient`。値は作れず、引数や属性の型として書く。

言語が持つ union: `Shape` (図形すべて)、`Node` (`Shape | Group | Composition | Audio | Narration`)、`Paint` (`Color | Gradient | Shader | Image | World`)、`Font` (`String | List`。候補を並べられる)、`Projection` (space3d のカメラ 3 種)。

型名の後ろに `< >` を書くと、中身の型まで見る。`List<Vector>` は要素が全部 `Vector`、`Dict<String, Number>` は値が全部 `Number`、`Tuple<Number, String>` は 1 つずつ。`Tuple<Number>` のように 1 つだけ書いたら全部その型。`< >` を書かなければ入れ物の型だけを見る。

```
type Points = List<Vector3>    # 形のある型にも名前を付けられる
```

### 3.6 record と struct

違いはイミュータブルかどうか。`record` はイミュータブル、`struct` はミュータブル。それ以外 (フィールド、`func`、`method`、`private`、注釈) は同じ。どちらもトップレベルで宣言する。

```
record Size {
  w: Number                      # フィールドは 名前: 型
  h: Number
  method area(self) -> Number {  # method は第 1 引数が受け手。使わないなら _
    self.w * self.h
  }
}

let a = Size(3, 4)               # 既定値の無いフィールドは、宣言順に位置で渡せる
let b = Size(w = 3, h = 4)       # 名前で渡すときは 名前 = 値
a.area()                         # 12
```

```
struct Counter {
  n: Number = 0                  # = 式 で既定値
  private step: Number = 1       # private なフィールドは外から読み書きできない
  func new(start: Number) -> Counter {   # 型名を呼んだときの作り方。受け手を取らない
    Counter(n = start)           # func new の中で型名を呼ぶと、フィールドから作る
  }
  private func twice(x: Number) -> Number { x * 2 }   # private は func と method にも付く
  method next(self) -> Number {
    self.n = self.n + self.step
    self.n
  }
}

let c = Counter(10)              # func new が呼ばれる
Counter.new(10)                  # 型名から直接呼んでも同じ
c.next()                         # method は受け手を書かずに呼ぶ
```

`func new` を書かなければ、`private` でないフィールドを宣言順に受け取る作り方になる (上の `Size`)。`private` で既定値の無いフィールドがあるとそれでは作れないので、`func new` が要る。

書き換えと複製:

```
let a = Size(3, 4)
let b = a
a.w = 9        # a に新しい Size を入れ直す。b は Size(3, 4) のまま
b.copy(w = 9)  # 入れ直さずに、w だけ違う Size を作る (func new を書いた型では、フィールドを変える copy はできない)

let c = Counter(0)
let d = c
c.next()          # 実体そのものが変わるので、d から見ても n は 1
c.shallowCopy()   # 属性を写した別の実体。中の struct は元と同じものを指す
c.deepCopy()      # 中の record と struct もたどって写した別の実体
```

record のフィールドへの代入は、その値を変えるのではなく、変数や属性に新しい record を入れ直す。だから同じ record を持っている他の変数は変わらない。関数の引数に代入しても、呼んだ側には伝わらない。

同じ名前の `func` / `method` を宣言すれば、`copy` などより先にそちらが使われる。

### 3.7 注釈

宣言の前の行に `@名前` を書く。複数は空白で並べる。

| 注釈 | 意味 |
|---|---|
| `@immutable` | `struct` をイミュータブルにする (record と同じ扱いになる) |
| `@nocopy` | `copy` `shallowCopy` `deepCopy` をすべて禁止する |
| `@nodeepcopy` | `deepCopy` だけ禁止する |
| `@deprecated("説明")` | その型を最初に作ったときに stderr へ警告を出す |

```
@deprecated("Vector を使う") @nocopy
record Vec2 {
  x: Number
  y: Number
}
```

### 3.8 別名

`alias V = Vector` で、長い型名に短い名前を付ける。そのファイルの、その行より後ろで使える。`export` はできない。

### 3.9 決まった値しか取らない型

属性がこの型なら、その型に載っている名前しか書けない。値はすべて Symbol で、型を呼んでは作れない。違う名前を書くと `ValueError.OutOfRange` が、取れる値を並べて出る。

| 型 | 何を決めるか | 取れる値 (先頭が既定) |
|---|---|---|
| Align | TextArea の行の寄せ方 | `:left` `:center` `:right` |
| StrokeCap | 線の両端の形 | `:butt` `:round` `:square` |
| StrokeJoin | 折れ曲がった線の、角の形 | `:miter` `:round` `:bevel` |
| Blend | 下にある絵との混ぜ方 | `:normal` `:multiply` `:screen` `:overlay` `:darken` `:lighten` `:difference` `:add` |
| GradientKind | Gradient の stops を並べる向き | `:linear` `:radial` `:sweep` |
| Easing | 区間の値の変わり方 (`progress` の値の隣にだけ書ける) | `:linear` `:ease_in` `:ease_out` `:ease` |

同じものを自分でも書ける。

```
type Mode = :fast | :slow
```

### 3.10 位置

- Vector: 2 つの成分。成分は Number か Length (`Vector(50vx, 40vy)`)。`.x` `.y`。`Vector ± Vector`、`Vector × Number`、`Vector ÷ Number`、`-Vector`
- `position` と `anchor` は別の属性で、どちらも Vector。`position` は `anchor` を置く場所で、親の座標で書く。`anchor` は Node 自身の座標で見た基準点で、`rotation` と `scale` の中心になる。3.11 を参照
- `(8, 4.5)` は Tuple。ただし**型の決まっている場所に書いた Tuple は、その型になる** (型注釈、属性、フィールド、引数、戻り値)。`Circle(position = (8, 4.5))`、`Polygon(points = [(0, 0), (1, 0)])`、`let ps: List<Vector3> = [(0, 0, 0)]` と書ける
- 変換するのは Tuple から `Vector` `Vector3` `Color` へだけ。型どうしの変換は無く、数が合わなければ型エラー
- 型の決まっていない場所では Tuple のまま。まとめて作るなら `Vector.from([(0, 0), (1, 0)])` (`Vector3` `Color` も同じ)

### 3.11 図形

図形は不変の record。変えるときは `copy(...)` で別の値を作るか、関数で作り直す。

```
let big = dot.copy(radius = 4vy)
```

| 型 | その型だけの属性 |
|---|---|
| Circle | `radius: Length` |
| Ellipse | `rx: Length` (横の半径) `ry: Length` (縦の半径) |
| Rect | `w: Length` `h: Length` `radius: Length` (角の丸み。省略は 0) |
| Line | `from: Vector` `to: Vector` |
| Polygon | `points: List<Vector>` |
| Path | `from: Vector` `segments: List<Tuple>` `closed: Bool` `upto: Number` |
| TextArea | `text: String` (`\n` で改行) `w: Length` (折り返す幅) `font: String` `fontSize: Length` `align: Align`。`size()` で置いたときの幅と高さ (Vector) を返す |

長さを表す属性 (`radius` `w` `h` `strokeWidth` など) は Length を取る。単位なしの数は `box` 単位。

図形は `length()` で輪郭の長さを返す。`dash` と `dashOffset` で線を少しずつ描き出すときに使う。

`Path` の `segments` は `(:move | :line | :quad | :curve, 点...)` の並び。`:quad` は制御点 1 つ、`:curve` は 2 つを、終点より前に書く。`closed = true` なら始点に戻って閉じる。`Path.through(points, closed =)` は点の並びをつないだ Path を作る。

`upto` は「`segments` の先頭から何割を描くか」(0..1、既定は 1)。点の列はそのままに、描く範囲だけ動かせる。最後の 1 区間は途中で切るので、線の先は滑らかに伸びる。`p.point_at(1)` がその先の点なので、ペン先を置くものを合わせられる。`upto` が 1 未満なら閉じない。

```
let drawn = line.copy(upto = t)
let pen = dot.copy(position = drawn.point_at(1))
```

すべての Node が、次の transform 一式を持つ。

| 属性 | 型 | 意味 |
|---|---|---|
| `anchor` | Vector | Node 自身の座標で見た基準点。`rotation` と `scale` の中心 |
| `position` | Vector | `anchor` を置く場所。親の座標で書く |
| `scale` | Number | 拡大率 |
| `rotation` | Number | 回転。ラジアン、時計回りが正 |
| `opacity` | Number | 不透明度 0..1 |

座標は x が右、y が下に増える。Node 自身の座標の原点と `anchor` の既定は型で決まる。

| 型 | 自身の原点 | `anchor` の既定 |
|---|---|---|
| Rect / TextArea | 左上。大きさは `w` × `h` | 自身の中心 `(w/2, h/2)` |
| Circle / Ellipse | 中心 | `(0, 0)` |
| Line / Polygon / Path | 点の座標がそのまま。原点は `(0, 0)` | `(0, 0)` |
| Group | 子の座標系の `(0, 0)` | `(0, 0)` |

`anchor` が既定なら `position` は Rect / TextArea の中心になる。Rect を左上に置くなら `anchor = Vector(0, 0)`。

図形はこれに加えて次の属性を持つ。効きようがないものは、その図形が持たない (書くと `NameError.UndefinedAttribute`)。既定のあるものは、書かなくてもその値で読める:

| 属性 | 型 | 意味 |
|---|---|---|
| `fill` | Paint | 塗り。Color / Gradient / Shader / Image |
| `stroke` | Color | 線の色 |
| `strokeWidth` | Length | 線の太さ |
| `strokeCap` | StrokeCap | 線の端の形 |
| `strokeJoin` | StrokeJoin | 線の角の形 |
| `dash` | List | 破線。線と間の長さを順に並べた Number |
| `dashOffset` | Number | 破線の始まりをずらす長さ |
| `blend` | Blend | 下の絵との重ね方 |

| 持たない図形 | 属性 | 理由 |
|---|---|---|
| Line | `fill` | 面が無い |
| Circle / Ellipse / Line | `strokeJoin` | 角が無い |
| TextArea | `strokeCap` `dash` `dashOffset` | 字の輪郭を破線にできない |

**Group** — Node をまとめた親。Group も不変で、transform 一式 (上の表) を持つ。中の座標は Group の `position` からの相対。子は親の `rotation` `scale` `opacity` を受けるので、Group を動かせば中身ごと動く。

```
let badge = Group(
  Circle(radius = 1.4, fill = #2f6fd0),
  Rect(w = 1.2, h = 1.2, fill = #ffb583),
  position = Vector(8, 4.5),
)
let moved = badge.copy(position = Vector(12, 4.5))   # 丸と四角が一緒に動く
```

`camera` (Camera、3.13)、`clip` と transform 一式でない引数が中身で、書いた順に描く。中身には図形と Group を入れられる。

`clip` は中身を切る図形 (マスク)。その輪郭の外にある中身は描かれない。図形の `position` や `anchor` は Group の座標で効き、`camera` で寄っても切り口は動かない。

```
Group(Circle(radius = 20vy, fill = #e04040), clip = Rect(w = 50vx, h = 100vy))
```

親子の連鎖は入れ子の深さで書く。順序に依存して親を決める記法は無い。

```
import { sin, tau } from math

func leg(t) {
  let s = sin(tau * t / 1s)
  Group(position = Vector(50vx, 40vy), rotation = 0.5 * s,
    Rect(anchor = Vector(1vx, 0), w = 2vx, h = 20vy, fill = #3060c0),
    Group(position = Vector(0, 20vy), rotation = 0.4 - 0.4 * s,
      Rect(anchor = Vector(1vx, 0), w = 2vx, h = 20vy, fill = #3060c0),
      Group(position = Vector(0, 20vy), rotation = -0.3 * s,
        Rect(anchor = Vector(1vx, 0), w = 6vx, h = 2vy, fill = #3060c0))))
}
```

外側の Group の回転が内側の 2 つを動かし、2 つ目の Group の回転が最後の Group を動かす。

Composition は箱 (`box`) と長さ (`duration`) を持つ。3.12 と 3.14 を参照。

### 3.12 箱

```
Composition(box = Vector(16, 9), duration = 6s)
```

| 属性 | 型 | 意味 |
|---|---|---|
| `box` | Vector | 座標系の幅と高さ。`Vector(16, 9)` なら 0..16 × 0..9。ピクセルは持たない。Length の基準 (3.15) |
| `duration` | Duration | 長さ |

Composition は Node でもあるので、transform 一式 (3.11) を持つ。別の Composition に置かれると、その中の座標と Length は自分の `box` が基準になる。

### 3.13 塗り

**Camera** — 不変の record。Group の `camera` に渡すと、Group の**中身**の `from` が `to` の位置に来るように置いて `scale` 倍する。Group の枠 (`position` `rotation` `scale`) は `camera` とは別で、動かない。

```
Group(circle, rect, camera = Camera(from = Vector(3, 2.5), to = Vector(8, 4.5), scale = 10))
```

| 属性 | 型 | 意味 |
|---|---|---|
| `from` | Vector | 中身のどの点を映すか |
| `to` | Vector | それを Group のどこに置くか。省略は `from` と同じ = 中身を動かさずに拡大だけする |
| `scale` | Number | 倍率。省略は 1。0 以下は `ValueError.OutOfRange` |

寄ると中身は枠からはみ出す。枠で切るには、同じ Group の `clip` に枠の図形を渡す。Composition の `box` の外は出力に出ない。

3 つを別々の属性にせず 1 つの型にしてあるのは、途中で食い違って画面が跳ねないようにするため。動かすときは、`func (t)` の中で毎フレーム新しい `Camera` を作る。

```
import { exp, ln } from math

let len = 6s

func zoom(t) {
  Group(circle, rect, camera = Camera(from = Vector(3, 2.5), scale = exp(ln(1e12) * t / len)))
}
```

`fill` に入れられるのは `Color` `Gradient` `Shader` `Image` `World` (`Paint`)。

```
Gradient(from = Vector(0, 0), to = Vector(16, 9), stops = [#4080e0, #e0604a])
```

| 属性 | 型 | 意味 |
|---|---|---|
| `kind` | GradientKind | 広がり方。省略は `:linear` |
| `from` | Vector | `:linear` の始点、`:radial` と `:sweep` の中心 |
| `to` | Vector | `:linear` の終点 |
| `radius` | Number | `:radial` の半径 |
| `stops` | List | Color を 2 つ以上。等間隔に並ぶ |

```
Shader(color = func (x, y, t) { Color(x * 16, 0, 128) })
```

| 属性 | 型 | 意味 |
|---|---|---|
| `color` | (Number, Number, Number) -> Color | 箱の座標 x, y と動画の時刻 t 秒から、そのピクセルの色を返す |
| `args` | Array か List | Number の並び。`color` の 4 つ目の引数として渡る。Array なら同じものである間は GPU に送り直さない |
| `samples` | Number | 1 ピクセルあたりの評価点の数。平方数に切り上げ、4 なら 2x2 の平均。省略は 1 |
| `zoom` | ZoomPath | ズーム動画の道筋。入れると `color` の引数の意味が変わる |

`color` は描画のたびに GPU で全ピクセル分走るので、書けるものが限られる。

書けるもの:

- `Number` `Bool` `Color` `Vector`。Vector は `Vector(x, y)` と `.x` `.y`、`+ -`、Number との `* /`
- 四則、`%`、`^`、比較、論理、`if`、範囲の `for`、`return`
- `math.*` と `Color(...)`
- 外側で定義した `Number` `Color` `Bool` `Vector` と、そのどれか 1 種類だけの `List`
- 外側で定義した関数 (再帰は不可)

書けないもの: 文字列、Duration、図形、Dict、素の Tuple。

大きな表は Array で渡す (List は毎フレーム写して送るが、Array は同じものである間は送らない)。`run` では走らず、`render` `preview` `sheet` で走る。GPU の実数は 32 bit。

**ZoomPath** — 1 点へ寄っていくだけのズーム動画は、(中心からの距離の対数, 角度) で見るとどのフレームも同じ絵の平行移動になる。`Shader.zoom` に `ZoomPath` を入れると、1 フレームずつ描かずに、その座標系の帯を伸ばしながら使い回す。帯は 1 フレームあたり数列しか伸びないので、1 枚ずつ描くより桁違いに速い。

```
Shader(zoom = ZoomPath(center = Vector(8, 4.5), zoom = func (t) { t + 1 }, duration = 2m), color = func (u, th, t) { ... })
```

| 属性 | 型 | 意味 |
|---|---|---|
| `center` | Vector | 箱の座標での、寄っていく先 |
| `zoom` | Func | 倍率の時間変化。`func (t) -> Number`。t は秒 |
| `duration` | Duration | `zoom` を読む範囲 (動画の長さ) |
| `unit` | Number | 倍率 1 のときの、箱の座標 1 あたりのシェーダの座標の長さ (省略は 1) |

`zoom` を入れると `color` の引数は `(中心からの距離の対数, 角度, 時刻)` になる。倍率は増えていくこと (戻ると帯を作り直すので遅い)。手で組むものではなく、標準ライブラリ `fractal` の `escape_time` が `duration` を書いたときに組み立てる。

`escape_time` の `center` を 10 進の文字列 2 つで渡すと、倍率が 32 bit にも 64 bit にも収まらない深さまで行ける。中心の点の軌道 (基準軌道) を多倍長で計算し、GPU は指数を別に持つ数でそこからの差分を回し、線形近似 (BLA) で反復をまとめて飛ばす。倍率そのものは扱えないので `max_steps` には倍率の log10 が渡る。寄る先は `find_center` で探せる (その点の近くにあるミニチュアの中心・周期・大きさを返す)。

**World** — `space3d` の 3D の場面。図形の `fill` に入れると、その図形の中に GPU が `Mesh` をそのまま描く。面を 1 枚ずつ図形に開く書き方と違って、1 コマにスクリプトがするのは `Solid` の置き方を変えることだけなので、面が何万枚あっても 1 コマの手間は変わらない。前後は深度で決まる。

```
let ball = space3d.Solid(mesh = space3d.sphere((0, 0, 0), 1, 16, 32), fill = #6fa3d6)
comp.place("world", Rect(position = Vector(0, 0), anchor = Vector(0, 0), w = 16, h = 9,
                         fill = space3d.World(camera = cam, parts = [ball], background = #101520)),
           in = 0s, out = comp.duration)
```

| 属性 | 型 | 意味 |
|---|---|---|
| `camera` | Projection | space3d のカメラ。`box` は塗る図形の大きさに合わせる |
| `parts` | List<Solid> | 描くもの |
| `light` | Vector3 | 光の向き。省略は `(0.4, 0.8, 0.5)` |
| `ambient` | Number | 光に背いた面の明るさ。省略は 0.25 |
| `background` | Color | 何も無いところの色。省略は透明 |

`Solid` は形と色と置き方の組。

| 属性 | 型 | 意味 |
|---|---|---|
| `mesh` | Mesh | 形。`space3d.box` などが返すもの。いくつもの `Solid` で使い回せる |
| `fill` | Color | 塗り。面の向きと `light` で陰が付く。省略は `#808080` |
| `transform` | Transform3 | 置き方。省略は `Mesh` の座標のまま |

こちらを向いていない面は描かない。`Mesh` の `edges` は使わない (線は引かない)。`run` では走らず、`render` `preview` `sheet` で走る。

### 3.14 時間

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
- **Node** は描かれるものの総称。図形、Group、Composition、Audio、Narration
- **Layer** は Composition に置かれたもの 1 つを指す言葉で、言語の型としては出てこない。名前と、置いたものと、`in` と `out` を持つ。`in` と `out` は親 Composition の時間軸で数える
- `in <= t < out` の外では、その Layer は存在しない。描かれず、音も鳴らず、何も計算しない。残したければ `out` を伸ばす
- Layer は束ねられない。別々の時刻に出入りするものをまとめたいときは、Composition を作って 1 枚の Layer として置く

**place**

```
comp.place("badge", badge, in = 2s, out = 8s)
```

- 1 つ目が名前、2 つ目が置くもの。`in` と `out` は必須
- 名前は String で、その Composition の中で一意。同じ名前を 2 度置いたらエラー
- 積み重ねの順は置いた順。後に置いたものが手前
- `place` は値を返さない
- 置いた Layer は取り除けない。置き換えも、`in` と `out` の変更もできない。消えることは `out` で表す

**Layer の中身** — 置けるものは 2 種類。

- 不変の Node。`Circle(...)` `Group(...)` `Composition(...)` `Audio(...)` `Narration(...)`
- 純粋関数 `func (t) -> Node`

`t` は Layer の `in` を 0 とした時刻 (Duration)。関数は毎フレーム、新しい不変の図形を返す。代入は無く、状態は持たない。同じ `t` には同じ Node が返る。

```
func pop(t) {
  Circle(position = center, radius = 10vy, fill = #e04040, scale = ease_out(t, 0s, 0.4s, 0, 1))
}

comp.place("pop-a", pop, in = 1s, out = 3s)
comp.place("pop-b", pop, in = 4s, out = 6s)
```

`pop` の `t` は、どちらの Layer でも自分の `in` が 0。

**progress** — 対象も長さも持たない、名前付きの表。0 から 1 の数を渡して呼ぶと、その点の値を Tuple で返す。行の間は補間する。

```
let slide = progress [:x, :y] {
  0:   2, 4.5
  0.4: _, 6 :ease
  1:  14 :ease_out, 4.5
}
let (x, y) = slide(0.5)
```

- 行の左は 0 から 1。0 と 1 の行は必須
- 列の名前を先に並べ、行には値だけを書く
- `_` はその列を飛ばす。名前ごとに別々の位置でキーフレームを打てる
- Easing (3.9) は値の隣に書く。書かなければ `:linear`。`:ease_in` は加速、`:ease_out` は減速、`:ease` は両方
- 0 より小さい数を渡すと 0 の行の値、1 より大きい数を渡すと 1 の行の値を返す
- 何秒かけるかは呼ぶ側が決める。`slide(t / 0.3s)` なら 0.3 秒で端から端まで動く

**補間関数**

```
ease(t, t0, t1, v0, v1)
```

`t` が `t0` から `t1` の間にあるとき `v0` から `v1` へ補間する。区間の外では端の値に止まる。引数の並びは After Effects の `ease` と同じ。`linear` `ease_in` `ease_out` も同じ形。`v0` と `v1` は Number、Vector、Length を取れる。`t` `t0` `t1` は Duration。

丸が動き、棒が伸び、文字が出て、最後に消える。

```
let len = 6s

func stage(t) {
  let x = ease(t, 0s, 2s, 10vx, 80vx)
  let bar = ease_out(t, 1s, 2.5s, 0vx, 60vx)
  let shown = ease(t, 2.5s, 3s, 0, 1)
  let fade = ease(t, len - 1s, len, 1, 0)
  Group(
    Circle(position = Vector(x, 30vy), radius = 4vy, fill = #e04040),
    Rect(position = Vector(10vx, 60vy), w = bar, h = 2vy, fill = #000000),
    TextArea(text = "Done", position = Vector(10vx, 75vy), opacity = shown),
    opacity = fade
  )
}

let comp = Composition(box = Vector(16, 9), duration = len)
comp.place("stage", stage, in = 0s, out = len)
output comp
```

**描画** — 1 コマは、時刻を入れて木をたどる。

1. その時刻に `in <= t < out` の Layer を、積み重ねの順に見る
2. 置いたものが関数なら `t - in` で呼んで Node を得て、描く
3. 置いたものが Node なら、そのまま描く
4. 置いたものが Composition なら、同じことを再帰でやる

各フレームは、その時刻だけで決まる。前のフレームには依存しないので、preview で時間を戻しても同じ画になる。

動画の長さは、`output` した Composition の `duration`。

**音声** — `import "bgm.m4a" as bgm` で得た Audio を Node として置く。`volume` は 1 がそのまま、`loop = true` は `out` まで繰り返す。音声は `out` で切る。繰り返さなければファイルより長くならない。同じ音声を何度でも置けて、重なれば混ざる。

```
comp.place("bgm", bgm.copy(volume = 0.6, loop = true), in = 0s, out = 30s)
```

読み上げは `Narration`。文だけを持ち、画面には描かない。長さは Layer の `in` から `out`。**その文を読んだ音声が動画の音になり、同じ文が字幕になる**。**どう喋らせるか**は `voice` で渡す。

```
let kyoko = SayVoiceEngine(voice = "Kyoko")
comp.place("line-1", Narration(text = "ここを読み上げる", voice = kyoko), in = 2s, out = 5s)
comp.place("line-2", Narration(text = "ここは速く", voice = SayVoiceEngine(voice = "Kyoko", rate = 300)), in = 5s, out = 8s)
```

`voice` に入れるのは音声合成の型 (`Voice`)。合成ごとに受け取る設定が違うので、型も別になっている。

| 型 | 何を使うか | 属性 |
|---|---|---|
| `SayVoiceEngine` | macOS の say | `voice` (`say -v ?` の名前) `rate` (1 分あたりの語数) |
| `EspeakVoiceEngine` | espeak-ng | `voice` (`espeak-ng --voices` の名前) `speed` `pitch` `gap` |

属性はその型のものだけ。無い名前を書けば `NameError.UndefinedAttribute` で止まる。`voice` を渡さなければ、その機械に入っているもので読む。`volume` で音量を変えられる。

合成した音声が `in` から `out` より長ければ、`out` で切って警告を出す。`in` と `out` は動かない。

字幕は読み上げからしか作らない。出るのは読み始めから読み終わりまでで、長さは作った音声から測る。`out` の見積もりがずれても、字幕と声はずれない。声を付けない字幕は書けない (書けるようにすると、また人が時刻を見積もることになるため)。

作った音声はビルドの中間物なので、リポジトリではなくキャッシュ (`~/.cache/mophila/voice/`、`$XDG_CACHE_HOME` があればその下) に置く。同じ型・同じ設定・同じ文なら作り直さない。書いたものが音声として読めなければ、そこで止まる (壊れたものをキャッシュに残さない)。

`render` は音声を ffmpeg で動画の音声トラックにし、字幕を SRT にして字幕トラックに入れる (mp4 と mov は mov_text、webm は webvtt、mkv は srt)。

`--trim` が時間の切り取りで、`--crop` が領域の切り取り。`--trim` の時刻は `00:15` `1.5s` `500ms` の他に、`94%` と割合でも、
`10978` とコマ数でも書ける (単位も `:` も無ければコマ数。`--fps` で秒に直す)。
`94%..96%` や `2s..50%` のように混ぜてもよい。`--crop height` は高さを箱いっぱいにして幅を出力の比から決め、
`--crop width` はその逆、`--crop 0.25,1` (`25%,100%` でも同じ) は箱に対する割合で直に決める。
切り取った範囲が箱より小さいとき、余ったところのどこに寄せるかが `--align` で、
`left` `center` `right` `top` `bottom` `topLeft` などの名前か、軸ごとの割合 (`30%,50%`) を書く (既定は中央)。
どちらも箱の大きさを知らずに書ける。絵が画面を埋めないときの帯の色は `--pad '#000000'` (既定は白)。

書き出した動画には `comment=mophila <版>` が入る (`ffprobe -show_entries format_tags=comment` で見える)。入れ物が対応していない png や gif には入らない。

`subs` は動画を描かずに字幕だけをファイルに出す (`-o subs.srt` / `-o subs.vtt`。省略すると標準出力)。中身は動画の字幕トラックと同じで、`--trim` も同じように効く。`preview` は音声を PCM にして鳴らし、字幕を画面の下に重ねる。`sheet` も重ねる。重ねる字幕は、箱を収めた絵の中に出す (ウィンドウの縦横比が箱と違えば、余った帯には出ない)。

### 3.15 Length

Duration と同じ作りの、型つきの遅延量。単位は次の 5 つ。

| 単位 | 意味 |
|---|---|
| `vx` | `box` の幅の 1/100 |
| `vy` | `box` の高さの 1/100 |
| `vmin` | `vx` と `vy` の小さい方 |
| `vmax` | `vx` と `vy` の大きい方 |
| `px` | 出力のピクセル |

- 描く先の Composition の `box` に対して、描画時に解決する。入れ子の Composition では、それぞれの `box` が基準になる
- 単位なしの数は `box` 単位
- Vector の成分は Length を取れる。`Vector(50vx, 40vy)`
- 加減算、Number との乗除算、`ease` などの補間が Length に対応する

### 3.16 角度

角度は Number のラジアン。`rotation` も `polar` も `sin` も同じ単位で、時計回りが正。

- `rotation` の単位は度でなくラジアン
- math の関数は、角度の向きを `rotation` と同じ時計回りに揃える。既存の関数への適用は未決 (10 章)

### 3.17 名前

- `center` は裸の名前で、描く先の Composition の中心を指す Length の Vector。入れ子の Composition の中では、その Composition の中心
- `tau` は小文字の定数。`polar(r, a)` と共に math から import する
- `polar(r, a)` は半径 `r`、角度 `a` の Vector を返す。`r` は Length を取れる

```
import { tau, polar } from math
center + polar(20vy, tau / 6)
```

## 4. 文

### 4.1 束縛と代入

```
let n = 1                # 束縛
let d: Duration = 2s     # 型を書いてもよい。実行時に確かめる
let (a, b) = (1, 2)      # Tuple を分解して束縛
let [p, q] = [10, 20]    # List も分解できる

n = 2                    # 再代入
a, b = b, a              # 右辺を全部評価してから代入するので、入れ替えになる
c.radius = 3             # 属性
xs[0] = 9                # 添字
d["k"] = 1               # Dict のキー
```

`let` を書かずに初めて代入すると `NameError.AssignWithoutLet`。同じスコープで `let` を書き直して束縛し直すことはできる。

### 4.2 条件と繰り返し

`if` は式で、ブロックの最後の式が値になる。

```
let sign = if n > 0 { "正" } else if n < 0 { "負" } else { "零" }

for i in 0..3 { }                              # 0, 1, 2
for i in 0..=3 { }                             # 0, 1, 2, 3
for c in ["赤", "青"] { }                       # "赤", "青"
for (i, c) in ["赤", "青"].enumerate() { }      # (0, "赤"), (1, "青")
for (k, v) in { "赤": 1, "青": 2 } { }          # ("赤", 1), ("青", 2)
```

`for` が回せるのは Range、List、Tuple、Dict。String は回せない。

回数が先に決まらない繰り返しは `while`。

```
while total < target {                         # 条件が偽になるまでくり返す
  total = total + step
}
```

`break` は `for` と `while` のどちらもその場で抜ける (`continue` は無い)。

`while` が 100 万回回っても条件が偽にならなければ `RuntimeError.EndlessLoop`。1 フレームを組む間の回数なので、
まともな探索がこの回数を要ることは無い。止まらない書き方をしたまま描き続けるより、そこで知らせる。
Shader の中では `while` は使えない (回数の決まらない繰り返しは GPU ごと固まるため)。`for` と `break` で書く。

### 4.3 関数

ブロックの最後の式が戻り値になる。

```
func area(w: Number, h: Number = 1) -> Number {   # = 式 で既定値
  w * h                                           # この式が戻り値
}

area(2)               # 2。h は既定値の 1
area(h = 3, w = 2)    # 6。名前で渡すときは 名前 = 値
```

`...` を付けると、並びをそのまま引数に広げる。Tuple / List / Range は位置の引数に、Dict は `キー = 値` の名前付き引数になる。名前は付けられない (`f(x = ...xs)` は書けない)。

```
let wh = (2, 3)
area(...wh)                    # 6。area(2, 3) と同じ
area(...[2], 5)                # 10。ほかの引数と混ぜてよい
area(...{ "w": 2, "h": 4 })    # 8。名前で渡したのと同じ
```

`return` は途中で抜けるときに書く。`if` や `for` の中からでも関数を抜ける。

```
func first_even(xs: List<Number>) -> Number {
  for x in xs {
    if x % 2 == 0 {
      return x
    }
  }
  -1        # 見つからなければこの式が戻り値
}
```

### 4.4 型の宣言

```
record Size { }        # 3.6
struct Counter { }     # 3.6
@nocopy                # 3.7。直後の record / struct に付く
alias V = Vector       # 3.8。そのファイルのその行より後ろだけ
type Fill = Color | Gradient   # 3.5
```

`record` `struct` `type` はトップレベルにだけ書ける。

### 4.5 import

`import` は 3 つある。何を束縛するかが違う。

```
import math                        # 標準ライブラリ。Module を math に束縛する
import fractal as f                # as で別名を付ける
import .orbit                      # 同じ場所の orbit.moph。Module を orbit に束縛する
import .lib.shapes                 # ./lib/shapes.moph
import "path/file.moph" as slides  # パスで書く形。as が無ければファイル名
import { a, b } from .slides       # Module を作らず、export した名前をここに持ち込む
import { sin } from math           # 標準ライブラリからも同じ
import "bgm.m4a" as bgm            # 音声。Audio を束縛する
import "logo.png" as logo          # 画像。Image を束縛する (png jpg gif webp bmp tiff)
```

`.moph` の Module から見えるのは、そのファイルが `export` した名前と、`output` した Composition (`orbit.output`) だけ。

同じファイルは 1 度しか実行されず、2 度目以降の `import` は同じ実体を返す。循環 import はエラー。`bundle` は音声も含めて import 先を実行ファイルに埋め込む。

`import ..parent` で 1 つ上のディレクトリも書けるが、ディレクトリをまたぐ設計は避ける。標準ライブラリを import せずに使うと、`NameError.UndefinedVariable` に `add "import math"` のヒントが付く。

`@` で始まる形は、プロジェクト設定 (7 章) の別名を指す。`@.` は root、`@名前.` はそこで決めた場所。

```
import @.slides                    # <root>/slides.moph
import @parts.box                  # <parts>/box.moph
import { card } from @.theme
import "@/media/bgm.m4a" as bgm    # 文字列の中では "/" 区切り
```

`import` はトップレベルにしか書けない。

### 4.6 export

```
export let theme = #4080e0
export func run(now: Duration) -> Duration { now }
export struct Card { }
export record Size { }
```

トップレベルにだけ書ける。`alias` は export できない。

### 4.7 output

```
output comp   # この Composition が動画になる。1 つのファイルに 1 つ
```

## 5. 空間

- 箱 (Composition) は `box = Vector(w, h)` で座標系を宣言する。中の座標はその単位。`Vector(16, 9)` なら 0..16 × 0..9
- 座標は意味のある式で書く。`center` は描く先の Composition の中心 (3.17)
- 単位なしの数は `box` 単位。`20vy` のような Length は描画時に `box` から解決する (3.15)
- `comp.place(name, node, in =, out =)`。Node の位置は Node の `position` と `anchor` で決まる (3.11)
- Composition を別の Composition に置くと入れ子になる。子の座標と Length は子の `box` で解釈され、`center` は子の中心を指す
- 角度はラジアンで、時計回りが正 (3.16)

## 6. builtin と標準ライブラリ

`import` なしで使えるのは、builtin の型の名前と、次の名前だけ。関数は呼び出しの形でしか書けず、値として取り出すことはできない。

| 名前 | 内容 |
|---|---|
| `log(値, ...)` | 引数を空白区切りで stderr へ出す。どの型でも受け取る。動画には出ない |
| `type_of(値)` | その値の型の名前を String で返す |
| `ease(t, t0, t1, v0, v1)` `linear` `ease_in` `ease_out` | 区間の補間。3.14 を参照 |
| `center` | 描く先の Composition の中心。3.17 を参照 |

String / List / Dict / Range のメソッドは builtin で、`import` は要らない。

`import` で使う標準ライブラリ。

| モジュール | 内容 |
|---|---|
| math | 数学。`tau` `pi` `e` の定数と、三角関数・丸め・平方根・対数・最大最小・範囲の変換。角度はラジアンで、向きは時計回りが正 (3.16)。`polar(r, a)` は半径 `r`、角度 `a` の Vector を作る |
| color | 色を作る、混ぜる。明るくする、暗くする、不透明度を変える、色相から作る |
| shape | Polygon の `points` を作る。円周上の点、正多角形、星、矢印 |
| layout | 並べる位置を計算する。格子、直線上の等間隔、比率を保って収めた大きさ |
| palette | そのまま使える配色。背景・文字・罫線・強調・系列色の組 (`Palette`) を名前で選ぶ |
| pattern | そのまま `fill` に入れられる模様。縞、細線、市松、水玉、方眼、ざらつき |
| icon | チェックや矢印などの記号の形。`mark` でそのまま置ける図形になる |
| space3d | 3D の点 (`Vector3`) と変換 (`Transform3`)、カメラ 3 種、`Mesh` の作り手。投影して 2D の `Vector` と奥行きの `Number` を返し、描くのは既存の図形。`World` を図形の `fill` に入れれば、面を図形に開かず GPU が `Mesh` のまま描く |
| fractal | エスケープタイム系フラクタルの Shader。反復式と色付けを組み合わせる。倍率は `zoom` / `duration` / `camera` のどれかで決める (`camera` は Camera を受ける)。中心を文字列で渡すと多倍長の基準軌道で深くまで寄れる。`find_center` は寄る先 (ミニチュアの中心) を探す |
| bignum | 多倍長の計算。`fractal` が使う基準軌道と BLA の表、ミニチュアの探索。Rust でループを回すので、深いズームの準備がスクリプトで待たされない |

## 7. プロジェクト設定

`mophila.yaml` を、コマンドを実行したディレクトリに置くと自動で読まれる。
スクリプトの代わりに**そのフォルダか `mophila.yaml` 自身を渡しても読む** (`mophila render project/`、`mophila run project/mophila.yaml`)。
このときのスクリプトは設定の `entry`。`-f` は、設定とスクリプトが別の場所にあるときに使う (`-f ../shared.yaml scenes/main.moph`)。
設定に書いたパスは設定ファイルのある場所からの相対なので、どこから実行しても同じものを指す。

```yaml
entry: scenes/main.moph      # スクリプトを省略したときに使う
root: .                      # @/ と @. が指す場所
aliases:
  parts: src/parts           # @parts. が指す場所
config:
  width: 16                  # import config で読める値
  title: "mophila"
  debug: false
render:
  size: 4k                   # render の既定。コマンドラインに書いたほうが勝つ
  fps: 60
  codec: hevc_nvenc
  codec-args: -cq 20 -b:v 0
```

**設定の中に書いたパスは、設定ファイルのある場所からの相対**。コマンドラインに書いたパスは、いま居るディレクトリからの相対。設定はプロジェクトの形を書いたものなので、どこで実行しても同じ場所を指す。

`render` に書けるのは `fps` `size` `jobs` `codec` `pix-fmt` `codec-args` `gpu-budget` `crop` `align` `pad`。
値はコマンドラインに書くのと同じ形で、コマンドラインに書いたほうが勝つ。書かなかった名前は既定のまま (何も言わない)。
知らない名前と読めない値のときだけ、そう言ってその項目を捨てる。

描き始める前に、効いた指定を 1 行出す。括弧の中は、そのうち設定から取ったもの。

```
render 3840x2160 60fps hevc_nvenc p010le -cq 20 -b:v 0 -> out.mp4   (size fps codec pix-fmt codec-args from mophila.yaml)
```

`config` の値は数・文字列・真偽の 3 つ。`import config` で読む。

```
import config
let comp = Composition(box = Vector(config.width, 9), duration = 6s)
```

値は 2 通りで上書きできる。どちらも設定に書いた型に合わせて読み、合わなければエラー。設定に無い名前を指定してもエラーになる。

| 書き方 | 例 |
|---|---|
| 環境変数 | `MOPHILA_WIDTH=32 mophila render` |
| コマンド | `mophila render --set width=32 --set title=abc` |

## 8. エラー

`種別.細目: line N: message` の形で、message は英語。

| 種別 | 細目 |
|---|---|
| SyntaxError | `UnexpectedToken` 構文として読めない / `InvalidLiteral` 色の桁数、Duration の形式など |
| NameError | `UndefinedVariable` / `UndefinedAttribute` / `AssignWithoutLet` let を書かずに初めて代入した / `Reserved` builtin の型の名前を宣言し直した |
| TypeError | `OperandType` 演算子の左右 / `AttributeType` 属性に入れる値 / `ArgumentType` 引数 / `ArityMismatch` 引数や列の数 / `NotPlaceable` place できない型 |
| ValueError | `OutOfRange` 型は合うが値が範囲外 / `AlreadyPlaced` 同じ名前の Layer を 2 度置いた |
| RuntimeError | `DivisionByZero` / `FontNotFound` / `AudioUnreadable` / `ImageUnreadable` / `ShaderCompile` / `ShaderUnavailable` / `EndlessLoop` / `OutOfMemory` |

`run` は、スクリプトを実行したあと、`output` の Composition を始め、中ほど、終わりの 3 時刻で組み立てる。`func (t)` の Layer の中の誤りは、描く前にここで出る。描画はしないので、`ShaderCompile` と `ShaderUnavailable` は出ない。
`ShaderCompile` と `ShaderUnavailable` は描画のときに出る。
`OutOfMemory` も描画のときで、GPU のメモリが足りないときに、何を (Shader のテクスチャか、ズームの帯か)
どれだけ確保できなかったかと、そのコマの合計を出す。`render --gpu-budget 2GB` を書いておくと、
足りなくなる前に、超えたコマの時刻と内訳を警告で出す。

## 9. After Effects 機能の段階

いずれも「Node を受けて Node を返す関数」として足す。

| Tier | 内容 |
|---|---|
| 1 (コア) | transform 属性、Group による親子、ease 系、progress、wiggle、複数 Composition |
| 2 | repeat、trim path、text animators、mask (図形の輪郭で切る `clip` は Group にある。マスクどうしの合成や反転は未) |
| 3 | effects (blur、glow、shadow など)、camera、time-remap |

repeat は After Effects の Repeater に相当する。

```
repeat(n, node, rotation = tau / n, ...)
```

## 10. 未決

- 入れ子の Composition が、自分の `duration` を超えた時間にどう振る舞うか
- 文字列補間 `"dot-{i}"` を言語に入れるか、`.format` のままにするか
- 属性名と、関数・メソッド名の camelCase と snake_case の統一
- `fill` に入る Shader と World を、毎フレーム新しい図形を作る形でどう扱うか。関数の外で 1 度だけ作って渡す前提だが、仕様としては確定していない
- y 軸の向きと、`atan2` など既存の math 関数の角度の向きを、時計回りが正の規約に揃える具体的な手順
