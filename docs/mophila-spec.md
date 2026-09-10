# mophila 言語仕様 (整理中)

決定済みの方針と examples/ を整理したもの。言語はできるだけ少ない部品の組み合わせで構成する。

実装はインタプリタ。ソースを字句解析 → 構文解析し、構文木を評価して View と Timeline を作り、毎フレーム、キーフレームの式を構文木のまま評価して描く。コード生成は行わない。`bundle` はソースと import 先のファイルを実行ファイルに埋め込むもので、コンパイルではない。構文の見本は examples/ を正とし、ここには意味と規則を書く。未決事項は mophila-draft.md。

## 1. 字句

- `# ` (井桁と空白) から行末までがコメント。`##` で始まる行もコメントで、`export` の直前に置くとその項目の説明 (ドキュメントコメント) として `scripts/docgen.py` が拾う。1 行目が要約、`@param name 説明` と `@returns 説明` が引数と戻り値

- コメント: `# ` (# の後に空白) から行末。`#` の直後に文字が続けば Color リテラル
- 識別子: 英字・数字・`_`。先頭は英字か `_`
- 列挙値: `:name` (例 `:center`)
- リテラル: 下記「型」を参照
- 文の区切り: 改行。`{ }` の中も同じ

## 2. 演算子と優先順位

高い順。同じ行は同じ優先順位。`expr` は式、`name` は識別子、`args` は `expr, ...` (名前付きは `name = expr`)。

| 優先 | 形 | 結合 | 意味 |
|---|---|---|---|
| 1 | `expr . name` | 左 | 属性参照 |
| 1 | `expr [ expr ]` | 左 | 添字 |
| 1 | `expr ( args )` | 左 | 呼び出し |
| 1 | `Name ( args )` | — | 型の生成 |
| 2 | `- expr` | 右 | 符号反転 |
| 2 | `not expr` | 右 | 否定 |
| 3 | `expr ^ expr` | 右 | べき |
| 4 | `expr * expr`, `expr / expr`, `expr % expr` | 左 | 乗除、剰余 |
| 5 | `expr + expr`, `expr - expr` | 左 | 加減 |
| 6 | `expr .. expr`, `expr ..= expr` | なし | 範囲 |
| 7 | `expr < expr`, `expr <= expr`, `expr > expr`, `expr >= expr` | なし | 比較 |
| 8 | `expr == expr`, `expr != expr` | なし | 等価 |
| 9 | `expr and expr` | 左 | 論理積 |
| 10 | `expr or expr` | 左 | 論理和 |

優先順位に属さないもの:

| 形 | 意味 |
|---|---|
| `( expr )` | 括弧。優先順位を変える |
| `( expr , expr , ... )` | Tuple。要素が 1 つなら括弧 |
| `[ expr , ... ]` | List |
| `{ "key" : expr , ... }` | Dict。`{ name, ... }` は `{ "name": name, ... }` |
| `func ( params ) { ... }` | 無名関数 |
| `if expr { ... } else { ... }` | 条件式。値を持つ |

文 (値を持たない):

| 形 | 意味 |
|---|---|
| `let name = expr` | 束縛 |
| `let name : Type = expr` | 型注釈付き束縛 |
| `name = expr`, `expr . name = expr` | 代入 |
| `expr` | 式文 |

`- expr` は優先 2、`expr - expr` は優先 5。`-2 ^ 2` は `- expr` が先に結合するので `(-2) ^ 2 = 4`。`a - b` は `a` の後に `-` が来るので優先 5。改行は文の区切りなので、行頭の `- x` は新しい文 (`- expr`)。

## 3. 型

### 3.1 基本型

| 型 | リテラル | 備考 |
|---|---|---|
| Number | `1` `1.5` `1/3` `25%` | 整数と実数を区別しない。`1/3` は分数のまま保持。`100%` = 1.0 |
| Duration | `83s` `1m23s` `500ms` `01:23` `01:23:45.678` | 単位は ms / s / m / h。コロン形式は mm:ss と hh:mm:ss、小数秒可 |
| Color | `#rgb` `#rgba` `#rrggbb` `#rrggbbaa` `Color(r, g, b)` `Color(r, g, b, a)` | r/g/b は 0..255、a は 0..1。3/4 桁は各桁を重ねて 6/8 桁に広げる |
| String | `"hello"` `"say \\"hi\\""` | エスケープは `\\"` `\\\\` `\\n` `\\t`。`+` で連結。埋め込みはしない。`"n = {n}".format(n)` で `{...}` を順に置き換える (名前は説明用)。Dict を渡せば `"{x} {y}".format({x, y})` のように名前で置き換える |
| Bool | `true` `false` | |
| Symbol | `:center` `:linear` | 名前そのものが値。同じ名前どうしだけが等しい |
| Tuple | `(1, 1.5)` | 要素ごとに型が違ってよい |
| List | `[1, 2, 3]` | 要素は同じ型 |
| Dict | `{ "k": v }` `{ x, y }` | キーは String。挿入順を保つ。`{ x, y }` は `{ "x": x, "y": y }` の省略形 |
| Range | `0..5` `0..=5` | `..` は末尾を含まない |
| Func | `func (x) { x }` | 型表記は `Func<引数 -> 戻り値>` |
| Type | `Circle` | 型そのもの。呼ぶとその型の値を作る |
| Nothing | | 値を返さなかった関数の戻り値。書き方は無い |

### 3.2 Func の型表記

`Func<Number, Number -> Number>`。引数なしは `Func<-> Number>`。可変長は `Func<Number... -> Number>`。組み込みの `log` と `type_of` はどの型でも受け取るので、型を書けない。

### 3.3 リテラルと型名

型は大文字で始まる。型名を呼ぶとその型の値ができる (`Vector(8, 4.5)`)。よく使う型にはリテラルがあり、同じ型名の呼び出しと同じ値になる。

| リテラル | 同じもの |
|---|---|
| `(1, 2)` | `Tuple(1, 2)` |
| `[1, 2, 3]` | `List(1, 2, 3)` |
| `{ "k": v }` | `Dict(k = v)` (名前にできないキーは `{ }` だけ) |
| `0..5` | `Range(0, 5)` |
| `0..=5` | `Range(0, 6)` |
| `#4080e0` | `Color(64, 128, 224)` |

`Number` `Duration` `Bool` `String` `Symbol` `Func` はリテラルだけで、呼んでは作れない。呼ぶと `TypeError.ArgumentType` で書き方を返す。

組み込みの型の名前は予約されていて、`struct` / `record` / `type` で宣言し直すと `NameError.Reserved`。

### 3.4 Union 型

既存の型を `|` で結んだ名前。`type Fill = Color | Gradient`。値は作れず、引数や属性の型として書く。

言語が持つ union: `Shape` (図形すべて)、`Placeable` (`Shape | View`)、`Paint` (`Color | Gradient | Shader`)。

### 3.5 record と struct

どちらもトップレベルで宣言する。中身はフィールドと、`func` / `method` / `private`。

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

- `record` は値。フィールドを書き換えられず、名前とフィールドが同じなら等しい
- `struct` は実体。属性を書き換えられ、同じ実体どうしだけが等しい
- フィールドは `名前: 型` で、`= 式` で既定値を付けられる。`private` を付けたフィールドは外から読めない
- `func` は受け手を取らない。型名から呼ぶ (`Counter.new(10)`)
- `method` は第 1 引数が受け手。名前は何でもよく、使わないなら `_`
- `private` はメンバーにも付く。中からだけ呼べる
- `func new` を書けば、それが型名を呼んだときの作り方。中で型名を呼ぶとフィールドから作る
- `func new` が無ければ、`private` でないフィールドを宣言順に受け取って作る。`private` で既定値の無いフィールドがあると作れないので、`func new` が要る
- 位置で渡せるのは既定値の無いフィールドまで。名前で渡すときは `名前 = 値`
- フィールドの型が `Vector` か `Pos` なら、素の Tuple を渡すとその型に変換される

複製:

| 書き方 | 対象 | 意味 |
|---|---|---|
| `r.copy(field = 値)` | record | 一部だけ変えた新しい値 |
| `o.shallowCopy()` | struct | 属性をそのまま写した別の実体 |
| `o.deepCopy()` | struct | 中の record と struct もたどって写した別の実体 |

同じ名前の `func` / `method` を宣言すれば、そちらが使われる。

### 3.6 注釈

宣言の前の行に `@名前` を書く。複数は空白で並べる。

| 注釈 | 意味 |
|---|---|
| `@immutable` | `struct` を record と同じ値にする |
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

### 3.7 別名

`alias P = Pos` で、長い型名に短い名前を付ける。そのファイルの、その行より後ろで使える。`export` はできない。

### 3.8 決まった値しか取らない型

値はすべて Symbol。呼んでは作れない。

| 型 | 値 |
|---|---|
| Anchor | `:center` `:topLeft` `:topRight` `:bottomLeft` `:bottomRight` `:top` `:bottom` `:left` `:right` |
| StrokeCap | `:butt` `:round` `:square` |
| StrokeJoin | `:miter` `:round` `:bevel` |
| Blend | `:normal` `:multiply` `:screen` `:overlay` `:darken` `:lighten` `:difference` `:add` |
| GradientKind | `:linear` `:radial` `:sweep` |
| Align | `:left` `:center` `:right` |
| Ease | `:linear` `:ease_in` `:ease_out` `:ease` |
| Effect | `:fade` |

各値の意味はドキュメントサイトの「組み込み」を正とする (元は src/docs.rs)。

### 3.9 位置

- Vector: 実数 2 つ。`.x` `.y`。`Vector ± Vector`、`Vector × Number`、`Vector ÷ Number`、`-Vector`
- Pos: `x`、`y`、`anchor` (既定 `:center`)。`.anchor` `.vector` `.x` `.y` (`.x` `.y` は読み書きできる)。`Pos(Vector(1, 1.5))` でも作れる
- 型が `Vector` か `Pos` と決まっている場所 (属性、フィールド、`place` の `at`、motion の行) に素の Tuple を書くと、その型に変換される: `box = (16, 9)`、`position = (8, 4.5)`、`position = (0, 0, :topLeft)`。合わなければ `TypeError`。変換はその場所だけで、Tuple の値そのものは変わらない
- 基準点の違う Pos どうしの変換には図形のサイズが要るので、place された後にしか行えない

### 3.10 図形と箱

| 型 | 属性 |
|---|---|
| Circle | position: Pos, radius = Number |
| Ellipse | position: Pos, rx = Number (横の半径), ry = Number (縦の半径) |
| Rect | position: Pos, w, h = Number, radius = Number (角の丸み、省略なら 0) |
| Line | from, to = Vector |
| Polygon | points: List<Vector> |
| Path | from: Vector (始点), segments = List<Tuple> (`(:move \| :line \| :quad \| :curve, 点...)`。:quad は制御点 1 つ、:curve は 2 つを先に書く), closed = Bool (true で始点に戻って閉じる) |
| TextArea | text: String, position = Pos, w = Number, font = String, fontSize = Number, align = Align |
| View | box: Vector (座標系の幅と高さ。`Vector(4, 3)` なら 0..4 × 0..3。ピクセルは持たない), opacity = Number (中身をまとめて 1 枚として掛ける。中の図形が重なっても二重に薄くならない)。別の View に置かれたときは position / w / h も持つ |
| Subtitle | text: String, duration = Duration。画面には描かず、Timeline に置くと動画の字幕トラックになる |
| Gradient | kind: Symbol (`:linear` 既定 / `:radial` / `:sweep`), from = Vector (linear の始点、radial と sweep の中心), to = Vector (linear の終点), radius = Number (radial の半径), stops = List<Color> (2 つ以上。等間隔に並ぶ)。図形の `fill` に入れる |
| Shader | color: `Func<Number, Number, Number -> Color>` (引数は箱の座標 x, y と動画の時刻 t 秒), args = List<Number> (省略可。4 つ目の引数として渡る), samples = Number (1 ピクセルあたりの評価点の数。平方数に切り上げ、4 なら 2x2 の平均。省略は 1)。図形の `fill` に入れる塗りで、図形の中の各ピクセルの色をこの関数で決める |

図形の共通属性: fill: Paint (`type Paint = Color | Gradient | Shader`), stroke = Color, strokeWidth = Number, strokeCap = Symbol (`:butt` `:round` `:square`), strokeJoin (`:miter` `:round` `:bevel`), dash = List<Number> (線と間の長さ。`dashOffset` でずらす), opacity = Number, rotation = Number (時計回りの回転。単位は度。中心は position、Line は from、Polygon は頂点の重心), blend (`:normal` `:multiply` `:screen` `:overlay` `:darken` `:lighten` `:difference` `:add`)。Line 以外は position (Pos) で置く。`type Placeable = Shape | View`。

Shader の関数は描画のたびに GPU で全ピクセル分走るので、書けるのは数値の部分だけ: Number / Bool / Color / Vector (`Vector(x, y)`、`.x` `.y`、`+ -`、Number との `* /`)、四則・`%`・`^`・比較・論理、`if`、範囲の `for`、`return`、`math.*`、`Color(...)`、外側の Number / Color / Bool / Vector / List (どれか 1 種類だけのもの) と関数 (再帰は不可)。文字列、Duration、図形、Dict、素の Tuple は使えない。`args` は毎フレーム読むので、motion の行で変えれば動く。`run` では実行されず、`render` / `preview` / `sheet` で走る。GPU の実数は 32 bit

### 3.11 時間

| 型 | 中身 | 作り方 | duration |
|---|---|---|---|
| Motion | 値の時間変化。対象なし | `motion (t, a, b) { 時刻: 値, ... }` | 既定は最後の時刻。`m.duration = 10s` で上書き |
| Timeline | どの対象のどの属性が、いつ、どの値になるかの並び。入れ物にもなり、Timeline の中に Timeline を置ける | `motion (t) { 時刻: obj.attr = ... }` (行が属性への代入なら Timeline) / `m.apply(target, f)` (f は `func (target, t, [cols]) { }`) / `Timeline(duration = ...)` | 同上 |
| Audio | 音声ファイル。`duration` (ファイルの長さ) と `file` を持つ | `import "bgm.m4a" as bgm` (.moph 以外のファイルの import。長さは ffprobe で読む) | ファイルの長さ |

- 行の時刻は Duration (`2s`) か 0..1 の実数 (`0.5`、`50%`)。1 つの motion で混ぜられない。実数のときは `duration` の設定が必須
- `duration` を設定すると、全キーフレームの時刻がそれに合わせて比例して伸縮する
- Motion の行では `t` = その行に書いた時刻 (伸縮前)、以降の名前 = 同じ行の左の列の値
- 行末に Ease / Effect を書ける
- Timeline を Timeline に置く: `place(tl, at = Duration, fadeIn =, fadeOut =)`。duration を指定した入れ子は、はみ出した分を切る
- 音声を置く: `place(bgm, at =, duration =, fadeIn =, fadeOut =, volume =, loop =)`。duration で切る (繰り返さなければファイルより長くはならない)、volume は 1 がそのまま、`loop = true` は duration か動画の終わりまで繰り返す (動画を延ばさない)。同じ音声を何度でも置け、重なれば混ざる。duration を指定した Timeline の中では、はみ出した分を切る。render が ffmpeg で動画の音声トラックにし、preview は ffmpeg で PCM にして鳴らす
- 字幕を置く: `place(Subtitle(text = ..., duration = ...), at = ...)`。render が SRT にして動画の字幕トラックに入れる (mp4/mov は mov_text、webm は webvtt、mkv は srt)。プレイヤー側で表示を切り替える。preview と sheet では画面の下に重ねて出す
- 動画の長さは、置いたものすべての終わりの最大 (音声・字幕も含む)
- Motion は単独では place できない
- 各フレームの状態は「スクリプト実行直後の状態 + その時刻までに始まった Timeline を置いた順に適用したもの」で決まり、前のフレームに依存しない。preview で時間を戻しても同じ画になる

## 4. 文

- `let name = 式` — 束縛。再代入は `name = 式` (let なしの初回代入はエラー)
- `if 条件 { } else if { } else { }` — 式でもあり、最後の式が値
- `for x in List | Range | Dict { }`、`for (i, x) in xs.enumerate() { }`
- `func name(引数) { }` — 最後の式が戻り値。`return` も可 (`if` や `for` の中から関数を抜ける)。既定値・名前付き引数あり
- `context expr as name, ... { }` — ブロック内で `expr` を `name` として参照する。複数可。長い名前を短く書くためのもので、式なので最後の式が値になる
- `record Name { ... }` / `struct Name { ... }` — 型の宣言。トップレベルのみ。3.5 を参照
- `@注釈` — 直後の `record` / `struct` の性質。3.6 を参照
- `alias 名前 = 型` — 型に短い名前を付ける。そのファイルのその行より後ろだけ。`export` はできない
- `type Name = A | B` — Union 型の定義
- `output view` — 動画全体の指定
- `import name` — 標準ライブラリのモジュールを `name` に束縛する (Module 型)。import せずに使うと `NameError.UndefinedVariable` に `add "import name"` のヒントが付く
- `import math` / `import fractal` — 名前だけなら標準ライブラリ (本体に入っているもの。6 を参照)
- `import .orbit` — 同じ場所の `orbit.moph` を別のスコープで実行し、`export` した名前と `output` した View (`orbit.output`) を持つ Module を `orbit` に束縛する。`import .lib.shapes` は `./lib/shapes.moph`、`import ..parent` は 1 つ上 (文法上は通るが避ける)。`as name` で別名。`import "path/file.moph"` の形も可 (既定の名前はファイル名)
- `import { a, b } from .slides` — export した名前を直接この場所に持ち込む。`from math` も可
- `import "bgm.m4a" as bgm` — .moph 以外のファイルは音声として読み、Audio を束縛する (`as` が無ければファイル名)。`bundle` はこのファイルも埋め込む
- 同じファイルは 1 度だけ実行され、以後の import は同じ実体を返す。循環 import はエラー
- `export let ...` / `export func ...` / `export struct ...` / `export record ...` — import した側に公開する。トップレベルのみ

## 5. 空間

- 箱 (View) は `box = Vector(w, h)` で座標系を宣言する。中の座標はその単位
- `view.place(shape)`。図形の位置は図形の `position` で決まる
- `view.place(other_view, at = Pos, w =, h =)` で View を入れ子にする。w か h の片方だけなら縦横比を保ち、両方なら比率を保ったまま収める。子の座標は子の box で解釈される
- `timeline.place(view, at =)` / `view.addTrack(view)` で、子 View の Timeline を親の時間軸で動かす
- クロージャと Timeline はスコープを共有する (複製しない)。定義後に変数を変えれば、その値が見える

## 6. 組み込みと標準ライブラリ

import なしで使える組み込み:

| 名前 | 型 | 内容 |
|---|---|---|
| log | 関数 | 引数を空白区切りで stderr へ出力。どの型でも受け取る。動画には出ない |
| type_of | 関数 | 型名を String で返す |
| 組み込みの型の名前 | Type | 呼ぶとその型の値を作る。3.3 を参照 |
| String.format / len / replace、List / Dict のメソッド (join など) | | examples/collection.moph を参照 |

`import` で使う標準ライブラリ:

| モジュール | 内容 |
|---|---|
| math | `PI` `TAU` `E` (Number)、`sin` `cos` `floor` `ceil` `abs` `sqrt` `ln` `exp` (`Func<Number -> Number>`)、`atan2(y, x)`、`max` `min` (`Func<Number... -> Number>`) |
| color | 色を作る、混ぜる: `mix(a, b, k)` `lighten(c, k)` `darken(c, k)` `alpha(c, a)` `hsl(h, s, l)` `gray(v)`。Color は `c.r` `c.g` `c.b` (0..255) `c.a` (0..1) が読める |
| shape | Polygon の points を作る: `regular_polygon(cx, cy, r, n, rotation)` `star(cx, cy, outer, inner, n, rotation)` `arrow(from, to, width, head, head_width)` |
| layout | 並べる位置: `grid(x, y, w, h, cols, rows)` (各マスの中心) `cell(w, h, cols, rows)` `along(from, to, n)` `fit(w, h, box_w, box_h)` |
| transition | 図形や View の属性を動かす Timeline: `fade_in(o, duration)` `fade_out` `fade_to(o, from, to, duration)` `slide_in(o, dx, dy, duration)` `slide_out` `move_by` `show(track, objs, at, end, duration, stagger)` |
| fractal | エスケープタイム系フラクタル。`escape_time(formula, coloring, center, span, ...)` が Shader を返す。反復式 `mandelbrot` `julia(c)` `burning_ship` `multibrot(n)`、精度 `plain` `perturbation`、色付け `smooth(stops, period)`。本体に埋め込んだ .moph (src/stdlib/fractal.moph) で、説明はそのドキュメントコメントから |

## 7. エラー

種別のツリーと例は examples/error.moph。メッセージは英語で `種別.細目: message`。
