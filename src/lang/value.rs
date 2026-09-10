use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::lang::ast::{Expr, FuncDef};

pub type ObjRef = Rc<RefCell<Object>>;
/// スコープの列。内側ほど後ろ。クロージャや Timeline は同じスコープを共有する (複製しない)
pub type Scope = Rc<RefCell<HashMap<String, Value>>>;
pub type Scopes = Vec<Scope>;

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

#[derive(Clone)]
pub enum Value {
    /// 実際の値と、分数のままの姿。分数で表せる間だけ後ろを持つ
    Number(f64, Option<Ratio>),
    Bool(bool),
    Str(String),
    Symbol(String),
    Duration(f64),
    Color([f32; 4]),
    Vector(f64, f64),
    /// (anchor, x, y)
    Apos(String, f64, f64),
    Tuple(Vec<Value>),
    List(Rc<RefCell<Vec<Value>>>),
    /// start..end (end は含まない)
    Range(i64, i64),
    /// 挿入順を保つ
    Dict(Rc<RefCell<Vec<(String, Value)>>>),
    Object(ObjRef),
    Timeline(Rc<Timeline>),
    Motion(Rc<Motion>),
    Func(Rc<Closure>),
    /// record (name!(...) が作る値)
    Record(Rc<Record>),
    /// 型そのもの。呼ぶと値を作る
    Type(Rc<UserType>),
    /// 組み込みの型の名前
    BuiltinType(String),
    /// import で束縛されるモジュール
    Module(Rc<Module>),
    /// 組み込み関数 (math.sin など)
    Builtin(&'static str),
    /// import "file.m4a" で読み込んだ音声ファイル
    Audio(Rc<Audio>),
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
    pub kind: String,
    /// ユーザーが宣言した型なら、その宣言
    pub decl: Option<Rc<UserType>>,
    pub attrs: HashMap<String, Value>,
    pub children: Vec<ObjRef>,
    /// View / Timeline に置かれた Timeline
    pub tracks: Vec<Placed>,
}

/// Timeline に置かれた 1 本。at は親の時間軸での開始時刻
#[derive(Clone)]
pub struct Placed {
    pub track: Track,
    pub at: f64,
    pub fade_in: f64,
    pub fade_out: f64,
}

#[derive(Clone)]
pub enum Track {
    Timeline(Rc<Timeline>),
    /// 入れ物の Timeline、または Timeline を持つ View (Object)
    Container(ObjRef),
    /// 音声。動画の音声トラックに混ぜる
    Audio(Rc<Audio>, Clip),
    /// 字幕 (Subtitle オブジェクト)。動画の字幕トラックに書く
    Subtitle(ObjRef),
}

/// 置いた音声の切り方と音量
#[derive(Clone)]
pub struct Clip {
    /// duration: で指定した長さ。繰り返さないときはファイルより長くならない
    pub cut: Option<f64>,
    pub volume: f64,
    /// loop: true。duration: か動画の終わりまで繰り返す
    pub looping: bool,
}

impl Audio {
    /// 時間軸の上で占める長さ。動画の終わりまで繰り返すものは長さを持たない (動画を延ばさない)
    pub fn clip_length(&self, clip: &Clip) -> f64 {
        match (clip.looping, clip.cut) {
            (true, cut) => cut.unwrap_or(0.0),
            (false, Some(c)) => c.min(self.length),
            (false, None) => self.length,
        }
    }
}

impl Placed {
    /// 親の時間軸での終わり
    pub fn end(&self) -> f64 {
        self.at + self.track.duration()
    }
}

impl Track {
    pub fn duration(&self) -> f64 {
        match self {
            Track::Timeline(tl) => tl.duration(),
            Track::Container(obj) => {
                let o = obj.borrow();
                match o.attrs.get("duration") {
                    Some(Value::Duration(d)) => *d,
                    _ => o.tracks.iter().map(|p| p.at + p.track.duration()).fold(0.0, f64::max),
                }
            }
            Track::Audio(audio, clip) => audio.clip_length(clip),
            Track::Subtitle(obj) => match obj.borrow().attrs.get("duration") {
                Some(Value::Duration(d)) => *d,
                _ => 0.0,
            },
        }
    }
}

/// 値だけの時間変化 (対象なし)
pub struct Motion {
    pub rows: Vec<MotionRowVal>,
    /// 行の時刻が 0..1 の割合か
    pub relative: bool,
    pub duration: Cell<Option<f64>>,
}

pub struct MotionRowVal {
    pub time: f64,
    pub values: Vec<Value>,
    pub ease: Option<String>,
}

impl Motion {
    pub fn duration(&self) -> f64 {
        duration_of(self.relative, self.duration.get(), self.rows.iter().map(|r| r.time))
    }

    pub fn normalize(mut self) -> Self {
        if self.rows.len() >= 2 {
            let ease = std::mem::take(&mut self.rows[0].ease);
            let second = &mut self.rows[1];
            second.ease = second.ease.take().or(ease);
        }
        self
    }

    /// 時間を逆にする。修飾子は同じ区間に付け直し、in/out を入れ替える
    /// 行の時刻を k 倍する
    pub fn scaled(&self, k: f64) -> Motion {
        let rows = self.rows.iter().map(|r| MotionRowVal { time: r.time * k, values: r.values.clone(), ease: r.ease.clone() }).collect();
        Motion { rows, relative: self.relative, duration: Cell::new(self.duration.get().map(|d| d * k)) }
    }

    /// 長さが d になるように行の時刻を伸縮する
    pub fn fit(&self, d: f64) -> Motion {
        let now = self.duration();
        let scaled = self.scaled(if now == 0.0 { 1.0 } else { d / now });
        scaled.duration.set(Some(d));
        scaled
    }

    /// from..to の行だけを取り出し、0 から始まる Motion にする
    pub fn trim(&self, from: f64, to: f64) -> Motion {
        let rows = self
            .rows
            .iter()
            .filter(|r| from <= r.time && r.time <= to)
            .map(|r| MotionRowVal { time: r.time - from, values: r.values.clone(), ease: r.ease.clone() })
            .collect();
        Motion { rows, relative: self.relative, duration: Cell::new(Some(to - from)) }
    }

    pub fn reverse(&self) -> Motion {
        let d = natural_span(self.relative, self.rows.iter().map(|r| r.time));
        let n = self.rows.len();
        let rows = (0..n)
            .map(|j| {
                let src = &self.rows[n - 1 - j];
                let ease = if j == 0 { None } else { self.rows[n - j].ease.as_deref().map(flip_name) };
                MotionRowVal { time: d - src.time, values: src.values.clone(), ease }
            })
            .collect();
        Motion { rows, relative: self.relative, duration: Cell::new(self.duration.get()) }
    }
}

fn modifiers(ease: &Option<String>) -> String {
    ease.as_ref().map(|m| format!(" :{m}")).unwrap_or_default()
}

/// 区間 (i-1, i) の修飾子は行 i に付いている。逆順では行 n-i の位置に来て、in と out が入れ替わる
fn flip_name(name: &str) -> String {
    match name {
        "ease_in" => "ease_out".into(),
        "ease_out" => "ease_in".into(),
        other => other.into(),
    }
}

/// Motion を対象に結び付けたもの。各割り当ては評価時の環境を持つ
pub struct Timeline {
    pub param: String,
    pub keyframes: Vec<TlKeyframe>,
    /// キーフレームの時刻が 0..1 の割合か
    pub relative: bool,
    pub duration: Cell<Option<f64>>,
    /// place で中に置いたもの。入れ物として使うときはこちらだけを持つ
    pub tracks: RefCell<Vec<Placed>>,
}

impl Timeline {
    /// 中身のない入れ物
    pub fn empty(duration: Option<f64>) -> Timeline {
        Timeline { param: "t".into(), keyframes: Vec::new(), relative: false, duration: Cell::new(duration), tracks: RefCell::new(Vec::new()) }
    }
}

/// 書かれた時刻の範囲。relative なら 1、そうでなければ最後の時刻
fn natural_span(relative: bool, times: impl Iterator<Item = f64>) -> f64 {
    if relative { 1.0 } else { times.fold(0.0, f64::max) }
}

/// 実際の長さ。relative で duration 未設定なら 0 (place / apply 時に DurationRequired にする)
fn duration_of(relative: bool, override_: Option<f64>, times: impl Iterator<Item = f64>) -> f64 {
    match (relative, override_) {
        (_, Some(d)) => d,
        (true, None) => 0.0,
        (false, None) => natural_span(false, times),
    }
}

pub struct TlKeyframe {
    pub time: f64,
    /// 範囲の行の終わり。この区間は式を毎フレーム評価する
    pub end: Option<f64>,
    pub assigns: Vec<TlAssign>,
    pub ease: Option<String>,
}

pub struct TlAssign {
    pub target: ObjRef,
    /// 属性パス。[position, x] なら position の中の x
    pub path: Vec<String>,
    pub expr: Expr,
    pub scopes: Scopes,
}

impl Timeline {
    pub fn duration(&self) -> f64 {
        let keyframes = duration_of(self.relative, self.duration.get(), self.keyframes.iter().map(|k| k.end.unwrap_or(k.time)));
        match self.duration.get() {
            Some(d) => d,
            // 入れ物として使っているときは、中に置いたものの終わりまで
            None => keyframes.max(self.tracks.borrow().iter().map(Placed::end).fold(0.0, f64::max)),
        }
    }

    /// 書かれた時刻 → 実際の時刻 の倍率。0..1 で書いた表だけ、duration が実時間を決める。
    /// 秒で書いた表はそのままの時刻で動く (duration は長さを決めるだけで、中身を動かさない)
    pub fn time_scale(&self) -> f64 {
        if !self.relative {
            return 1.0;
        }
        let d = self.duration();
        if d == 0.0 { 1.0 } else { d }
    }

    /// キーフレームだけを写した Timeline。時刻は f で移す
    fn mapped(&self, keep: impl Fn(&TlKeyframe) -> bool, f: impl Fn(f64) -> f64) -> Vec<TlKeyframe> {
        self.keyframes
            .iter()
            .filter(|k| keep(k))
            .map(|k| TlKeyframe {
                time: f(k.time),
                end: k.end.map(&f),
                assigns: k
                    .assigns
                    .iter()
                    .map(|a| TlAssign { target: a.target.clone(), path: a.path.clone(), expr: a.expr.clone(), scopes: a.scopes.clone() })
                    .collect(),
                ease: k.ease.clone(),
            })
            .collect()
    }

    /// 時間の軸を k 倍する。キーフレームの時刻と、中に置いたものの開始時刻が k 倍になる。
    /// 中に置いた Timeline も同じ倍率で伸縮する。音声と字幕は開始時刻だけ動き、長さは変わらない
    pub fn scaled(&self, k: f64) -> Timeline {
        let tracks = self
            .tracks
            .borrow()
            .iter()
            .map(|p| Placed {
                track: match &p.track {
                    Track::Timeline(tl) => Track::Timeline(Rc::new(tl.scaled(k))),
                    other => other.clone(),
                },
                at: p.at * k,
                fade_in: p.fade_in * k,
                fade_out: p.fade_out * k,
            })
            .collect();
        Timeline {
            param: self.param.clone(),
            keyframes: self.mapped(|_| true, |t| t * k),
            relative: self.relative,
            duration: Cell::new(self.duration.get().map(|d| d * k)),
            tracks: RefCell::new(tracks),
        }
    }

    /// 長さが d になるように時間の軸を伸縮した Timeline
    pub fn fit(&self, d: f64) -> Timeline {
        let now = self.duration();
        let scaled = self.scaled(if now == 0.0 { 1.0 } else { d / now });
        scaled.duration.set(Some(d));
        scaled
    }

    /// from..to の区間だけを取り出し、0 から始まる Timeline にする。
    /// その区間の外にあるキーフレームと、置いたものは入らない
    pub fn trim(&self, from: f64, to: f64) -> Timeline {
        let inside = |t: f64| from <= t && t <= to;
        let tracks = self
            .tracks
            .borrow()
            .iter()
            .filter(|p| inside(p.at) && inside(p.end()))
            .map(|p| Placed { track: p.track.clone(), at: p.at - from, fade_in: p.fade_in, fade_out: p.fade_out })
            .collect();
        Timeline {
            param: self.param.clone(),
            keyframes: self.mapped(|k| inside(k.time) && inside(k.end.unwrap_or(k.time)), |t| t - from),
            relative: self.relative,
            duration: Cell::new(Some(to - from)),
            tracks: RefCell::new(tracks),
        }
    }

    pub fn needs_duration(&self) -> bool {
        self.relative && self.duration.get().is_none() && !self.keyframes.is_empty()
    }

    /// 先頭行に付いた修飾子は最初の区間のものなので、2 行目に移す
    pub fn normalize(mut self) -> Self {
        if self.keyframes.len() >= 2 {
            let first = std::mem::take(&mut self.keyframes[0].ease);
            let second = &mut self.keyframes[1];
            second.ease = second.ease.take().or(first);
        }
        self
    }

    pub fn reverse(&self) -> Timeline {
        let d = natural_span(self.relative, self.keyframes.iter().map(|k| k.end.unwrap_or(k.time)));
        let n = self.keyframes.len();
        let keyframes = (0..n)
            .map(|j| {
                let src = &self.keyframes[n - 1 - j];
                let ease = if j == 0 { None } else { self.keyframes[n - j].ease.as_deref().map(flip_name) };
                let assigns = src
                    .assigns
                    .iter()
                    .map(|a| TlAssign { target: a.target.clone(), path: a.path.clone(), expr: a.expr.clone(), scopes: a.scopes.clone() })
                    .collect();
                // 範囲の行は (end, time) が (d - end, d - time) になる
                let (time, end) = match src.end {
                    Some(e) => (d - e, Some(d - src.time)),
                    None => (d - src.time, None),
                };
                TlKeyframe { time, end, assigns, ease }
            })
            .collect();
        Timeline { param: self.param.clone(), keyframes, relative: self.relative, duration: Cell::new(self.duration.get()), tracks: RefCell::new(self.tracks.borrow().clone()) }
    }
}

impl std::fmt::Debug for Value {
    /// 中身にスコープや関数が入っているので、型名だけ出す
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({self})", self.type_name())
    }
}

impl Value {
    pub fn type_name(&self) -> String {
        match self {
            Value::Number(..) => "Number".into(),
            Value::Bool(_) => "Bool".into(),
            Value::Str(_) => "String".into(),
            Value::Symbol(_) => "Symbol".into(),
            Value::Duration(_) => "Duration".into(),
            Value::Color(_) => "Color".into(),
            Value::Vector(..) => "Vector".into(),
            Value::Apos(..) => "Pos".into(),
            Value::Tuple(_) => "Tuple".into(),
            Value::List(_) => "List".into(),
            Value::Range(..) => "Range".into(),
            Value::Dict(_) => "Dict".into(),
            Value::Object(o) => o.borrow().kind.clone(),
            Value::Timeline(_) => "Timeline".into(),
            Value::Motion(_) => "Motion".into(),
            Value::Func(_) => "Func".into(),
            Value::Record(r) => r.name.clone(),
            Value::Type(_) | Value::BuiltinType(_) => "Type".into(),
            Value::Module(_) => "Module".into(),
            Value::Builtin(_) => "Func".into(),
            Value::Audio(_) => "Audio".into(),
            Value::Nothing => "Nothing".into(),
        }
    }
}

impl Value {
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
        let text = format!("{v}");
        let (int, frac) = text.split_once('.')?;
        // 指数表記や、長すぎる小数は分数にしない
        if frac.len() > 9 || !frac.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let den = 10_i64.checked_pow(frac.len() as u32)?;
        let digits: i64 = format!("{}{}", int.trim_start_matches('-'), frac).parse().ok()?;
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
            Value::Color([r, g, b, a]) => {
                let ch = |v: f32| (v * 255.0).round() as u8;
                write!(f, "#{:02x}{:02x}{:02x}", ch(*r), ch(*g), ch(*b))?;
                if *a < 1.0 { write!(f, "{:02x}", ch(*a)) } else { Ok(()) }
            }
            Value::Vector(x, y) => write!(f, "Vector({x}, {y})"),
            Value::Apos(a, x, y) => {
                if a == "center" { write!(f, "Pos({x}, {y})") } else { write!(f, "Pos({x}, {y}, anchor = :{a})") }
            }
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
            Value::Timeline(t) => {
                let unit = if t.relative { "" } else { "s" };
                let rows: Vec<String> = t
                    .keyframes
                    .iter()
                    .map(|k| match k.end {
                        Some(e) => format!("{}{unit}..{e}{unit}{}", k.time, modifiers(&k.ease)),
                        None => format!("{}{unit}{}", k.time, modifiers(&k.ease)),
                    })
                    .collect();
                write!(f, "Timeline {{ duration: {}s, keyframes: [{}] }}", t.duration(), rows.join(", "))
            }
            Value::Motion(m) => {
                let unit = if m.relative { "" } else { "s" };
                let rows: Vec<String> = m.rows.iter().map(|r| format!("{}{unit}{}", r.time, modifiers(&r.ease))).collect();
                write!(f, "Motion {{ duration: {}s, rows: [{}] }}", m.duration(), rows.join(", "))
            }
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
            Value::Nothing => write!(f, "nothing"),
        }
    }
}
