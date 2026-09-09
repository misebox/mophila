//! 組み込みの説明。LSP のホバーと補完、`mophila doc --json` (docgen の元) が同じ表を使う

use serde_json::{Value as Json, json};

use crate::lang::eval::schema;

/// 名前、呼び方、戻り値の型、説明
pub struct Entry {
    pub name: &'static str,
    pub signature: &'static str,
    pub returns: &'static str,
    pub doc: &'static str,
}

/// import なしで使えるもの
pub const BUILTINS: &[Entry] = &[
    Entry { name: "log", signature: "log(値: Any, ...)", returns: "Nothing", doc: "引数を空白区切りで stderr に出す。動画には出ない" },
    Entry { name: "type_of", signature: "type_of(値: Any)", returns: "String", doc: "型名を返す" },
    Entry { name: "vector!", signature: "vector!(x: Number, y: Number)", returns: "Vector", doc: "座標や複素数に使う実数 2 つの組" },
    Entry { name: "apos!", signature: "apos!(anchor: Anchor, x: Number, y: Number) / apos!(anchor: Anchor, v: Vector)", returns: "AnchoredPosition", doc: "位置と、その位置がものの どこ を指すか" },
    Entry { name: "rgb!", signature: "rgb!(r: Number, g: Number, b: Number)", returns: "Color", doc: "各 0..255" },
    Entry { name: "rgba!", signature: "rgba!(r: Number, g: Number, b: Number, a: Number)", returns: "Color", doc: "r g b は 0..255、a は 0..1" },
];

/// 値のメソッドと属性。receiver は型名、returns は戻り値の型 (代入だけのものは空)
pub struct Method {
    pub receiver: &'static str,
    pub name: &'static str,
    pub signature: &'static str,
    pub returns: &'static str,
    pub doc: &'static str,
}

pub const METHODS: &[Method] = &[
    Method { receiver: "String", name: "len", signature: "s.len()", returns: "Number", doc: "文字数" },
    Method { receiver: "String", name: "replace", signature: "s.replace(from: String, to: String)", returns: "String", doc: "from を to に置き換えた文字列" },
    Method { receiver: "String", name: "format", signature: "\"{name}\".format(値: Any, ...)", returns: "String", doc: "{ } を順に引数で置き換える。名前は説明用" },
    Method { receiver: "List", name: "len", signature: "xs.len()", returns: "Number", doc: "要素数" },
    Method { receiver: "List", name: "push", signature: "xs.push(値: T)", returns: "Nothing", doc: "末尾に追加する。その List 自身が変わる" },
    Method { receiver: "List", name: "enumerate", signature: "xs.enumerate()", returns: "List<(Number, T)>", doc: "番号と要素の組。for (i, x) in xs.enumerate() で使う" },
    Method { receiver: "List", name: "reverse", signature: "xs.reverse()", returns: "List<T>", doc: "逆順にした新しい List" },
    Method { receiver: "List", name: "contains", signature: "xs.contains(値: T)", returns: "Bool", doc: "その値を含むか" },
    Method { receiver: "List", name: "index_of", signature: "xs.index_of(値: T)", returns: "Number", doc: "最初に現れる位置。無ければ -1" },
    Method { receiver: "List", name: "sum", signature: "xs.sum()", returns: "Number", doc: "Number の合計" },
    Method { receiver: "List", name: "join", signature: "xs.join(sep: String)", returns: "String", doc: "各要素を文字列にして sep でつなぐ" },
    Method { receiver: "List", name: "map", signature: "xs.map(f: Func<T -> U>)", returns: "List<U>", doc: "各要素に f を通した結果の List" },
    Method { receiver: "List", name: "filter", signature: "xs.filter(f: Func<T -> Bool>)", returns: "List<T>", doc: "f が true を返した要素だけの List" },
    Method { receiver: "List", name: "reduce", signature: "xs.reduce(初期値: U, f: Func<U, T -> U>)", returns: "U", doc: "初期値から順に f を通して 1 つの値にする" },
    Method { receiver: "List", name: "sort", signature: "xs.sort()", returns: "List<T>", doc: "昇順に並べた新しい List (要素は Number か Duration)" },
    Method { receiver: "List", name: "zip", signature: "xs.zip(ys: List<U>)", returns: "List<(T, U)>", doc: "同じ位置どうしを組にする。短い方に合わせる" },
    Method { receiver: "Range", name: "to_list", signature: "(0..n).to_list()", returns: "List<Number>", doc: "範囲の整数を並べた List" },
    Method { receiver: "Range", name: "steps", signature: "(0..=1).steps(n: Number)", returns: "List<Number>", doc: "両端を含めて n 等分した値の List (要素は n + 1 個)" },
    Method { receiver: "Dict", name: "keys", signature: "d.keys()", returns: "List<String>", doc: "キーの List" },
    Method { receiver: "Dict", name: "values", signature: "d.values()", returns: "List<T>", doc: "値の List" },
    Method { receiver: "Dict", name: "has", signature: "d.has(key: String)", returns: "Bool", doc: "そのキーがあるか" },
    Method { receiver: "Dict", name: "len", signature: "d.len()", returns: "Number", doc: "要素数" },
    Method { receiver: "View", name: "place", signature: "v.place(o: Placeable, at: AnchoredPosition, w: Number, h: Number)", returns: "Nothing", doc: "箱の中に置く。View を置くときは at と大きさを渡す (w か h の片方だけなら比率を保つ)" },
    Method { receiver: "View", name: "addTrack", signature: "v.addTrack(tl: Timeline)", returns: "Nothing", doc: "この View の動きとして Timeline を付ける。output する View に付けたものが動画になる" },
    Method { receiver: "Timeline", name: "place", signature: "tl.place(x: Timeline | View | Audio | Subtitle, at: Duration, fadeIn: Duration, fadeOut: Duration, duration: Duration, volume: Number, loop: Bool)", returns: "Nothing", doc: "at の時刻に置く。duration volume loop は Audio だけ" },
    Method { receiver: "Timeline", name: "reverse", signature: "tl.reverse()", returns: "Timeline", doc: "時間を逆にした Timeline" },
    Method { receiver: "Timeline", name: "duration", signature: "tl.duration = 8s", returns: "Duration", doc: "全体の長さ。0..1 で書いた表はこれに合わせて伸縮する" },
    Method { receiver: "Motion", name: "apply", signature: "m.apply(target: Placeable, f: Func<Placeable, Duration, List -> Nothing>)", returns: "Timeline", doc: "表の行ごとに f を呼び、そこで target の属性に代入された値を、その時刻の値とする Timeline を返す" },
    Method { receiver: "Motion", name: "reverse", signature: "m.reverse()", returns: "Motion", doc: "時間を逆にした Motion" },
    Method { receiver: "Motion", name: "duration", signature: "m.duration = 8s", returns: "", doc: "全体の長さを決める。0..1 で書いた表はこれに合わせて伸縮する。代入だけで、読み出しはできない" },
    Method { receiver: "Vector", name: "x", signature: "v.x", returns: "Number", doc: "x 成分" },
    Method { receiver: "Vector", name: "y", signature: "v.y", returns: "Number", doc: "y 成分" },
    Method { receiver: "AnchoredPosition", name: "anchor", signature: "p.anchor", returns: "Symbol", doc: "基準点 (:center など)" },
    Method { receiver: "AnchoredPosition", name: "vector", signature: "p.vector", returns: "Vector", doc: "座標" },
    Method { receiver: "AnchoredPosition", name: "x", signature: "p.x", returns: "Number", doc: "x 成分。代入もできる" },
    Method { receiver: "AnchoredPosition", name: "y", signature: "p.y", returns: "Number", doc: "y 成分。代入もできる" },
    Method { receiver: "Audio", name: "duration", signature: "bgm.duration", returns: "Duration", doc: "ファイルの長さ" },
    Method { receiver: "Audio", name: "file", signature: "bgm.file", returns: "String", doc: "import に書いたパス" },
    Method { receiver: "Color", name: "r", signature: "c.r / c.g / c.b", returns: "Number", doc: "成分 0..255" },
    Method { receiver: "Color", name: "a", signature: "c.a", returns: "Number", doc: "不透明度 0..1" },
    Method { receiver: "Module", name: "output", signature: "mod.output", returns: "View", doc: "そのファイルが output した View" },
    Method { receiver: "Module", name: "export", signature: "mod.<export した名前>", returns: "その値の型", doc: "そのモジュールが export した値。名前の数だけ読める" },
];

/// 型。union は言語が定義している union の名前 (Shape / Paint) で、属さない型は空。
/// 属性は eval::schema から、メソッドは METHODS から付ける
pub struct Type {
    pub name: &'static str,
    pub union: &'static str,
    pub make: &'static str,
    pub doc: &'static str,
}

pub const TYPES: &[Type] = &[
    Type { name: "Number", union: "", make: "1  1.5  1/3  25%", doc: "数。整数と実数を区別しない。1/3 は分数のまま持ち、100% は 1 になる" },
    Type { name: "Duration", union: "", make: "2s  500ms  1m23s  01:23", doc: "時間の長さ。単位は ms / s / m / h。01:23 は mm:ss、01:23:45 は hh:mm:ss" },
    Type { name: "Bool", union: "", make: "true  false", doc: "真か偽。if と論理演算で使う" },
    Type { name: "String", union: "", make: "\"hello\"", doc: "文字列。+ でつなぐ。\"{n}\".format(n) で {} の所へ値を入れる" },
    Type { name: "Tuple", union: "", make: "(1, 1.5)", doc: "要素ごとに型を持つ組。型の決まった所に書くと、その型 (Vector や AnchoredPosition) に変換される" },
    Type { name: "Range", union: "", make: "0..5  0..=5", doc: "整数の範囲。.. は末尾を含まず、..= は含む。for i in 0..n のように回す" },
    Type { name: "Func", union: "", make: "func (x) { x * 2 }", doc: "関数。変数に入れて渡せる。型は Func<引数 -> 戻り値> と書く" },
    Type { name: "List", union: "", make: "[1, 2, 3]", doc: "同じ型の要素を順に並べたもの。for で回し、map や filter で作り直す" },
    Type { name: "Dict", union: "", make: "{ \"k\": v }  { x, y }", doc: "String のキーから値を引く。{ x, y } は { \"x\": x, \"y\": y } の省略形" },
    Type { name: "Vector", union: "", make: "vector!(x, y)", doc: "実数 2 つの組。座標や複素数に使う。Vector 同士を + -、Number と * / できる" },
    Type { name: "AnchoredPosition", union: "", make: "apos!(:center, x, y)", doc: "位置と、その位置がものの どこ を指すか (:center や :topLeft)。図形や View の position に入れる" },
    Type { name: "View", union: "", make: "new View { box: (16, 9) }", doc: "座標系を持つ入れ物。中に図形や View を place し、addTrack で Timeline を付ける。output した View が動画になる" },
    Type { name: "Timeline", union: "", make: "new Timeline {}  motion (t) { 0s: c.radius = 1  2s: c.radius = 3 }", doc: "どの対象のどの属性が、いつ、どの値になるかの並び。行に属性への代入を書いた motion がこれになる。別の Timeline、Audio、Subtitle も place で時刻に置ける" },
    Type { name: "Motion", union: "", make: "motion (t) { 0s: 1  2s: 3 }", doc: "時刻と値の表。どの対象のどの属性に入れるかは持たない。apply がそれを決めて Timeline にする" },
    Type { name: "Subtitle", union: "", make: "new Subtitle { text: \"ここに字幕\", duration: 2s }", doc: "字幕。画面には描かず、Timeline に置くと動画の字幕トラックになる" },
    Type { name: "Audio", union: "", make: "import \"bgm.m4a\" as bgm", doc: "読み込んだ音声ファイル。Timeline に置くと動画の音になる" },
    Type { name: "Anchor", union: "", make: ":center :topLeft :topRight :bottomLeft :bottomRight :top :bottom :left :right", doc: "位置がものの どこ を指すか。apos! に渡す" },
    Type { name: "Align", union: "", make: ":left :center :right", doc: "TextArea の行の寄せ方" },
    Type { name: "Ease", union: "", make: ":linear :ease_in :ease_out :ease", doc: "motion の行末に書く、値の変わり方" },
    Type { name: "Effect", union: "", make: ":fade", doc: "motion の行末に書く効果" },
    Type { name: "StrokeCap", union: "", make: ":butt :round :square", doc: "線の端の形" },
    Type { name: "StrokeJoin", union: "", make: ":miter :round :bevel", doc: "線の角の形" },
    Type { name: "Blend", union: "", make: ":normal :multiply :screen :overlay :darken :lighten :difference :add", doc: "下の絵との重ね方" },
    Type { name: "GradientKind", union: "", make: ":linear :radial :sweep", doc: "Gradient の広がり方" },
    Type { name: "Module", union: "", make: "import .file  import { name } from math", doc: "import が返すもの。そのファイルが export した名前と、output した View を持つ" },
    Type { name: "Circle", union: "Shape", make: "new Circle { position: apos!(:center, 8, 4.5), radius: 1, fill: #4080e0 }", doc: "position を中心に、radius の半径で描く円" },
    Type { name: "Ellipse", union: "Shape", make: "new Ellipse { position: apos!(:center, 8, 4.5), rx: 3, ry: 1.4, fill: #4080e0 }", doc: "楕円。rx が横の半径、ry が縦の半径" },
    Type { name: "Rect", union: "Shape", make: "new Rect { position: apos!(:topLeft, 0, 0), w: 16, h: 9, fill: #f5f4f0 }", doc: "幅 w、高さ h の四角形。radius を付けると角が丸くなる" },
    Type { name: "Line", union: "Shape", make: "new Line { from: (0, 0), to: (4, 3), stroke: #303030, strokeWidth: 0.1 }", doc: "from から to へ引く 1 本の線。太さは strokeWidth、色は stroke" },
    Type { name: "Polygon", union: "Shape", make: "new Polygon { points: [(0, 0), (2, 0), (1, 2)], fill: #3f9b6d }", doc: "points の頂点を順に結んで閉じた図形。points は Vector の List" },
    Type { name: "Path", union: "Shape", make: "new Path { from: (0, 0), segments: [(:line, (2, 0)), (:curve, (3, 1), (3, 2), (2, 3))], closed: true }", doc: "曲線も引ける形。from から始まり、segments の (:move | :line | :quad | :curve, 点...) を順にたどる。closed: true で始点に戻って閉じる" },
    Type { name: "TextArea", union: "Shape", make: "new TextArea { text: \"hello\", position: apos!(:center, 8, 4.5), fontSize: 1, fill: #303030 }", doc: "text を描く。w を付けるとその幅で折り返し、align で行の寄せ方を決める" },
    Type { name: "Color", union: "Paint", make: "#4080e0  #4080e080  rgb!(r, g, b)  rgba!(r, g, b, a)", doc: "1 つの色。r g b は 0..255、a は 0..1。c.r や c.a で成分を読める" },
    Type { name: "Gradient", union: "Paint", make: "new Gradient { from: (0, 0), to: (16, 9), stops: [#4080e0, #e0604a] }", doc: "色が連続して変わる塗り。stops は等間隔に並ぶ。kind が :linear なら from から to へ、:radial なら from を中心に radius まで、:sweep なら from のまわりを 1 周する" },
    Type { name: "Shader", union: "Paint", make: "new Shader { color: func (x, y, t) { rgb!(x * 16, 0, 128) } }", doc: "位置 (x, y) と時刻 t から、そのピクセルの色を返す関数。図形の fill に入れると GPU で走る" },
];

/// 属性の説明。(型, 属性, 説明)。型が "*" なら図形に共通
pub const ATTRS: &[(&str, &str, &str)] = &[
    ("*", "position", "基準点付きの位置 (apos!)"),
    ("*", "fill", "塗り。Color か Shader"),
    ("*", "stroke", "線の色"),
    ("*", "strokeWidth", "線の太さ (箱の座標の単位)"),
    ("*", "strokeCap", "線の端の形。:butt (既定) :round :square"),
    ("*", "strokeJoin", "線の角の形。:miter (既定) :round :bevel"),
    ("*", "dash", "破線。線と間の長さを順に並べた Number の List (例 [0.6, 0.4])"),
    ("*", "dashOffset", "破線の始まりをずらす長さ。motion で動かすと破線が流れる"),
    ("*", "blend", "下の絵との重ね方。:normal (既定) :multiply :screen :overlay :darken :lighten :difference :add"),
    ("*", "opacity", "不透明度 0..1"),
    ("*", "rotation", "時計回りの回転 (度)。中心は position。Line は from、Polygon と Path は囲む四角形の中心"),
    ("Circle", "radius", "半径"),
    ("Ellipse", "rx", "横の半径"),
    ("Ellipse", "ry", "縦の半径"),
    ("Path", "from", "始点"),
    ("Path", "segments", "(:move | :line | :quad | :curve, 点...) の List。:quad は制御点 1 つ、:curve は 2 つを先に書く"),
    ("Path", "closed", "true なら始点に戻って閉じる (既定は false)"),
    ("Rect", "w", "幅"),
    ("Rect", "h", "高さ"),
    ("Rect", "radius", "角の丸み (省略は 0)"),
    ("Line", "from", "始点"),
    ("Line", "to", "終点"),
    ("Polygon", "points", "頂点。Vector の List"),
    ("TextArea", "text", "文字列"),
    ("TextArea", "w", "折り返す幅 (省略なら折り返さない)"),
    ("TextArea", "font", "フォント名"),
    ("TextArea", "fontSize", "文字の大きさ (箱の座標の単位)"),
    ("TextArea", "align", ":left :center :right"),
    ("View", "box", "座標系の幅と高さ。ピクセルは持たない"),
    ("View", "position", "別の View に置かれたときの位置"),
    ("View", "w", "別の View に置かれたときの幅"),
    ("View", "h", "別の View に置かれたときの高さ"),
    ("View", "opacity", "中身をまとめて 1 枚として掛ける不透明度"),
    ("View", "blend", "中身をまとめて 1 枚にしてから、下の絵と重ねるときの重ね方"),
    ("Timeline", "duration", "全体の長さ。指定すると、はみ出した分を切る"),
    ("Subtitle", "text", "字幕の文字列"),
    ("Subtitle", "duration", "表示する長さ"),
    ("Gradient", "kind", ":linear (既定) :radial :sweep"),
    ("Gradient", "from", ":linear の始点、:radial と :sweep の中心 (箱の座標)"),
    ("Gradient", "to", ":linear の終点"),
    ("Gradient", "radius", ":radial の半径"),
    ("Gradient", "stops", "Color の List。等間隔に並ぶ。2 つ以上"),
    ("Shader", "color", "func (x, y, t [, args]) -> Color。x y は箱の座標、t は秒"),
    ("Shader", "args", "Number の List。color の 4 つ目の引数。毎フレーム読む"),
    ("Shader", "samples", "1 ピクセルあたりの評価点の数 (4 なら 2x2 の平均。省略は 1)"),
];

/// 型と属性の説明。型ごとの説明が無ければ共通のもの
pub fn attr_doc(kind: &str, attr: &str) -> &'static str {
    ATTRS.iter().find(|(k, a, _)| *k == kind && *a == attr).or_else(|| ATTRS.iter().find(|(k, a, _)| *k == "*" && *a == attr)).map_or("", |(_, _, d)| *d)
}

pub fn json() -> Json {
    let entries = |list: &[Entry]| -> Vec<Json> { list.iter().map(|e| json!({ "name": e.name, "signature": e.signature, "returns": e.returns, "doc": e.doc })).collect() };
    let types: Vec<Json> = TYPES
        .iter()
        .map(|t| {
            let attrs: Vec<Json> = schema(t.name).unwrap_or(&[]).iter().map(|(a, ty)| json!({ "name": a, "type": ty, "doc": attr_doc(t.name, a) })).collect();
            let methods: Vec<Json> = METHODS.iter().filter(|m| m.receiver == t.name).map(|m| json!({ "name": m.name, "signature": m.signature, "returns": m.returns, "doc": m.doc })).collect();
            json!({ "name": t.name, "union": t.union, "make": t.make, "doc": t.doc, "attrs": attrs, "methods": methods })
        })
        .collect();
    json!({ "builtins": entries(BUILTINS), "math": entries(crate::stdlib::math::DOCS), "types": types })
}
