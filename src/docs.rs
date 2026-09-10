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
    Entry { name: "log", signature: "log(値, ...)", returns: "Nothing", doc: "引数を空白区切りで stderr に表示する。動画には入らない。引数はどの型でもよいので、型を書けない" },
    Entry { name: "type_of", signature: "type_of(値)", returns: "String", doc: "その値の型の名前を返す。引数はどの型でもよいので、型を書けない" },
];


/// 組み込みの型。
/// - category はドキュメントの分類。CATEGORIES の順に並べる
/// - union は言語が定義している union のうち、この型が属するもの (属さなければ空)
/// - make は作り方。1 行が 1 通りで、ここに無い書き方は無い。作れない型は空
/// - values は決まった値しか取らない型の、値と意味
/// - members は union がまとめている型 (union でなければ空)
/// 属性は eval::schema から、メソッドは METHODS から付ける
pub struct Type {
    pub name: &'static str,
    pub category: &'static str,
    pub union: &'static str,
    pub make: &'static str,
    pub values: &'static [(&'static str, &'static str)],
    pub members: &'static [&'static str],
    pub doc: &'static str,
}

/// 分類の並び順
pub const CATEGORIES: &[&str] =
    &["Primitive", "Collection", "Function", "Position", "Shape", "Paint", "Scene", "Time", "Media", "Enum", "Union", "Meta"];

pub const TYPES: &[Type] = &[
    Type {
        name: "Number",
        category: "Primitive",
        union: "",
        make: "1\n1.5\n1/3\n25%",
        values: &[],
        members: &[],
        doc: "数。整数と実数を区別しない。分数で表せるあいだは分数のまま持つので、1/3 * 3 は 1、0.1 + 0.2 は 0.3 になる。sqrt のように分数で表せない計算が来ると実数に落ちる。25% は 0.25",
    },
    Type {
        name: "Duration",
        category: "Primitive",
        union: "",
        make: "2s\n500ms\n1m23s\n01:23\n01:23:45.678",
        values: &[],
        members: &[],
        doc: "時間の長さ。単位は ms / s / m / h。01:23 は mm:ss、01:23:45.678 は hh:mm:ss.mmm。motion の時刻と duration に使う",
    },
    Type {
        name: "Bool",
        category: "Primitive",
        union: "",
        make: "true\nfalse",
        values: &[],
        members: &[],
        doc: "真か偽。if の条件になり、and or not と比較演算子が返す",
    },
    Type {
        name: "String",
        category: "Primitive",
        union: "",
        make: "\"hello\"",
        values: &[],
        members: &[],
        doc: "文字列。+ でつなぐ。\"{n}\".format(n) で { } の所へ値を入れる",
    },
    Type {
        name: "Symbol",
        category: "Primitive",
        union: "",
        make: ":center\n:linear",
        values: &[],
        members: &[],
        doc: "先頭に : を付けた名前。名前そのものが値で、同じ名前どうしだけが等しい。Anchor や Easing のように「決まった名前しか取らない型」の値は、すべて Symbol",
    },
    Type {
        name: "Tuple",
        category: "Collection",
        union: "",
        make: "(1, 1.5)\nTuple(1, 1.5)\n(1,)\n()",
        values: &[],
        members: &[],
        doc: "要素ごとに型が違ってよい組。長さは書いたときに決まる。要素 1 つは (1,)、空は ()。Vector や Pos が要る場所には書けない。そこには型名を書く",
    },
    Type {
        name: "List",
        category: "Collection",
        union: "",
        make: "[1, 2, 3]\nList(1, 2, 3)\nList()",
        values: &[],
        members: &[],
        doc: "値を順に並べたもの。[ ] は List(...) と同じものを作る。for で回し、map や filter で作り直す。push で伸ばすと、その List 自身が変わる。要素の型は検査しないので、揃えるかどうかは書く側が決める。メソッドの署名にある T は要素の型、U は変換後の型",
    },
    Type {
        name: "Dict",
        category: "Collection",
        union: "",
        make: "{ \"k\": v }\n{ x, y }\nDict(k = v)\nDict()",
        values: &[],
        members: &[],
        doc: "String のキーから値を引く。挿入した順を保つ。{ x, y } は { \"x\": x, \"y\": y } の省略形。Dict(k = v) の k は名前として書けるキーだけで、空白などを含むキーは { } で書く",
    },
    Type {
        name: "Range",
        category: "Collection",
        union: "",
        make: "0..5\n0..=5\nRange(0, 5)",
        values: &[],
        members: &[],
        doc: "整数の範囲。.. は末尾を含まず、..= は含む。Range(0, 5) は 0..5 と同じで、0..=5 は Range(0, 6)。for i in 0..n のように回す。両端が整数でなければ ValueError.OutOfRange",
    },
    Type {
        name: "Func",
        category: "Function",
        union: "",
        make: "func (x: Number) -> Number { x * 2 }\nfunc (x) { x * 2 }\nfunc (a: Number, b: Number = 1) -> Number { a + b }\nfunc () -> Duration { 3s }\nfunc (o) { o.opacity = 0 }",
        values: &[],
        members: &[],
        doc: "関数。変数に入れて渡せる。引数の型 (名前: 型) と戻り値の型 (-> 型) は、どちらも書かなくてよい。既定値は = で書く。型を書く場所での表記は宣言と同じ形で、(引数) -> 戻り値",
    },
    Type {
        name: "Vector",
        category: "Position",
        union: "",
        make: "Vector(8, 4.5)",
        values: &[],
        members: &[],
        doc: "実数 2 つの組。座標、大きさ、複素数に使う。Vector どうしを + -、Number と * / できる",
    },
    Type {
        name: "Pos",
        category: "Position",
        union: "",
        make: "Pos(8, 4.5)\nPos(0, 0, anchor = :topLeft)\nPos(Vector(8, 4.5))",
        values: &[],
        members: &[],
        doc: "位置と、その位置がものの どこ を指すか。フィールドは x、y、anchor で、anchor の既定は :center。図形や View の position に入れる",
    },
    Type {
        name: "Circle",
        category: "Shape",
        union: "Shape",
        make: "Circle(position = Pos(8, 4.5), radius = 1, fill = #4080e0)",
        values: &[],
        members: &[],
        doc: "position を中心に、radius の半径で描く円",
    },
    Type {
        name: "Ellipse",
        category: "Shape",
        union: "Shape",
        make: "Ellipse(position = Pos(8, 4.5), rx = 3, ry = 1.4, fill = #4080e0)",
        values: &[],
        members: &[],
        doc: "楕円。rx が横の半径、ry が縦の半径",
    },
    Type {
        name: "Rect",
        category: "Shape",
        union: "Shape",
        make: "Rect(position = Pos(0, 0, anchor = :topLeft), w = 16, h = 9, fill = #f5f4f0)",
        values: &[],
        members: &[],
        doc: "幅 w、高さ h の四角形。radius を付けると角が丸くなる",
    },
    Type {
        name: "Line",
        category: "Shape",
        union: "Shape",
        make: "Line(from = Vector(0, 0), to = Vector(4, 3), stroke = #303030, strokeWidth = 0.1)",
        values: &[],
        members: &[],
        doc: "from から to へ引く 1 本の線。太さは strokeWidth、色は stroke",
    },
    Type {
        name: "Polygon",
        category: "Shape",
        union: "Shape",
        make: "Polygon(points = [Vector(0, 0), Vector(2, 0), Vector(1, 2)], fill = #3f9b6d)",
        values: &[],
        members: &[],
        doc: "points の頂点を順に結んで閉じた図形。points は Vector の List",
    },
    Type {
        name: "Path",
        category: "Shape",
        union: "Shape",
        make: "Path(from = Vector(0, 0), segments = [(:line, Vector(2, 0)), (:curve, Vector(3, 1), Vector(3, 2), Vector(2, 3))], closed = true)",
        values: &[],
        members: &[],
        doc: "曲線も引ける形。from から始まり、segments の (:move | :line | :quad | :curve, 点...) を順にたどる。closed = true で始点に戻って閉じる",
    },
    Type {
        name: "TextArea",
        category: "Shape",
        union: "Shape",
        make: "TextArea(text = \"hello\", position = Pos(8, 4.5), fontSize = 1, fill = #303030)",
        values: &[],
        members: &[],
        doc: "text を描く。w を付けるとその幅で折り返し、align で行の寄せ方を決める",
    },
    Type {
        name: "Color",
        category: "Paint",
        union: "Paint",
        make: "#4080e0\n#48e\n#4080e080\n#48e8\nColor(64, 128, 224)\nColor(64, 128, 224, 0.5)",
        values: &[],
        members: &[],
        doc: "1 つの色。# は 16 進で、3 桁と 4 桁は各桁を 2 回書いたものと同じ (#48e は #4488ee)。4 桁と 8 桁の最後は不透明度。Color の r g b は 0..255、a は 0..1",
    },
    Type {
        name: "Gradient",
        category: "Paint",
        union: "Paint",
        make: "Gradient(from = Vector(0, 0), to = Vector(16, 9), stops = [#4080e0, #e0604a])",
        values: &[],
        members: &[],
        doc: "色が連続して変わる塗り。stops は等間隔に並ぶ。広がり方は kind (GradientKind) で決める",
    },
    Type {
        name: "Shader",
        category: "Paint",
        union: "Paint",
        make: "Shader(color = func (x, y, t) { Color(x * 16, 0, 128) })",
        values: &[],
        members: &[],
        doc: "位置 (x, y) と時刻 t から、そのピクセルの色を返す関数。図形の fill に入れると GPU で走る",
    },
    Type {
        name: "View",
        category: "Scene",
        union: "",
        make: "View(box = Vector(16, 9))",
        values: &[],
        members: &[],
        doc: "座標系を持つ入れ物。中に図形や View を place し、addTrack で Timeline を付ける。output した View が動画になる",
    },
    Type {
        name: "Timeline",
        category: "Time",
        union: "",
        make: "Timeline()\nTimeline(duration = 8s)\nmotion (t) {\n  0s: c.radius = 1\n  2s: c.radius = 3\n}",
        values: &[],
        members: &[],
        doc: "どの対象のどの属性が、いつ、どの値になるかの並び。行に属性への代入を書いた motion がこれになる。別の Timeline、Audio、Subtitle も place で時刻に置ける",
    },
    Type {
        name: "Motion",
        category: "Time",
        union: "",
        make: "motion (t) {\n  0s: 1\n  2s: 3\n}",
        values: &[],
        members: &[],
        doc: "時刻と値の表。どの対象のどの属性に入れるかは持たない。apply がそれを決めて Timeline にする",
    },
    Type {
        name: "Audio",
        category: "Media",
        union: "",
        make: "import \"bgm.m4a\" as bgm",
        values: &[],
        members: &[],
        doc: "読み込んだ音声ファイル。Timeline に置くと動画の音になる",
    },
    Type {
        name: "Subtitle",
        category: "Media",
        union: "",
        make: "Subtitle(text = \"ここに字幕\", duration = 2s)",
        values: &[],
        members: &[],
        doc: "字幕。画面には描かず、Timeline に置くと動画の字幕トラックになる",
    },
    Type {
        name: "Anchor",
        category: "Enum",
        union: "",
        make: "",
        values: &[
            (":center", "中央。書かなければこれ"),
            (":topLeft", "左上の角"),
            (":top", "上辺の中央"),
            (":topRight", "右上の角"),
            (":left", "左辺の中央"),
            (":right", "右辺の中央"),
            (":bottomLeft", "左下の角"),
            (":bottom", "下辺の中央"),
            (":bottomRight", "右下の角"),
        ],
        members: &[],
        doc: "Pos の anchor。その位置が、置くものの どこ にあたるかを決める",
    },
    Type {
        name: "Align",
        category: "Enum",
        union: "",
        make: "",
        values: &[(":left", "左に寄せる。書かなければこれ"), (":center", "中央に寄せる"), (":right", "右に寄せる")],
        members: &[],
        doc: "TextArea の align。w で折り返した各行を、幅の中でどちらに寄せるか",
    },
    Type {
        name: "Easing",
        category: "Enum",
        union: "",
        make: "",
        values: &[
            (":linear", "ずっと同じ速さで変わる。書かなければこれ"),
            (":ease_in", "止まった状態から始まり、だんだん速くなる"),
            (":ease_out", "速く始まり、だんだん遅くなって止まる"),
            (":ease", "両端が遅く、真ん中が速い"),
        ],
        members: &[],
        doc: "motion の行末に書く修飾子。前の行からこの行までの区間で、値がどう変わるかを決める。時刻と値は変えず、その間の通り方だけを変える。属性の型ではなく、行末にだけ書ける。名前は CSS の easing キーワードと同じ",
    },
    Type {
        name: "StrokeCap",
        category: "Enum",
        union: "",
        make: "",
        values: &[
            (":butt", "端でそのまま切る。書かなければこれ"),
            (":round", "端から半円ぶんはみ出す"),
            (":square", "端から線の太さの半分だけ四角くはみ出す"),
        ],
        members: &[],
        doc: "線の両端の形。strokeCap に渡す",
    },
    Type {
        name: "StrokeJoin",
        category: "Enum",
        union: "",
        make: "",
        values: &[(":miter", "外側をとがらせる。書かなければこれ"), (":round", "外側を丸める"), (":bevel", "外側の角を落として平らにする")],
        members: &[],
        doc: "折れ曲がった線の、角の形。strokeJoin に渡す",
    },
    Type {
        name: "Blend",
        category: "Enum",
        union: "",
        make: "",
        values: &[
            (":normal", "そのまま上に重ねる。書かなければこれ"),
            (":multiply", "成分どうしを掛ける。全体が暗くなる"),
            (":screen", "反転して掛け、戻す。全体が明るくなる"),
            (":overlay", "下が暗い所は :multiply、明るい所は :screen。明暗の差が強くなる"),
            (":darken", "成分ごとに暗い方を採る"),
            (":lighten", "成分ごとに明るい方を採る"),
            (":difference", "成分ごとの差の絶対値。同じ色どうしは黒になる"),
            (":add", "成分どうしを足す。光を重ねたように明るくなる"),
        ],
        members: &[],
        doc: "図形や View を、下にある絵とどう混ぜるか。blend に渡す",
    },
    Type {
        name: "GradientKind",
        category: "Enum",
        union: "",
        make: "",
        values: &[
            (":linear", "from から to へまっすぐ変わる。書かなければこれ"),
            (":radial", "from を中心に、radius の距離まで外へ広がる"),
            (":sweep", "from を中心に、角度に沿って 1 周する"),
        ],
        members: &[],
        doc: "Gradient の kind。stops の色をどの向きに並べるか",
    },
    Type {
        name: "Shape",
        category: "Union",
        union: "",
        make: "",
        values: &[],
        members: &["Circle", "Ellipse", "Rect", "Line", "Polygon", "Path", "TextArea"],
        doc: "描ける図形をまとめた名前。View に place できて、fill や opacity など図形に共通の属性を持つ",
    },
    Type {
        name: "Placeable",
        category: "Union",
        union: "",
        make: "",
        values: &[],
        members: &["Shape", "View"],
        doc: "View の中に place できるものをまとめた名前。図形と View",
    },
    Type {
        name: "Paint",
        category: "Union",
        union: "",
        make: "",
        values: &[],
        members: &["Color", "Gradient", "Shader"],
        doc: "図形の fill に入れられるものをまとめた名前",
    },
    Type {
        name: "Module",
        category: "Meta",
        union: "",
        make: "import math\nimport .file as m\nimport \"path/file.moph\" as m",
        values: &[],
        members: &[],
        doc: "import が束縛するもの。そのファイルが export した名前と、output した View を持つ。import { name } from math と書くと、Module を作らずに名前だけを持ち込む",
    },
    Type {
        name: "Type",
        category: "Meta",
        union: "",
        make: "Circle\nalias P = Pos",
        values: &[],
        members: &[],
        doc: "型そのものを指す値。型名を書くとこの値になり、呼ぶとその型の値を作る。alias で別の名前を付けられる。struct と record が作る型もこれ",
    },
    Type {
        name: "Nothing",
        category: "Meta",
        union: "",
        make: "",
        values: &[],
        members: &[],
        doc: "値を返さなかった関数の戻り値。書き方は無く、log や place のように結果を持たない呼び出しがこれを返す",
    },
];

/// 属性の説明。(型, 属性, 説明)。型が "*" なら図形に共通
pub const ATTRS: &[(&str, &str, &str)] = &[
    ("*", "position", "位置と基準点 (Pos)"),
    ("*", "fill", "塗り。Color か Shader"),
    ("*", "stroke", "線の色"),
    ("*", "strokeWidth", "線の太さ (箱の座標の単位)"),
    ("*", "strokeCap", "線の端の形。:butt (既定) :round :square"),
    ("*", "strokeJoin", "線の角の形。:miter (既定) :round :bevel"),
    ("*", "dash", "破線。線と間の長さを順に並べた Number の List (例 [0.6, 0.4])"),
    ("*", "dashOffset", "破線の始まりをずらす長さ。motion で動かすと破線が流れる"),
    ("*", "blend", "下の絵との重ね方。:normal (既定) :multiply :screen :overlay :darken :lighten :difference :add"),
    ("*", "opacity", "不透明度 0..1"),
    ("*", "rotation", "時計回りの回転 (度)"),
    ("*", "pivot", "回転の中心 (箱の座標)。書かなければ、その図形を囲む四角形の中心"),
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
            // 属性は 2 か所から来る: Object の型は schema、値の型は attr の表
            let mut attrs: Vec<Json> = schema(t.name).unwrap_or(&[]).iter().map(|(a, ty)| json!({ "name": a, "type": ty, "doc": attr_doc(t.name, a) })).collect();
            attrs.extend(
                crate::lang::attr::ATTRS
                    .iter()
                    .filter(|a| a.receivers.contains(&t.name))
                    .map(|a| json!({ "name": a.name, "type": a.ty, "doc": a.doc })),
            );
            let methods: Vec<Json> = crate::lang::method::METHODS
                .iter()
                .filter(|m| m.receivers.contains(&t.name))
                .map(|m| json!({ "name": m.name, "signature": m.signature, "returns": m.returns, "doc": m.doc }))
                .collect();
            let values: Vec<Json> = t.values.iter().map(|(v, d)| json!({ "value": v, "doc": d })).collect();
            json!({ "name": t.name, "category": t.category, "union": t.union, "make": t.make, "values": values, "members": t.members, "doc": t.doc, "attrs": attrs, "methods": methods })
        })
        .collect();
    let errors: Vec<Json> = crate::lang::error::KINDS.iter().map(|k| json!({ "name": k.name(), "doc": k.doc() })).collect();
    json!({ "builtins": entries(BUILTINS), "math": entries(crate::stdlib::math::DOCS), "types": types, "categories": CATEGORIES, "errors": errors })
}
