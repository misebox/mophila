//! 組み込みの説明。LSP のホバーと補完、`mophila doc --json` (docgen の元) が同じ表を使う

use serde_json::{Value as Json, json};

use crate::lang::eval::schema;

/// 名前、呼び方、説明
pub struct Entry {
    pub name: &'static str,
    pub signature: &'static str,
    pub doc: &'static str,
}

/// import なしで使えるもの
pub const BUILTINS: &[Entry] = &[
    Entry { name: "log", signature: "log(値, ...)", doc: "引数を空白区切りで stderr に出す。動画には出ない" },
    Entry { name: "type_of", signature: "type_of(値)", doc: "型名を String で返す" },
    Entry { name: "vector!", signature: "vector!(x, y)", doc: "Vector。座標や複素数" },
    Entry { name: "apos!", signature: "apos!(:anchor, x, y) / apos!(:anchor, vector)", doc: "AnchoredPosition。基準点付きの位置。anchor は :center :topLeft :topRight :bottomLeft :bottomRight :top :bottom :left :right" },
    Entry { name: "rgb!", signature: "rgb!(r, g, b)", doc: "Color。各 0..255" },
    Entry { name: "rgba!", signature: "rgba!(r, g, b, a)", doc: "Color。r g b は 0..255、a は 0..1" },
];

/// 値のメソッドと属性。receiver は型名
pub struct Method {
    pub receiver: &'static str,
    pub name: &'static str,
    pub signature: &'static str,
    pub doc: &'static str,
}

pub const METHODS: &[Method] = &[
    Method { receiver: "String", name: "len", signature: "s.len()", doc: "文字数" },
    Method { receiver: "String", name: "replace", signature: "s.replace(from, to)", doc: "文字列の置き換え" },
    Method { receiver: "String", name: "format", signature: "\"{name}\".format(値, ...)", doc: "{ } を順に引数で置き換える。名前は説明用" },
    Method { receiver: "List", name: "len", signature: "xs.len()", doc: "要素数" },
    Method { receiver: "List", name: "push", signature: "xs.push(値)", doc: "末尾に追加 (その List を変える)" },
    Method { receiver: "List", name: "enumerate", signature: "xs.enumerate()", doc: "(番号, 要素) の List。for (i, x) in xs.enumerate() で使う" },
    Method { receiver: "List", name: "reverse", signature: "xs.reverse()", doc: "逆順の List" },
    Method { receiver: "List", name: "contains", signature: "xs.contains(値)", doc: "含むか" },
    Method { receiver: "List", name: "index_of", signature: "xs.index_of(値)", doc: "最初の位置。無ければ -1" },
    Method { receiver: "List", name: "sum", signature: "xs.sum()", doc: "Number の合計" },
    Method { receiver: "List", name: "join", signature: "xs.join(sep)", doc: "文字列にして sep でつなぐ" },
    Method { receiver: "List", name: "map", signature: "xs.map(func (x) { ... })", doc: "各要素に関数を当てた List" },
    Method { receiver: "List", name: "filter", signature: "xs.filter(func (x) { ... })", doc: "条件に合う要素だけの List" },
    Method { receiver: "List", name: "reduce", signature: "xs.reduce(初期値, func (acc, x) { ... })", doc: "畳み込み" },
    Method { receiver: "List", name: "sort", signature: "xs.sort()", doc: "昇順に並べた List (Number か Duration)" },
    Method { receiver: "List", name: "zip", signature: "xs.zip(ys)", doc: "(x, y) の List" },
    Method { receiver: "Range", name: "to_list", signature: "(0..n).to_list()", doc: "List にする" },
    Method { receiver: "Range", name: "steps", signature: "(0..=1).steps(n)", doc: "端を含めて n 等分した Number の List" },
    Method { receiver: "Dict", name: "keys", signature: "d.keys()", doc: "キーの List" },
    Method { receiver: "Dict", name: "values", signature: "d.values()", doc: "値の List" },
    Method { receiver: "Dict", name: "has", signature: "d.has(key)", doc: "キーがあるか" },
    Method { receiver: "Dict", name: "len", signature: "d.len()", doc: "要素数" },
    Method { receiver: "View", name: "place", signature: "v.place(shape) / v.place(view, at:, w:, h:)", doc: "図形を置く。View を置くときは位置と大きさ (w か h の片方なら比率を保つ)" },
    Method { receiver: "View", name: "addTrack", signature: "v.addTrack(timeline)", doc: "Timeline を付ける。output する View に付けたものが動画になる" },
    Method { receiver: "Timeline", name: "place", signature: "tl.place(x, at:, fadeIn:, fadeOut:, duration:, volume:, loop:)", doc: "Timeline / View / Audio / Subtitle を時刻に置く。duration volume loop は Audio だけ" },
    Method { receiver: "Timeline", name: "reverse", signature: "tl.reverse()", doc: "逆再生の Timeline" },
    Method { receiver: "Motion", name: "apply", signature: "m.apply(target, func (target, t, [cols]) { ... })", doc: "対象に当てて Timeline にする" },
    Method { receiver: "Motion", name: "reverse", signature: "m.reverse()", doc: "逆再生の Motion" },
    Method { receiver: "Vector", name: "x", signature: "v.x", doc: "x 成分" },
    Method { receiver: "Vector", name: "y", signature: "v.y", doc: "y 成分" },
    Method { receiver: "AnchoredPosition", name: "anchor", signature: "p.anchor", doc: "基準点 (Symbol)" },
    Method { receiver: "AnchoredPosition", name: "vector", signature: "p.vector", doc: "座標 (Vector)" },
    Method { receiver: "AnchoredPosition", name: "x", signature: "p.x", doc: "x 成分。書き換えも可" },
    Method { receiver: "AnchoredPosition", name: "y", signature: "p.y", doc: "y 成分。書き換えも可" },
    Method { receiver: "Audio", name: "duration", signature: "bgm.duration", doc: "ファイルの長さ (Duration)" },
    Method { receiver: "Audio", name: "file", signature: "bgm.file", doc: "import に書いたパス" },
    Method { receiver: "Color", name: "r", signature: "c.r / c.g / c.b", doc: "成分 0..255" },
    Method { receiver: "Color", name: "a", signature: "c.a", doc: "不透明度 0..1" },
    Method { receiver: "Module", name: "output", signature: "mod.output", doc: "そのファイルが output した View" },
    Method { receiver: "Module", name: "name", signature: "mod.name", doc: "そのファイルが export した名前" },
    Method { receiver: "Motion", name: "duration", signature: "m.duration = 8s", doc: "全体の長さ。0..1 で書いた表はこれに合わせて伸縮する" },
    Method { receiver: "Timeline", name: "duration", signature: "tl.duration = 8s", doc: "全体の長さ。0..1 で書いた表はこれに合わせて伸縮する" },
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
    Type { name: "Timeline", union: "", make: "new Timeline {}  context o as x { motion (t) { ... } }", doc: "何を、いつ動かすか。motion を対象に当てたもの、別の Timeline、Audio、Subtitle を place で時刻に置く" },
    Type { name: "Motion", union: "", make: "motion (t) { 0s: 1  2s: 3 }", doc: "時刻と値の表。対象を持たないので、apply か context で対象に当てて Timeline にする" },
    Type { name: "Subtitle", union: "", make: "new Subtitle { text:, duration: }", doc: "字幕。画面には描かず、Timeline に置くと動画の字幕トラックになる" },
    Type { name: "Audio", union: "", make: "import \"bgm.m4a\" as bgm", doc: "読み込んだ音声ファイル。Timeline に置くと動画の音になる" },
    Type { name: "Module", union: "", make: "import .file  import { name } from math", doc: "import が返すもの。そのファイルが export した名前と、output した View を持つ" },
    Type { name: "Circle", union: "Shape", make: "new Circle { position:, radius:, fill: }", doc: "position を中心に、radius の半径で描く円" },
    Type { name: "Rect", union: "Shape", make: "new Rect { position:, w:, h:, fill: }", doc: "幅 w、高さ h の四角形。radius を付けると角が丸くなる" },
    Type { name: "Line", union: "Shape", make: "new Line { from:, to:, stroke:, strokeWidth: }", doc: "from から to へ引く 1 本の線。太さは strokeWidth、色は stroke" },
    Type { name: "Polygon", union: "Shape", make: "new Polygon { points:, fill: }", doc: "points の頂点を順に結んで閉じた図形。points は Vector の List" },
    Type { name: "TextArea", union: "Shape", make: "new TextArea { text:, position:, fontSize:, fill: }", doc: "text を描く。w を付けるとその幅で折り返し、align で行の寄せ方を決める" },
    Type { name: "Color", union: "Paint", make: "#4080e0  #4080e080  rgb!(r, g, b)  rgba!(r, g, b, a)", doc: "1 つの色。r g b は 0..255、a は 0..1。c.r や c.a で成分を読める" },
    Type { name: "Shader", union: "Paint", make: "new Shader { color: func (x, y, t) { ... } }", doc: "位置 (x, y) と時刻 t から、そのピクセルの色を返す関数。図形の fill に入れると GPU で走る" },
];

/// 属性の説明。(型, 属性, 説明)。型が "*" なら図形に共通
pub const ATTRS: &[(&str, &str, &str)] = &[
    ("*", "position", "基準点付きの位置 (apos!)"),
    ("*", "fill", "塗り。Color か Shader"),
    ("*", "stroke", "線の色"),
    ("*", "strokeWidth", "線の太さ (箱の座標の単位)"),
    ("*", "opacity", "不透明度 0..1"),
    ("Circle", "radius", "半径"),
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
    ("Timeline", "duration", "全体の長さ。指定すると、はみ出した分を切る"),
    ("Subtitle", "text", "字幕の文字列"),
    ("Subtitle", "duration", "表示する長さ"),
    ("Shader", "color", "func (x, y, t [, args]) -> Color。x y は箱の座標、t は秒"),
    ("Shader", "args", "Number の List。color の 4 つ目の引数。毎フレーム読む"),
    ("Shader", "samples", "1 ピクセルあたりの評価点の数 (4 なら 2x2 の平均。省略は 1)"),
];

/// 型と属性の説明。型ごとの説明が無ければ共通のもの
pub fn attr_doc(kind: &str, attr: &str) -> &'static str {
    ATTRS.iter().find(|(k, a, _)| *k == kind && *a == attr).or_else(|| ATTRS.iter().find(|(k, a, _)| *k == "*" && *a == attr)).map_or("", |(_, _, d)| *d)
}

pub fn json() -> Json {
    let entries = |list: &[Entry]| -> Vec<Json> { list.iter().map(|e| json!({ "name": e.name, "signature": e.signature, "doc": e.doc })).collect() };
    let types: Vec<Json> = TYPES
        .iter()
        .map(|t| {
            let attrs: Vec<Json> = schema(t.name).unwrap_or(&[]).iter().map(|(a, ty)| json!({ "name": a, "type": ty, "doc": attr_doc(t.name, a) })).collect();
            let methods: Vec<Json> = METHODS.iter().filter(|m| m.receiver == t.name).map(|m| json!({ "name": m.name, "signature": m.signature, "doc": m.doc })).collect();
            json!({ "name": t.name, "union": t.union, "make": t.make, "doc": t.doc, "attrs": attrs, "methods": methods })
        })
        .collect();
    json!({ "builtins": entries(BUILTINS), "math": entries(crate::stdlib::math::DOCS), "types": types })
}
