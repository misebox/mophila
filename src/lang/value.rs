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

#[derive(Clone)]
pub enum Value {
    Number(f64),
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
    /// let の型注釈 (type_of の表示用)
    pub type_text: Option<String>,
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
        duration_of(self.relative, self.duration.get(), self.keyframes.iter().map(|k| k.end.unwrap_or(k.time)))
    }

    /// 書かれた時刻 → 実際の時刻 の倍率。duration を変えると全体が伸縮する
    pub fn time_scale(&self) -> f64 {
        let span = natural_span(self.relative, self.keyframes.iter().map(|k| k.end.unwrap_or(k.time)));
        if span == 0.0 { 1.0 } else { self.duration() / span }
    }

    pub fn needs_duration(&self) -> bool {
        self.relative && self.duration.get().is_none()
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
        Timeline { param: self.param.clone(), keyframes, relative: self.relative, duration: Cell::new(self.duration.get()) }
    }
}

impl Value {
    pub fn type_name(&self) -> String {
        match self {
            Value::Number(_) => "Number".into(),
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
            Value::Func(c) => c.type_text.clone().unwrap_or_else(|| "Func".into()),
            Value::Record(r) => r.name.clone(),
            Value::Type(_) | Value::BuiltinType(_) => "Type".into(),
            Value::Module(_) => "Module".into(),
            Value::Builtin(_) => "Func".into(),
            Value::Audio(_) => "Audio".into(),
            Value::Nothing => "Nothing".into(),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(v) => write!(f, "{v}"),
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
                write!(f, "({})", parts.join(", "))
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
            Value::Func(c) => write!(f, "func ({} params)", c.def.params.len()),
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
