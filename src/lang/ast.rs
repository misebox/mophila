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
    Length(crate::lang::length::Length),
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
    Dict(Vec<(DictKey, Expr)>),
    /// progress [:x, :y] { 0: 2, 4.5 ; 1: 14 :ease_out, 4.5 }
    Progress(Box<ProgressDef>),
}

/// progress の表。列の名前を先に並べ、行には値だけを書く
#[derive(Debug, Clone)]
pub struct ProgressDef {
    pub columns: Vec<String>,
    pub rows: Vec<ProgressRow>,
}

#[derive(Debug, Clone)]
pub struct ProgressRow {
    /// 0 から 1 の位置
    pub at: Expr,
    /// 列ごとの値と、その値へ向かう曲線。`_` は None
    pub cells: Vec<Option<(Expr, Option<String>)>>,
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
    /// f(...xs) の形。Tuple / List / Range を並びのまま引数に広げる
    pub spread: bool,
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

/// 型注釈。`List<Vector>` なら name が "List"、args が [Vector]。
/// 決まった Symbol は name が ":center"、関数の型は name が "Func"
#[derive(Debug, Clone)]
pub struct TypeAnn {
    pub name: String,
    /// < > の中。書いていなければ空
    pub args: Vec<TypeAnn>,
    /// 書いたままの姿。エラーに出す
    pub text: String,
}

impl TypeAnn {
    /// < > の無い型
    pub fn plain(name: &str) -> TypeAnn {
        TypeAnn { name: name.to_string(), args: Vec::new(), text: name.to_string() }
    }
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
    TypeDef(String, Vec<TypeAnn>),
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
    /// 条件が満たされている間くり返す
    While(Expr, Vec<Stmt>),
    /// for や while を抜ける
    Break,
    Return(Expr),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Name(String),
    Tuple(Vec<Pattern>),
    List(Vec<Pattern>),
}

/// 引数の見出しに使う名前
pub fn pattern_text(p: &Pattern) -> String {
    match p {
        Pattern::Name(n) => n.clone(),
        Pattern::Tuple(items) => format!("({})", items.iter().map(pattern_text).collect::<Vec<_>>().join(", ")),
        Pattern::List(items) => format!("[{}]", items.iter().map(pattern_text).collect::<Vec<_>>().join(", ")),
    }
}
