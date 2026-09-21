# mophila 言語仕様

言語の意味と規則。できるだけ少ない部品の組み合わせで構成する。

## 1. 字句

```
# これはコメント     # "# " から行末まで
#4080e0            # # の直後に文字が続けば Color リテラル
name_1             # 識別子は英字・数字・_。先頭は英字か _
:center            # Symbol
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

括弧 `( expr )` は優先順位を変える。リテラル (`( )` `[ ]` `{ }`) と `func` `if` `context` `motion` は優先順位を持たない。3 と 4 を参照。

`-expr` は優先 2、`expr - expr` は優先 5。だから `-2 ^ 2` は `(-2) ^ 2` で 4 になる。改行は文の区切りなので、行頭の `- x` は引き算ではなく新しい文。

## 3. 型

### 3.1 基本型

| 型 | リテラル | 備考 |
|---|---|---|
| Number | `1` `1.5` `1/3` `25%` `1e-9` | 整数と実数を区別しない。`100%` = 1.0。`1e-9` は指数表記。3.2 を参照 |
| Duration | `83s` `1m23s` `500ms` `01:23` `01:23:45.678` | 単位は ms / s / m / h。コロン形式は mm:ss と hh:mm:ss、小数秒可 |
| Color | `#rgb` `#rgba` `#rrggbb` `#rrggbbaa` `Color(r, g, b)` `Color(r, g, b, a)` | r/g/b は 0..255、a は 0..1。3/4 桁は各桁を重ねて 6/8 桁に広げる |
| String | `"hello"` `"say \"hi\""` | エスケープは `\"` `\\` `\n` `\t`。`+` で連結。埋め込みはしない。`"n = {n}".format(n)` が `{...}` を順に置き換える (中の名前は説明用)。Dict を渡すと `"{x} {y}".format({x, y})` のように名前で置き換える |
| Bool | `true` `false` | |
| Symbol | `:center` `:linear` | 名前そのものが値。同じ名前どうしだけが等しい |
| Tuple | `(1, 1.5)` | 要素ごとに型が違ってよい。長さは書いたときに決まる。要素 1 つは `(1,)`、空は `()` |
| List | `[1, 2, 3]` | 値を順に並べたもの。要素の型は検査しない。メソッドの署名の `T` は要素の型、`U` は変換後の型 |
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

`Number` `Duration` `Bool` `String` `Symbol` `Func` はリテラルだけで、呼んでは作れない。呼ぶと `TypeError.ArgumentType` で書き方を返す。

builtin の型の名前は予約されていて、`struct` / `record` / `type` で宣言し直すと `NameError.Reserved`。

### 3.5 Union 型

既存の型を `|` で結んだ名前。`type Fill = Color | Gradient`。値は作れず、引数や属性の型として書く。

言語が持つ union: `Shape` (図形すべて)、`Placeable` (`Shape | View`)、`Paint` (`Color | Gradient | Shader`)、`Font` (`String | List`。候補を並べられる)。

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

`alias P = Pos` で、長い型名に短い名前を付ける。そのファイルの、その行より後ろで使える。`export` はできない。

### 3.9 決まった値しか取らない型

属性がこの型なら、その型に載っている名前しか書けない。値はすべて Symbol で、型を呼んでは作れない。違う名前を書くと `ValueError.OutOfRange` が、取れる値を並べて出る。

| 型 | 何を決めるか | 取れる値 (先頭が既定) |
|---|---|---|
| Anchor | Pos の座標が、置くもののどこを指すか | `:center` `:topLeft` `:top` `:topRight` `:left` `:right` `:bottomLeft` `:bottom` `:bottomRight` |
| Align | TextArea の行の寄せ方 | `:left` `:center` `:right` |
| StrokeCap | 線の両端の形 | `:butt` `:round` `:square` |
| StrokeJoin | 折れ曲がった線の、角の形 | `:miter` `:round` `:bevel` |
| Blend | 下にある絵との混ぜ方 | `:normal` `:multiply` `:screen` `:overlay` `:darken` `:lighten` `:difference` `:add` |
| GradientKind | Gradient の stops を並べる向き | `:linear` `:radial` `:sweep` |
| Easing | 区間の値の変わり方 (motion の行末にだけ書ける) | `:linear` `:ease_in` `:ease_out` `:ease` |

同じものを自分でも書ける。

```
type Mode = :fast | :slow
```

### 3.10 位置

- Vector: 実数 2 つ。`.x` `.y`。`Vector ± Vector`、`Vector × Number`、`Vector ÷ Number`、`-Vector`
- Pos: `x`、`y`、`anchor` (既定 `:center`)。`.anchor` `.vector` `.x` `.y` (`.x` `.y` は読み書きできる)。`Pos(Vector(1, 1.5))` でも作れる
- `(8, 4.5)` は Tuple であって Vector でも Pos でもない。`Vector` や `Pos` が要る場所には型名を書く
- anchor の解決 (`:topLeft` が図形のどこかを出すこと) は描画のときに行われ、スクリプトからは触れない

### 3.11 図形

`Circle` `Ellipse` `Rect` `TextArea` は `position` (Pos) で置く。`Line` は `from` と `to`、`Polygon` は `points`、`Path` は `from` と `segments` が位置を決めるので `position` を持たない。

| 型 | その型だけの属性 |
|---|---|
| Circle | `radius: Number` |
| Ellipse | `rx: Number` (横の半径) `ry: Number` (縦の半径) |
| Rect | `w: Number` `h: Number` `radius: Number` (角の丸み。省略は 0) |
| Line | `from: Vector` `to: Vector` |
| Polygon | `points: List<Vector>` |
| Path | `from: Vector` `segments: List<Tuple>` `closed: Bool` `upto: Number` |
| TextArea | `text: String` (`\n` で改行) `w: Number` (折り返す幅) `font: String` `fontSize: Number` `align: Align`。`size()` で置いたときの幅と高さ (Vector) を返す |

図形は `length()` で輪郭の長さを返す。`dash` と `dashOffset` で線を少しずつ描き出すときに使う。

`Path` の `segments` は `(:move | :line | :quad | :curve, 点...)` の並び。`:quad` は制御点 1 つ、`:curve` は 2 つを、終点より前に書く。`closed = true` なら始点に戻って閉じる。`Path.through(points, closed =)` は点の並びをつないだ Path を作る。

`upto` は「`segments` の先頭から何割を描くか」(0..1、既定は 1)。点の列はそのままに、描く範囲だけ動かせる。最後の 1 区間は途中で切るので、線の先は滑らかに伸びる。`p.point_at(1)` がその先の点なので、ペン先を置くものを合わせられる。`upto` が 1 未満なら閉じない。

```
line.upto = t                    # 範囲の行で動かす
pen.position = Pos(line.point_at(1))
```

どの図形も持つ属性。効きようがないものは、その図形が持たない (書くと `NameError.UndefinedAttribute`)。既定のあるものは、書かなくてもその値で読める:

| 属性 | 型 | 意味 |
|---|---|---|
| `fill` | Paint | 塗り。Color / Gradient / Shader / Image |
| `stroke` | Color | 線の色 |
| `strokeWidth` | Number | 線の太さ (箱の座標の単位) |
| `strokeCap` | StrokeCap | 線の端の形 |
| `strokeJoin` | StrokeJoin | 線の角の形 |
| `dash` | List | 破線。線と間の長さを順に並べた Number |
| `dashOffset` | Number | 破線の始まりをずらす長さ |
| `opacity` | Number | 不透明度 0..1 |
| `rotation` | Number | 時計回りの回転 (度) |
| `pivot` | Vector | 回転の中心。書かなければ、その図形を囲む四角形の中心 |
| `blend` | Blend | 下の絵との重ね方 |
| `zIndex` | Number | 同じ View の中で描く順。小さいものが先 (下)。既定は 0 で、同じ値なら place した順 |

| 持たない図形 | 属性 | 理由 |
|---|---|---|
| Line | `fill` | 面が無い |
| Circle / Ellipse / Line | `strokeJoin` | 角が無い |
| TextArea | `strokeCap` `dash` `dashOffset` | 字の輪郭を破線にできない |

### 3.12 箱

```
View(box = Vector(16, 9))
```

| 属性 | 型 | 意味 |
|---|---|---|
| `box` | Vector | 座標系の幅と高さ。`Vector(16, 9)` なら 0..16 × 0..9。ピクセルは持たない |
| `opacity` | Number | 中身をまとめて 1 枚として掛ける。中の図形が重なっても二重に薄くならない |
| `clip` | Bool | `true` なら箱からはみ出した中身を描かない。既定は `false` |
| `blend` | Blend | 中身を 1 枚にしてから、下の絵と重ねる |
| `zIndex` | Number | 親の中で描く順。図形と同じ規則 |
| `position` `w` `h` | Pos / Number | 別の View に置かれたときの位置と大きさ |
| `rotation` | Number | 置いたものを 1 枚として回す (度)。中身の座標は変わらない |
| `scale` | Number | 置いたものを 1 枚として拡大する。既定は 1 |
| `pivot` | Vector | `rotation` と `scale` の中心。子の箱の座標で書く。書かなければ箱の真ん中 |
| `camera` | Camera | 箱の中身の寄り引き。枠は動かさず中身だけ動かす |

`rotation` と `scale` は置いたあとの絵に掛かるので、中の図形の座標や `place` の書き方は変えなくてよい。150 枚のタイルをまとめて回すなら、タイルを入れた View を 1 つ作って、その `rotation` を動かす。

### 3.13 塗り

**Camera** — `View.camera` に入れると、箱の**中身**の `from` が `to` の位置に来るように置いて `scale` 倍する。枠 (箱の外周) は動かない。

```
v.camera = Camera(from = Vector(3, 2.5), to = Vector(8, 4.5), scale = 10)
```

| 属性 | 型 | 意味 |
|---|---|---|
| `from` | Vector | 中身のどの点を映すか (箱の座標) |
| `to` | Vector | それを画面のどこに置くか (箱の座標)。省略は `from` と同じ = 中身を動かさずに拡大だけする |
| `scale` | Number | 倍率。省略は 1。0 以下は `ValueError.OutOfRange` |

`position` `w` `h` `pivot` `rotation` `scale` は**置かれた View そのもの**を枠ごと動かすもので、`camera` とは別物。両方書けば掛かり合う。寄ると中身は箱からはみ出すので、`clip = true` と併せて使う。

3 つを別々の属性にせず 1 つの型にしてあるのは、途中で食い違って画面が跳ねないようにするため。動かすときはフィールドを動かす (`Camera` 値そのものは補間しない)。

```
motion (t) { 0..1: { v.camera.scale = math.exp(math.ln(1e12) * t) } }
```

`fill` に入れられるのは `Color` `Gradient` `Shader` (`Paint`)。

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
| `args` | List | Number の並び。`color` の 4 つ目の引数として渡る |
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

`args` は毎フレーム読むので、motion の行で変えれば動く。`run` では走らず、`render` `preview` `sheet` で走る。GPU の実数は 32 bit。

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

### 3.14 時間

時間を持つ型は 3 つ。`Motion` は値の並びだけを持ち、対象を持たない。`Timeline` は対象と属性まで決まったもので、同時に入れ物でもある。`Audio` は読み込んだ音声。

`motion` が返す `Timeline` も `Timeline()` で作った `Timeline` も同じ型で、どちらも `place` で中にものを置ける。

```
let m = motion (t, a, b) {      # Motion。値の表
  0s: 1, 2, 3
  2s: 3, 6, 9
}
let tl = motion (t) {           # 行が属性への代入なら Timeline
  0s: c.radius = 1
  2s: c.radius = 3
}
m.apply(c, func (o, t, [r]) { o.radius = r })   # Motion に対象を与えても Timeline
let track = Timeline(duration = 8s)             # 入れ物だけの Timeline
import "bgm.m4a" as bgm                         # Audio
```

行の時刻は Duration (`2s`) か 0..1 の実数 (`0.5` `50%`)。1 つの motion で混ぜられない。実数で書いたら `duration` の設定が要る。

時刻には式も書ける。`..` は範囲の区切りなので、そこで読むのを止める。

```
let dur = 3s
motion (t) { 0s: c.radius = 1
             dur: c.radius = 3 }          # 変数
motion (t) { start: c.opacity = 0
             start + 0.4s: c.opacity = 1 }   # 足し算
```


間を補間できるのは Number、Duration、Vector、Pos、Color と、それらを同じ長さで並べた Tuple / List。長さの違う並びや、補間できない値は、次の行の時刻で切り替わる。`Path` の `segments` や `Polygon` の `points` を動かすと形が変わる。

`duration` は長さ。既定は最後の時刻で、代入すると長さだけが変わる (行の時刻は動かない。短くすれば、そこから先の行は使われない)。0..1 で書いた表は時刻が割合なので、`duration` が実時間を決める。Audio の `duration` はファイルの長さ。

時刻そのものを動かすのはメソッド。

| 書き方 | すること |
|---|---|
| `tl.scale(2)` | 時間の軸を 2 倍にする。時刻も長さも 2 倍 |
| `tl.fit(3s)` | 長さが 3s になるように時間の軸を伸縮する |
| `tl.trim(from = 1s, to = 4s)` | 1s から 4s までを取り出し、0 から始まるものにする |
| `tl.reverse()` | 時間の向きを逆にする |

どれも新しい Timeline (Motion なら Motion) を返し、元は変わらない。`scale` と `fit` は中に置いた Timeline も同じ倍率で伸縮する。音声と字幕は開始時刻だけ動き、長さは変わらない。

Motion の行では、`t` はその行に書いた時刻、2 つ目以降の名前は同じ行の左の列の値。

行が書けるのは、図形の属性 (`c.radius`) と**普通の変数** (`weight`、`p.x`) の両方。変数を動かせば、図形を持たない値 (重み、位相、進み具合) をそのまま動かせる。

```
let weight = 0
let p = Vector(0, 0)
motion (t) {
  0s: weight = 0, p = Vector(2, 2)
  2s: weight = 10, p = Vector(14, 7) :ease
}
```

**範囲の行** — 時刻を `0..1:` のように範囲で書くと、その区間は補間せず、式を毎フレーム評価する。

```
motion (t) { 0..1: c.position = Pos(2 + t * 12, 4.5) }
```

範囲の行は値の代わりに文の列を書ける。`let` が書けるので、1 つの点から複数の属性を決めるときに 1 度だけ計算すれば済む。

```
motion (t) {
  0..1: {
    let p = tip(t)
    pen.position = Pos(p)
    rail.to = Vector(p.x, p.y)
  }
}
```

同じ属性を上の行と両方で書いたら、ブロックのほうが後に走る。`mophila timeline` は、ブロックを 1 度走らせて何を書いたかを出す。

属性に代入する motion は、2 つ目の名前で**経過秒**を受け取れる。`0..1` の割合で書いた行から、秒で決まる動きをそのまま書ける (長さを 2 か所に書かなくて済む)。

```
let tl = motion (t, sec) {
  0..1: c.position = Pos(8 + math.cos(sec / 2.5s * math.TAU) * 5, 4.5)
}
tl.duration = 5s      # 2.5 秒で 1 周する動きが 5 秒続く
```

行末に Easing を 1 つ書ける。効くのは、前の行からその行までの区間。

```
motion (t) {
  0s: c.radius = 1
  1s: c.radius = 3 :ease
}
```

`:linear` `:ease_in` `:ease_out` `:ease` の 4 つで、書かなければ `:linear`。`:ease_in` は加速、`:ease_out` は減速、`:ease` は両方。Easing は属性の型ではなく、この行末にだけ書ける。

置く。`Timeline.place` は Timeline の中へ、`View.addTrack` はその View の動きとして付ける。引数は同じで、`output` した View に付いたものが動画になる。

```
v.addTrack(track)
track.place(tl, at = 2s, fadeIn = 0.3s, fadeOut = 0.3s)
track.place(bgm, at = 0s, duration = 30s, volume = 0.6, loop = true)
track.place(Narration(text = "ここを読み上げる", duration = 2s), at = 5s)
```

`fadeIn` は置いた時刻から、`fadeOut` は終わりに向かって、置いたものの不透明度を動かす。View に付けると中身をまとめて 1 枚として掛かる。Narration には効かない (`duration` で長さを決める)。

`duration` を指定した Timeline は、その長さより後を使わない。音声は `duration` で切り (繰り返さなければファイルより長くならない)、`volume` は 1 がそのまま、`loop = true` は `duration` か動画の終わりまで繰り返す (動画は延びない)。同じ音声を何度でも置けて、重なれば混ざる。

読み上げは `Narration`。文と長さだけを持ち、Timeline に置く (画面には描かない)。**その文を読んだ音声が動画の音になり、同じ文が字幕になる**。**どう喋らせるか**は置くときに `voice` で渡す。

```
let kyoko = SayVoiceEngine(voice = "Kyoko")
track.place(Narration(text = "ここを読み上げる", duration = 3s), at = 2s, voice = kyoko)
track.place(Narration(text = "ここは速く", duration = 3s), at = 5s, voice = SayVoiceEngine(voice = "Kyoko", rate = 300))
```

`voice` に入れるのは音声合成の型 (`Voice`)。合成ごとに受け取る設定が違うので、型も別になっている。

| 型 | 何を使うか | 属性 |
|---|---|---|
| `SayVoiceEngine` | macOS の say | `voice` (`say -v ?` の名前) `rate` (1 分あたりの語数) |
| `EspeakVoiceEngine` | espeak-ng | `voice` (`espeak-ng --voices` の名前) `speed` `pitch` `gap` |

属性はその型のものだけ。無い名前を書けば `NameError.UndefinedAttribute` で止まる。`voice` を渡さなければ、その機械に入っているもので読む。置くときの `volume` で音量を変えられる。

合成した音声が `duration` より長ければ、そこで切って警告を出す。行の時刻は動かない。

字幕は読み上げからしか作らない。出るのは読み始めから読み終わりまでで、長さは作った音声から測る。`duration` の見積もりがずれても、字幕と声はずれない。声を付けない字幕は書けない (書けるようにすると、また人が時刻を見積もることになるため)。

作った音声はビルドの中間物なので、リポジトリではなくキャッシュ (`~/.cache/mophila/voice/`、`$XDG_CACHE_HOME` があればその下) に置く。同じ型・同じ設定・同じ文なら作り直さない。書いたものが音声として読めなければ、そこで止まる (壊れたものをキャッシュに残さない)。

`render` は音声を ffmpeg で動画の音声トラックにし、字幕を SRT にして字幕トラックに入れる (mp4 と mov は mov_text、webm は webvtt、mkv は srt)。

`--trim` が時間の切り取りで、`--crop` が領域の切り取り。`--crop height` は高さを箱いっぱいにして幅を出力の比から決め、
`--crop width` はその逆、`--crop 0.25,1` (`25%,100%` でも同じ) は箱に対する割合で直に決める。
切り取った範囲が箱より小さいとき、余ったところのどこに寄せるかが `--align` で、
`left` `center` `right` `top` `bottom` `topLeft` などの名前か、軸ごとの割合 (`30%,50%`) を書く (既定は中央)。
どちらも箱の大きさを知らずに書ける。絵が画面を埋めないときの帯の色は `--pad '#000000'` (既定は白)。

`subs` は動画を描かずに字幕だけをファイルに出す (`-o subs.srt` / `-o subs.vtt`。省略すると標準出力)。中身は動画の字幕トラックと同じで、`--trim` も同じように効く。`preview` は音声を PCM にして鳴らし、字幕を画面の下に重ねる。`sheet` も重ねる。重ねる字幕は、箱を収めた絵の中に出す (窓の縦横比が箱と違えば、余った帯には出ない)。

Motion は単独では place できない。動画の長さは、置いたものすべての終わりの最大 (音声と字幕も含む)。

各フレームの状態は、スクリプトを実行した直後の状態に、その時刻までに始まった Timeline を置いた順で適用して決まる。前のフレームには依存しないので、preview で時間を戻しても同じ画になる。

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

### 4.4 context

長い名前を短く書くための式。ブロックの最後の式が値になる。

```
let r = context board.left.card as o {   # o は board.left.card
  o.radius
}

context a as x, b as y { }   # 複数書ける
```

### 4.5 型の宣言

```
record Size { }        # 3.6
struct Counter { }     # 3.6
@nocopy                # 3.7。直後の record / struct に付く
alias P = Pos          # 3.8。そのファイルのその行より後ろだけ
type Fill = Color | Gradient   # 3.5
```

`record` `struct` `type` はトップレベルにだけ書ける。

### 4.6 import

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

`.moph` の Module から見えるのは、そのファイルが `export` した名前と、`output` した View (`orbit.output`) だけ。

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

### 4.7 export

```
export let theme = #4080e0
export func run(now: Duration) -> Duration { now }
export struct Card { }
export record Size { }
```

トップレベルにだけ書ける。`alias` は export できない。

### 4.8 output

```
output v      # この View が動画になる。1 つのファイルに 1 つ
```

## 5. 空間

- 箱 (View) は `box = Vector(w, h)` で座標系を宣言する。中の座標はその単位
- `view.place(shape)`。図形の位置は図形の `position` で決まる
- `view.place(other_view, at = Pos, w =, h =)` で View を入れ子にする。w か h の片方だけなら縦横比を保ち、両方なら比率を保ったまま収める。子の座標は子の box で解釈される。置き先での位置と大きさは子の属性になるので、同じ View を 2 か所には置けない (`copy()` を置く)
- `view.addTrack(x)` で、その View の動きとして付ける。`output` した View に付けたものが動画になる。`x` は Timeline、View、Audio、Narration のどれかで、引数は `Timeline.place` と同じ
- クロージャと Timeline はスコープを共有する (複製しない)。定義後に変数を変えれば、その値が見える

## 6. builtin と標準ライブラリ

`import` なしで使えるのは、builtin の型の名前と、次の 2 つだけ。どちらも呼び出しの形でしか書けず、値として取り出すことはできない。

| 名前 | 内容 |
|---|---|
| `log(値, ...)` | 引数を空白区切りで stderr へ出す。どの型でも受け取る。動画には出ない |
| `type_of(値)` | その値の型の名前を String で返す |

String / List / Dict / Range のメソッドは builtin で、`import` は要らない。

`import` で使う標準ライブラリ。

| モジュール | 内容 |
|---|---|
| math | 数学。`PI` `TAU` `E` などの定数と、三角関数・丸め・平方根・対数・最大最小・範囲の変換。`polar(r, a)` は数学の向き (弧度・反時計回り) で Vector を作る |
| color | 色を作る、混ぜる。明るくする、暗くする、不透明度を変える、色相から作る |
| shape | Polygon の `points` を作る。円周上の点、正多角形、星、矢印 |
| layout | 並べる位置を計算する。格子、直線上の等間隔、比率を保って収めた大きさ |
| animation | 図形や View を動かす Timeline を返す。現れる、消える、滑る、回る |
| palette | そのまま使える配色。背景・文字・罫線・強調・系列色の組 (`Palette`) を名前で選ぶ |
| pattern | そのまま `fill` に入れられる模様。縞、細線、市松、水玉、方眼、ざらつき |
| icon | チェックや矢印などの記号の形。`mark` でそのまま置ける図形になる |
| fractal | エスケープタイム系フラクタルの Shader。反復式と色付けを組み合わせる。倍率は `zoom` / `duration` / `camera` のどれかで決める |

## 7. プロジェクト設定

`mophila.yaml` を、コマンドを実行したディレクトリに置くと自動で読まれる。
台本の代わりに**そのフォルダか `mophila.yaml` 自身を渡しても読む** (`mophila render project/`、`mophila run project/mophila.yaml`)。
このときの台本は設定の `entry`。`-f` は、設定と台本が別の場所にあるときに使う (`-f ../shared.yaml scenes/main.moph`)。
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
```

**設定の中に書いたパスは、設定ファイルのある場所からの相対**。コマンドラインに書いたパスは、いま居るディレクトリからの相対。設定はプロジェクトの形を書いたものなので、どこで実行しても同じ場所を指す。

`config` の値は数・文字列・真偽の 3 つ。`import config` で読む。

```
import config
let v = View(box = Vector(config.width, 9))
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
| ValueError | `DurationRequired` 時刻の単位が混在、相対時刻なのに duration が無い / `OutOfRange` 型は合うが値が範囲外 / `AlreadyPlaced` 同じ View を 2 か所に置いた |
| RuntimeError | `DivisionByZero` / `FontNotFound` / `AudioUnreadable` / `ImageUnreadable` / `ShaderCompile` / `ShaderUnavailable` / `EndlessLoop` / `OutOfMemory` |

`ShaderCompile` と `ShaderUnavailable` は描画のときに出る。`run` では Shader を走らせないので出ない。
`OutOfMemory` も描画のときで、GPU のメモリが足りないときに、何を (Shader のテクスチャか、ズームの帯か)
どれだけ確保できなかったかと、そのコマの合計を出す。`render --gpu-budget 2GB` を書いておくと、
足りなくなる前に、超えたコマの時刻と内訳を警告で出す。
