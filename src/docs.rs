//! 組み込みの説明。LSP のホバーと補完、`mophila doc --json` (docgen の元) が同じ表を使う

use serde_json::{Value as Json, json};

use crate::eval::{KINDS, schema};

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

/// `import math` で使えるもの
pub const MATH: &[Entry] = &[
    Entry { name: "PI", signature: "math.PI", doc: "円周率" },
    Entry { name: "TAU", signature: "math.TAU", doc: "2π" },
    Entry { name: "E", signature: "math.E", doc: "自然対数の底" },
    Entry { name: "sin", signature: "math.sin(x)", doc: "正弦 (ラジアン)" },
    Entry { name: "cos", signature: "math.cos(x)", doc: "余弦 (ラジアン)" },
    Entry { name: "floor", signature: "math.floor(x)", doc: "切り捨て" },
    Entry { name: "ceil", signature: "math.ceil(x)", doc: "切り上げ" },
    Entry { name: "abs", signature: "math.abs(x)", doc: "絶対値" },
    Entry { name: "sqrt", signature: "math.sqrt(x)", doc: "平方根" },
    Entry { name: "ln", signature: "math.ln(x)", doc: "自然対数" },
    Entry { name: "exp", signature: "math.exp(x)", doc: "e の x 乗" },
    Entry { name: "atan2", signature: "math.atan2(y, x)", doc: "(x, y) の角度 (ラジアン)" },
    Entry { name: "max", signature: "math.max(a, b, ...)", doc: "最大" },
    Entry { name: "min", signature: "math.min(a, b, ...)", doc: "最小" },
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
    Method { receiver: "Module", name: "output", signature: "mod.output", doc: "そのファイルが output した View" },
    Method { receiver: "Motion", name: "duration", signature: "m.duration = 8s", doc: "全体の長さ。0..1 で書いた表はこれに合わせて伸縮する" },
    Method { receiver: "Timeline", name: "duration", signature: "tl.duration = 8s", doc: "全体の長さ。0..1 で書いた表はこれに合わせて伸縮する" },
];

/// 型の説明。属性は eval::schema から
pub const TYPES: &[(&str, &str)] = &[
    ("Circle", "円"),
    ("Rect", "矩形"),
    ("Line", "線分"),
    ("Polygon", "多角形"),
    ("TextArea", "文字"),
    ("View", "箱。中に図形や View を置き、Timeline を付ける。output した View が動画になる"),
    ("Timeline", "時刻に置かれたものの集まり。motion を当てたもの、別の Timeline、音声、字幕を置く"),
    ("Subtitle", "字幕。画面には描かず、動画の字幕トラックになる"),
    ("Shader", "位置と時刻から色を決める塗り。図形の fill に入れる"),
    ("Color", "色。#rrggbb / #rrggbbaa / #rgb、rgb!() rgba!()、new Color { r, g, b, a }"),
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
    let methods: Vec<Json> = METHODS.iter().map(|m| json!({ "receiver": m.receiver, "name": m.name, "signature": m.signature, "doc": m.doc })).collect();
    let types: Vec<Json> = KINDS
        .iter()
        .map(|k| {
            let doc = TYPES.iter().find(|(n, _)| n == k).map_or("", |(_, d)| *d);
            let attrs: Vec<Json> = schema(k)
                .unwrap_or(&[])
                .iter()
                .map(|(a, t)| {
                    json!({ "name": a, "type": t, "doc": attr_doc(k, a) })
                })
                .collect();
            json!({ "name": k, "doc": doc, "attrs": attrs })
        })
        .collect();
    json!({ "builtins": entries(BUILTINS), "math": entries(MATH), "methods": methods, "types": types })
}
