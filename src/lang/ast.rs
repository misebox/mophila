use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    Range,
    RangeInclusive,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Number(f64),
    Duration(f64),
    Color([f32; 4]),
    Str(String),
    Symbol(String),
    Bool(bool),
    Ident(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// a < b <= c。真ん中は 1 度だけ評価し、偽が出たらそこで止める
    Compare(Box<Expr>, Vec<(BinOp, Expr)>),
    Attr(Box<Expr>, String),
    Call(Box<Expr>, Vec<Arg>),
    Index(Box<Expr>, Box<Expr>),
    Tuple(Vec<Expr>),
    List(Vec<Expr>),
    If(Box<Expr>, Vec<Stmt>, Option<Vec<Stmt>>),
    Func(Rc<FuncDef>),
    /// context a as x, b as y { }
    Context(Vec<(Expr, String)>, Vec<Stmt>),
    Dict(Vec<(DictKey, Expr)>),
    Motion(Box<MotionDef>),
}

#[derive(Debug)]
pub struct FuncDef {
    pub params: Vec<Param>,
    /// -> の後に書いた戻り値の型
    pub returns: Option<TypeAnn>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub pattern: Pattern,
    /// 名前の後に書いた型
    pub ann: Option<TypeAnn>,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct Arg {
    pub name: Option<String>,
    pub value: Expr,
}

/// motion 式。3 つの形がある:
/// - `motion (t, a, b) { 1s: t*3, a*1, b*3 }`  値の表 (Motion)
/// - `motion (t) { 1s: o.radius = t }`         属性への割り当て (Timeline)
/// - `motion c1 [:position, :radius] { 0s: ..., 0.5 }`  対象と属性パスを先に列挙 (Timeline)
#[derive(Debug, Clone)]
pub struct MotionDef {
    pub params: Vec<String>,
    /// 対象と属性パス (3 つ目の形)
    pub target: Option<(Expr, Vec<Vec<String>>)>,
    pub rows: Vec<MotionRow>,
}

#[derive(Debug, Clone)]
pub struct MotionRow {
    /// 時刻。relative なら 0..1 の割合、そうでなければ秒
    pub time: f64,
    /// `0..1:` のように範囲で書いた行の終わり。この区間は補間せず、式を毎フレーム評価する
    pub end: Option<f64>,
    pub relative: bool,
    pub items: Vec<RowItem>,
    pub ease: Option<String>,
}

#[derive(Debug, Clone)]
pub enum RowItem {
    /// 対象, 属性パス, 値
    Assign(Expr, Vec<String>, Expr),
    Value(Expr),
}

/// import の元。標準ライブラリ名か、ファイル (import する側からの相対パス)
#[derive(Debug, Clone)]
pub enum ImportSource {
    Std(String),
    File(String),
}

#[derive(Debug, Clone)]
pub enum ImportKind {
    /// import math / import .slides [as name] — モジュールを 1 つの名前に束縛する
    Module { source: ImportSource, alias: String },
    /// import { a, b } from .slides — 名前を直接持ち込む
    Names { source: ImportSource, names: Vec<String> },
}

#[derive(Debug, Clone)]
pub enum DictKey {
    Str(String),
    /// { x } は { "x": x } の省略形
    Shorthand(String),
}

/// 型注釈。name は型の名前 (関数なら "Func")、text は表示用の全文 (例: "(Number) -> Number")
/// struct Name { フィールド、func、method }
#[derive(Debug, Clone)]
pub struct TypeDecl {
    pub name: String,
    /// record または @immutable。値になる
    pub immutable: bool,
    pub nocopy: bool,
    pub nodeepcopy: bool,
    /// @deprecated("代わりの書き方")。作るときに一度だけ警告を出す
    pub deprecated: Option<String>,
    pub fields: Vec<FieldDecl>,
    pub members: Vec<MemberDecl>,
}

#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub name: String,
    pub ann: TypeAnn,
    pub default: Option<Expr>,
    pub private: bool,
}

/// func は受け手なし、method は第 1 引数が受け手
#[derive(Debug, Clone)]
pub struct MemberDecl {
    pub name: String,
    pub private: bool,
    pub receiver: bool,
    pub def: Rc<FuncDef>,
}

#[derive(Debug, Clone)]
pub struct TypeAnn {
    pub name: String,
    pub text: String,
}

/// 文と、その開始行
#[derive(Debug, Clone)]
pub struct Stmt {
    pub line: usize,
    pub kind: StmtKind,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Let(Pattern, Option<TypeAnn>, Expr),
    /// type Name = A | B
    TypeDef(String, Vec<String>),
    /// record name(field: Type, ...)
    /// struct / record の宣言
    TypeDecl(Rc<TypeDecl>),
    /// import name  /  import "file.moph" as name
    Import(ImportKind),
    /// export let / export func。import した側に公開する
    Export(Box<Stmt>),
    AssignIndex(Expr, Expr, Expr),
    /// a, b = x, y
    AssignMulti(Vec<Expr>, Vec<Expr>),
    AssignVar(String, Expr),
    AssignAttr(Expr, String, Expr),
    Output(Expr),
    For(Pattern, Expr, Vec<Stmt>),
    Return(Expr),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Name(String),
    Tuple(Vec<Pattern>),
    List(Vec<Pattern>),
}

impl Expr {
    /// 評価済みの値を、評価すると同じ値になる式にする。Motion.apply が記録するキーフレーム用
    pub fn from_value(v: &crate::lang::value::Value) -> Expr {
        use crate::lang::value::Value;
        match v {
            Value::Number(n, _) => Expr::Number(*n),
            Value::Duration(d) => Expr::Duration(*d),
            Value::Color(c) => Expr::Color(*c),
            Value::Str(s) => Expr::Str(s.clone()),
            Value::Symbol(s) => Expr::Symbol(s.clone()),
            Value::Bool(b) => Expr::Bool(*b),
            Value::Vector(x, y) => construct("Vector", vec![Expr::Number(*x), Expr::Number(*y)]),
            Value::Apos(a, x, y) => construct("Pos", vec![Expr::Number(*x), Expr::Number(*y), Expr::Symbol(a.clone())]),
            Value::Tuple(items) => Expr::Tuple(items.iter().map(Expr::from_value).collect()),
            _ => Expr::Ident(format!("<{}>", v.type_name())),
        }
    }
}

/// 型名を呼ぶ式
fn construct(name: &str, args: Vec<Expr>) -> Expr {
    let args = args.into_iter().map(|value| Arg { name: None, value }).collect();
    Expr::Call(Box::new(Expr::Ident(name.to_string())), args)
}

/// 引数の見出しに使う名前
pub fn pattern_text(p: &Pattern) -> String {
    match p {
        Pattern::Name(n) => n.clone(),
        Pattern::Tuple(items) => format!("({})", items.iter().map(pattern_text).collect::<Vec<_>>().join(", ")),
        Pattern::List(items) => format!("[{}]", items.iter().map(pattern_text).collect::<Vec<_>>().join(", ")),
    }
}
