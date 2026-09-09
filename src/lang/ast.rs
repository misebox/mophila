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
    Attr(Box<Expr>, String),
    Call(Box<Expr>, Vec<Arg>),
    Index(Box<Expr>, Box<Expr>),
    /// name!(args)
    Specific(String, Vec<Expr>),
    Tuple(Vec<Expr>),
    List(Vec<Expr>),
    If(Box<Expr>, Vec<Stmt>, Option<Vec<Stmt>>),
    Func(Rc<FuncDef>),
    New(String, Vec<(String, Expr)>),
    /// context a as x, b as y { }
    Context(Vec<(Expr, String)>, Vec<Stmt>),
    Dict(Vec<(DictKey, Expr)>),
    Motion(Box<MotionDef>),
}

#[derive(Debug)]
pub struct FuncDef {
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub pattern: Pattern,
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
    pub effect: Option<String>,
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

/// 型注釈。name は先頭の型名、text は表示用の全文 (例: "Func<Number -> Number>")
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
    /// tuple name(field: Type, ...)
    TupleDef(String, Vec<(String, String)>),
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
            Value::Number(n) => Expr::Number(*n),
            Value::Duration(d) => Expr::Duration(*d),
            Value::Color(c) => Expr::Color(*c),
            Value::Str(s) => Expr::Str(s.clone()),
            Value::Symbol(s) => Expr::Symbol(s.clone()),
            Value::Bool(b) => Expr::Bool(*b),
            Value::Vector(x, y) => Expr::Specific("vector".into(), vec![Expr::Number(*x), Expr::Number(*y)]),
            Value::Apos(a, x, y) => Expr::Specific("apos".into(), vec![Expr::Symbol(a.clone()), Expr::Number(*x), Expr::Number(*y)]),
            Value::Tuple(items) => Expr::Tuple(items.iter().map(Expr::from_value).collect()),
            _ => Expr::Ident(format!("<{}>", v.type_name())),
        }
    }
}
