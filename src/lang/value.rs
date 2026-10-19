use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::fmt::Write as _;
use std::rc::Rc;

use crate::lang::ast::FuncDef;

pub type ObjRef = Rc<RefCell<Object>>;
/// スコープの列。内側ほど後ろ。クロージャは同じスコープを共有する (複製しない)
pub type Scope = Rc<RefCell<HashMap<String, Value>>>;
pub type Scopes = Vec<Scope>;

/// Number だけを詰めた配列。Rust の関数が作り、Rust の関数や Shader.args にそのまま渡す。要素ごとの Value を持たない
#[derive(Debug)]
pub struct Array {
    pub nums: Vec<f64>,
    /// GPU に送る f32 の写し。最初に要るときに 1 回だけ作る
    f32s: std::cell::OnceCell<Vec<f32>>,
}

impl Array {
    pub fn new(nums: Vec<f64>) -> Array {
        Array { nums, f32s: std::cell::OnceCell::new() }
    }

    pub fn f32s(&self) -> &[f32] {
        self.f32s.get_or_init(|| self.nums.iter().map(|x| *x as f32).collect())
    }
}

pub fn new_scope() -> Scope {
    Rc::new(RefCell::new(HashMap::new()))
}

/// 分数のまま持っている数。約分済みで、分母は正
#[derive(Clone, Copy, PartialEq)]
pub struct Ratio {
    pub num: i64,
    pub den: i64,
}

impl Ratio {
    /// 約分して作る。分母が 0 なら作らない
    pub fn new(num: i64, den: i64) -> Option<Ratio> {
        if den == 0 {
            return None;
        }
        let sign = if den < 0 { -1 } else { 1 };
        let g = gcd(num.unsigned_abs(), den.unsigned_abs()) as i64;
        Some(Ratio { num: sign * (num / g), den: sign * (den / g) })
    }

    pub fn value(self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

/// 10 進にすると長くなる数か
fn long_decimal(v: f64) -> bool {
    format!("{v}").split_once('.').is_some_and(|(_, frac)| frac.len() > 6)
}

fn gcd(a: u64, b: u64) -> u64 {
    match b {
        0 => a.max(1),
        _ => gcd(b, a % b),
    }
}

/// 数を 10 進で書き出す入れ物。f64 は "{}" で 24 桁を超えないので、これで足りる
struct Digits {
    data: [u8; 32],
    len: usize,
}

impl Digits {
    fn new() -> Self {
        Digits { data: [0; 32], len: 0 }
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.data[..self.len]).unwrap_or("")
    }
}

impl fmt::Write for Digits {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len + s.len();
        if end > self.data.len() {
            return Err(fmt::Error);
        }
        self.data[self.len..end].copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}


#[derive(Clone)]
pub enum Value {
/// 実際の値と、分数のままの姿。分数で表せる間だけ後ろを持つ
    Number(f64, Option<Ratio>),
    Bool(bool),
    Str(String),
    Symbol(String),
    Duration(f64),
    /// 単位つきの長さ。描く先の箱が決まってから数になる
    Length(crate::lang::length::Length),
    Color([f32; 4]),
    Vector(f64, f64),
    /// 成分に単位つきの長さを含む Vector。型としては Vector
    LVec(crate::lang::length::Length, crate::lang::length::Length),
    /// 3D の点。space3d の中だけで使う
    Vector3(f64, f64, f64),
    Tuple(Vec<Value>),
    List(Rc<RefCell<Vec<Value>>>),
    /// Number だけの配列。作ったら変えない
    Array(Rc<Array>),
    /// start..end (end は含まない)
    Range(i64, i64),
    /// 挿入順を保つ
    Dict(Rc<RefCell<Vec<(String, Value)>>>),
    Object(ObjRef),
    Func(Rc<Closure>),
    /// progress の表。0 から 1 の数で呼ぶと、その点の値の Tuple を返す
    Progress(Rc<ProgressTable>),
    /// record (name!(...) が作る値)
    Record(Rc<Record>),
    /// 型そのもの。呼ぶと値を作る
    Type(Rc<UserType>),
    /// builtin の型の名前
    BuiltinType(String),
    /// import で束縛されるモジュール
    Module(Rc<Module>),
    /// builtin 関数 (math.sin など)
    Builtin(&'static str),
    /// import "file.m4a" で読み込んだ音声ファイル
    Audio(Rc<Audio>),
    /// import "file.png" で読み込んだ画像
    Image(Rc<ImageFile>),
    Nothing,
}

/// 音声ファイル。長さは読み込み時に ffprobe で調べる
pub struct Audio {
    /// import に書いたパス
    pub name: String,
    /// 実際のファイル
    pub path: std::path::PathBuf,
    /// 秒
    pub length: f64,
    /// 音量。1 がそのまま
    pub volume: f64,
    /// `out` まで繰り返すか
    pub looping: bool,
}

impl Audio {
    /// Layer の長さ span のうち、実際に鳴る長さ。繰り返さなければファイルより長くならない
    pub fn played(&self, span: f64) -> f64 {
        if self.looping { span } else { span.min(self.length) }
    }
}

/// 画像ファイル。中身は描くときに読む (スクリプトを実行するだけなら開かない)
pub struct ImageFile {
    /// import に書いたパス
    pub name: String,
    /// 実際のファイル
    pub path: std::path::PathBuf,
    pub width: f64,
    pub height: f64,
}

pub struct Module {
    pub name: String,
    pub items: HashMap<String, Value>,
}

pub struct Record {
    pub name: String,
    /// ユーザーが宣言した型なら、その宣言
    pub decl: Option<Rc<UserType>>,
    pub fields: Vec<(String, Value)>,
}

/// struct / record の宣言と、宣言した場所のスコープ (メソッドの本体が外の名前を見るため)
pub struct UserType {
    pub decl: Rc<crate::lang::ast::TypeDecl>,
    pub scopes: Scopes,
}

/// 関数と、定義時のスコープ
pub struct Closure {
    pub def: Rc<FuncDef>,
    pub scopes: Scopes,
}

pub struct Object {
    /// 作った順の通し番号。キャッシュの鍵に使う。
    /// アドレスは、消えた実体の番地に別の実体が載ると同じ値になるので鍵にできない
    pub id: u64,
    pub kind: String,
    /// ユーザーが宣言した型なら、その宣言
    pub decl: Option<Rc<UserType>>,
    pub attrs: HashMap<String, Value>,
    /// Group の中身
    pub children: Vec<ObjRef>,
    /// Composition に置かれたもの。置いた順が積み重ねの順
    pub layers: Vec<Layer>,
}

/// Composition に置かれたもの 1 つ。`in` と `out` は親の時間軸で数える
#[derive(Clone)]
pub struct Layer {
    pub name: String,
    /// 不変の Node (図形、Group、Composition、Audio、Narration) か、`func (t) -> Node`
    pub content: Value,
    pub at: f64,
    pub out: f64,
}

impl Layer {
    /// t で存在するか
    pub fn exists_at(&self, t: f64) -> bool {
        self.at <= t && t < self.out
    }
}

/// Object に振る次の通し番号。フレームを組むスレッドをまたいでも重ならない
pub fn next_object_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// progress の表。列ごとに、キーフレームの (位置, 値, その値へ向かう曲線) を位置の順に持つ
pub struct ProgressTable {
    pub columns: Vec<String>,
    pub tracks: Vec<Vec<(f64, Value, Option<String>)>>,
}

impl ProgressTable {
    /// 位置 p (0..1) の値の Tuple。範囲の外は端の値
    pub fn at(&self, p: f64) -> Value {
        let cell = |keys: &Vec<(f64, Value, Option<String>)>| -> Value {
            let (first, last) = (&keys[0], &keys[keys.len() - 1]);
            if p <= first.0 {
                return first.1.clone();
            }
            if p >= last.0 {
                return last.1.clone();
            }
            let i = keys.iter().rposition(|k| k.0 <= p).expect("p is after the first key");
            let (a, b) = (&keys[i], &keys[i + 1]);
            let k = crate::lang::curve::shape(b.2.as_deref(), (p - a.0) / (b.0 - a.0));
            crate::lang::eval::interpolate(&a.1, &b.1, k)
        };
        Value::Tuple(self.tracks.iter().map(cell).collect())
    }
}


impl std::fmt::Debug for Value {
    /// 中身にスコープや関数が入っているので、型名だけ出す
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({self})", self.type_name())
    }
}

impl Value {
    /// 型の名前。Object と Record 以外は借りられるので、確保しないで比べられる
    pub fn type_name_ref(&self) -> Option<&'static str> {
        Some(match self {
            Value::Number(..) => "Number",
            Value::Bool(_) => "Bool",
            Value::Str(_) => "String",
            Value::Symbol(_) => "Symbol",
            Value::Duration(_) => "Duration",
            Value::Length(_) => "Length",
            Value::Color(_) => "Color",
            Value::Vector(..) | Value::LVec(..) => "Vector",
            Value::Vector3(..) => "Vector3",
            Value::Tuple(_) => "Tuple",
            Value::List(_) => "List",
            Value::Array(_) => "Array",
            Value::Range(..) => "Range",
            Value::Dict(_) => "Dict",
            Value::Func(_) | Value::Builtin(_) => "Func",
            Value::Progress(_) => "Progress",
            Value::Audio(_) => "Audio",
            Value::Image(_) => "Image",
            Value::Nothing => "Nothing",
            // 名前が実体に入っているもの
            Value::Object(_) | Value::Record(_) | Value::Type(_) | Value::BuiltinType(_) | Value::Module(_) => return None,
        })
    }

    pub fn type_name(&self) -> String {
        match self {
            Value::Number(..) => "Number".into(),
            Value::Bool(_) => "Bool".into(),
            Value::Str(_) => "String".into(),
            Value::Symbol(_) => "Symbol".into(),
            Value::Duration(_) => "Duration".into(),
            Value::Length(_) => "Length".into(),
            Value::Color(_) => "Color".into(),
            Value::Vector(..) | Value::LVec(..) => "Vector".into(),
            Value::Vector3(..) => "Vector3".into(),
            Value::Tuple(_) => "Tuple".into(),
            Value::List(_) => "List".into(),
            Value::Array(_) => "Array".into(),
            Value::Range(..) => "Range".into(),
            Value::Dict(_) => "Dict".into(),
            Value::Object(o) => o.borrow().kind.clone(),
            Value::Func(_) => "Func".into(),
            Value::Record(r) => r.name.clone(),
            Value::Type(_) | Value::BuiltinType(_) => "Type".into(),
            Value::Module(_) => "Module".into(),
            Value::Builtin(_) => "Func".into(),
            Value::Progress(_) => "Progress".into(),
            Value::Audio(_) => "Audio".into(),
            Value::Image(_) => "Image".into(),
            Value::Nothing => "Nothing".into(),
        }
    }
}

impl Value {
    /// 長さとして読む。Number は単位なしの長さ (箱の単位)
    pub fn as_length(&self) -> Option<crate::lang::length::Length> {
        match self {
            Value::Number(n, _) => Some(crate::lang::length::Length::bare(*n)),
            Value::Length(l) => Some(*l),
            _ => None,
        }
    }

    /// 成分が長さの Vector として読む
    pub fn as_lvec(&self) -> Option<(crate::lang::length::Length, crate::lang::length::Length)> {
        use crate::lang::length::Length;
        match self {
            Value::Vector(x, y) => Some((Length::bare(*x), Length::bare(*y))),
            Value::LVec(x, y) => Some((*x, *y)),
            _ => None,
        }
    }

    /// 成分が長さなら LVec、全部単位なしなら Vector にする
    pub fn vector_of(x: crate::lang::length::Length, y: crate::lang::length::Length) -> Value {
        match (x.as_bare(), y.as_bare()) {
            (Some(x), Some(y)) => Value::Vector(x, y),
            _ => Value::LVec(x, y),
        }
    }

    /// 分数として持たない数
    pub fn num(v: f64) -> Value {
        Value::Number(v, Ratio::new_from(v))
    }

    /// 分数のまま持つ数
    pub fn ratio(num: i64, den: i64) -> Option<Value> {
        Ratio::new(num, den).map(|r| Value::Number(r.value(), Some(r)))
    }

}

impl Ratio {
    /// 短い 10 進で書ける数は、その分数として持つ。1/3 のように書けないものは持たない
    fn new_from(v: f64) -> Option<Ratio> {
        if !v.is_finite() || v.abs() >= 9.0e15 {
            return None;
        }
        if v.fract() == 0.0 {
            return Some(Ratio { num: v as i64, den: 1 });
        }
        // 数を 1 つ作るたびに通るので、桁はスタックに書いて読む (ヒープを使わない)
        let mut buf = Digits::new();
        write!(buf, "{v}").ok()?;
        let text = buf.as_str();
        let (int, frac) = text.split_once('.')?;
        // 指数表記や、長すぎる小数は分数にしない
        if frac.len() > 9 || !frac.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let den = 10_i64.checked_pow(frac.len() as u32)?;
        let mut digits: i64 = 0;
        for b in int.bytes().chain(frac.bytes()) {
            if b == b'-' {
                continue;
            }
            digits = digits.checked_mul(10)?.checked_add(i64::from(b.wrapping_sub(b'0')))?;
        }
        let num = if text.starts_with('-') { -digits } else { digits };
        Ratio::new(num, den)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // 10 進で短く書けない数だけ分数で出す。0.5 は 0.5、1/3 は 1/3
            Value::Number(v, Some(r)) if r.den != 1 && long_decimal(*v) => write!(f, "{}/{}", r.num, r.den),
            Value::Number(v, _) => write!(f, "{v}"),
            Value::Bool(v) => write!(f, "{v}"),
            Value::Str(s) => write!(f, "{s}"),
            Value::Symbol(s) => write!(f, ":{s}"),
            Value::Duration(v) => write!(f, "{v}s"),
            Value::Length(l) => write!(f, "{l}"),
            Value::LVec(x, y) => write!(f, "Vector({x}, {y})"),
            Value::Color([r, g, b, a]) => {
                let ch = |v: f32| (v * 255.0).round() as u8;
                write!(f, "#{:02x}{:02x}{:02x}", ch(*r), ch(*g), ch(*b))?;
                if *a < 1.0 { write!(f, "{:02x}", ch(*a)) } else { Ok(()) }
            }
            Value::Vector(x, y) => write!(f, "Vector({x}, {y})"),
            Value::Vector3(x, y, z) => write!(f, "Vector3({x}, {y}, {z})"),
            Value::Tuple(items) => {
                let parts: Vec<String> = items.iter().map(|v| v.to_string()).collect();
                // 要素 1 つは括弧付きの式と見分けが付くように (1,)
                match parts.len() {
                    1 => write!(f, "({},)", parts[0]),
                    _ => write!(f, "({})", parts.join(", ")),
                }
            }
            Value::List(items) => {
                let parts: Vec<String> = items.borrow().iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", parts.join(", "))
            }
            Value::Array(a) => {
                let shown: Vec<String> = a.nums.iter().take(16).map(|x| x.to_string()).collect();
                match a.nums.len() > shown.len() {
                    true => write!(f, "Array[{}, … ({} 個)]", shown.join(", "), a.nums.len()),
                    false => write!(f, "Array[{}]", shown.join(", ")),
                }
            }
            Value::Range(a, b) => write!(f, "{a}..{b}"),
            Value::Dict(entries) => {
                let parts: Vec<String> = entries.borrow().iter().map(|(k, v)| format!("{k:?}: {v}")).collect();
                write!(f, "{{ {} }}", parts.join(", "))
            }
            Value::Object(o) => {
                let o = o.borrow();
                let mut attrs: Vec<_> = o.attrs.iter().collect();
                attrs.sort_by(|a, b| a.0.cmp(b.0));
                write!(f, "{} {{", o.kind)?;
                for (i, (k, v)) in attrs.iter().enumerate() {
                    write!(f, "{}{k}: {v}", if i == 0 { " " } else { ", " })?;
                }
                write!(f, " }}")
            }
            Value::Progress(p) => write!(f, "progress [{}]", p.columns.iter().map(|c| format!(":{c}")).collect::<Vec<_>>().join(", ")),
            // 書いてある通りの見出し。型と既定値があればそれも出す
            Value::Func(c) => {
                let params: Vec<String> = c
                    .def
                    .params
                    .iter()
                    .map(|p| {
                        let name = crate::lang::ast::pattern_text(&p.pattern);
                        let ann = p.ann.as_ref().map(|a| format!(": {}", a.text)).unwrap_or_default();
                        let default = p.default.as_ref().map(|_| " = ...".to_string()).unwrap_or_default();
                        format!("{name}{ann}{default}")
                    })
                    .collect();
                let returns = c.def.returns.as_ref().map(|r| format!(" -> {}", r.text)).unwrap_or_default();
                write!(f, "func ({}){returns}", params.join(", "))
            }
            Value::Record(r) => {
                let parts: Vec<String> = r.fields.iter().map(|(_, v)| v.to_string()).collect();
                write!(f, "{}({})", r.name, parts.join(", "))
            }
            Value::Type(t) => write!(f, "type {}", t.decl.name),
            Value::BuiltinType(n) => write!(f, "type {n}"),
            Value::Module(m) => write!(f, "module {}", m.name),
            Value::Builtin(name) => write!(f, "builtin {name}"),
            Value::Audio(a) => write!(f, "Audio {{ file: {:?}, duration: {}s }}", a.name, a.length),
            Value::Image(i) => write!(f, "Image {{ file: {:?}, size: {} x {} }}", i.name, i.width, i.height),
            Value::Nothing => write!(f, "nothing"),
        }
    }
}

