use std::rc::Rc;

use crate::lang::ast::{Arg, BinOp, DictKey, Expr, FieldDecl, FuncDef, ImportKind, ImportSource, MemberDecl, Param, Pattern, ProgressDef, ProgressRow, Stmt, StmtKind, TypeAnn, TypeDecl};
use crate::lang::error::{Kind, Result, err};
use crate::lang::lexer::{Tok, Token, lex};

pub fn parse(src: &str) -> Result<Vec<Stmt>> {
    let mut p = Parser { tokens: lex(src)?, pos: 0, loops: 0 };
    let stmts = p.stmts_until(&Tok::Eof)?;
    Ok(stmts)
}

/// 型注釈 1 つだけを読む。schema に書いた型名 ("List<Vector>") を構造にするのに使う
pub fn parse_type(src: &str) -> Result<TypeAnn> {
    let mut p = Parser { tokens: lex(src)?, pos: 0, loops: 0 };
    let ann = p.type_ann()?;
    match p.peek() {
        Tok::Eof | Tok::Newline => Ok(ann),
        _ => p.unexpected("end of type"),
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// いま何重の for の中か。break はこの中でしか書けない
    loops: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.tokens[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        &self.tokens[(self.pos + n).min(self.tokens.len() - 1)].tok
    }

    fn line(&self) -> usize {
        self.tokens[self.pos].line
    }

    fn col(&self) -> usize {
        self.tokens[self.pos].col
    }

    fn next(&mut self) -> Tok {
        let tok = self.tokens[self.pos].tok.clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, tok: Tok) -> Result<()> {
        if *self.peek() == tok {
            self.next();
            return Ok(());
        }
        self.unexpected(&format!("{tok:?}"))
    }

    fn unexpected<T>(&self, expected: &str) -> Result<T> {
        err(Kind::UnexpectedToken, format!("line {}:{}: expected {expected} but found {:?}", self.line(), self.col(), self.peek()))
    }

    fn ident(&mut self) -> Result<String> {
        match self.next() {
            Tok::Ident(name) => Ok(name),
            _ => {
                self.pos -= 1;
                self.unexpected("identifier")
            }
        }
    }

    fn skip_newlines(&mut self) {
        while *self.peek() == Tok::Newline {
            self.next();
        }
    }

    fn stmts_until(&mut self, end: &Tok) -> Result<Vec<Stmt>> {
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek() == end {
                self.next();
                return Ok(stmts);
            }
            stmts.push(self.stmt()?);
            match self.peek() {
                Tok::Newline => {}
                t if t == end => {}
                _ => return self.unexpected("end of statement"),
            }
        }
    }

    fn stmt(&mut self) -> Result<Stmt> {
        let line = self.line();
        self.stmt_kind().map(|kind| Stmt { line, kind })
    }

    fn stmt_kind(&mut self) -> Result<StmtKind> {
        match self.peek() {
            Tok::Let => {
                self.next();
                let pat = self.pattern()?;
                let ann = if *self.peek() == Tok::Colon {
                    self.next();
                    Some(self.type_ann()?)
                } else {
                    None
                };
                self.expect(Tok::Eq)?;
                Ok(StmtKind::Let(pat, ann, self.expr(0)?))
            }
            Tok::Export => {
                let line = self.line();
                self.next();
                if *self.peek() == Tok::AliasKw {
                    return err(Kind::UnexpectedToken, format!("line {line}: alias is a short name for this file only; it cannot be exported"));
                }
                // export { a, b } from <source> — 取り込んで、そのまま外にも出す
                if *self.peek() == Tok::LBrace {
                    let inner = Stmt { line, kind: StmtKind::Import(self.named_import()?) };
                    return Ok(StmtKind::Export(Box::new(inner)));
                }
                let inner = self.stmt()?;
                match inner.kind {
                    StmtKind::Let(..) | StmtKind::TypeDecl(..) => Ok(StmtKind::Export(Box::new(inner))),
                    _ => err(Kind::UnexpectedToken, format!("line {}: export must be followed by let, func, struct, record, or {{ names }} from", inner.line)),
                }
            }
            Tok::Import => {
                self.next();
                // import { a, b } from <source>
                if *self.peek() == Tok::LBrace {
                    return Ok(StmtKind::Import(self.named_import()?));
                }
                let (source, default) = self.import_source()?;
                let alias = self.import_alias(default)?;
                Ok(StmtKind::Import(ImportKind::Module { source, alias }))
            }
            Tok::RecordKw | Tok::StructKw => {
                let immutable = *self.peek() == Tok::RecordKw;
                self.next();
                self.type_decl(immutable, Vec::new())
            }
            Tok::At => {
                let mut anns: Vec<(String, Vec<Expr>)> = Vec::new();
                while *self.peek() == Tok::At {
                    self.next();
                    let name = self.ident()?;
                    let args = if *self.peek() == Tok::LParen {
                        self.next();
                        self.args()?.into_iter().map(|a| a.value).collect()
                    } else {
                        Vec::new()
                    };
                    if anns.iter().any(|(n, _)| *n == name) {
                        return err(Kind::UnexpectedToken, format!("@{name} is written twice"));
                    }
                    anns.push((name, args));
                    self.skip_newlines();
                }
                let immutable = match self.peek() {
                    Tok::RecordKw => true,
                    Tok::StructKw => false,
                    _ => return self.unexpected("struct or record"),
                };
                self.next();
                self.type_decl(immutable, anns)
            }
            Tok::AliasKw => {
                self.next();
                let name = self.ident()?;
                self.expect(Tok::Eq)?;
                Ok(StmtKind::Let(Pattern::Name(name), None, self.expr(0)?))
            }
            Tok::Type => {
                self.next();
                let name = self.ident()?;
                self.expect(Tok::Eq)?;
                let mut members = vec![self.type_member()?];
                while *self.peek() == Tok::Pipe {
                    self.next();
                    members.push(self.type_member()?);
                }
                Ok(StmtKind::TypeDef(name, members))
            }
            Tok::Output => {
                self.next();
                Ok(StmtKind::Output(self.expr(0)?))
            }
            Tok::Return => {
                self.next();
                Ok(StmtKind::Return(self.expr(0)?))
            }
            Tok::Break => {
                self.next();
                if self.loops == 0 {
                    return err(Kind::UnexpectedToken, format!("line {}:{}: break is only for inside a for", self.line(), self.col()));
                }
                Ok(StmtKind::Break)
            }
            // func name(params) { } は let name = func (params) { } と同じ
            Tok::Func if matches!(self.peek_at(1), Tok::Ident(_)) => {
                self.next();
                let name = self.ident()?;
                Ok(StmtKind::Let(Pattern::Name(name), None, self.func_expr()?))
            }
            Tok::For => {
                self.next();
                let pat = self.pattern()?;
                self.expect(Tok::In)?;
                let iter = self.expr(0)?;
                self.expect(Tok::LBrace)?;
                self.loops += 1;
                let body = self.stmts_until(&Tok::RBrace);
                self.loops -= 1;
                Ok(StmtKind::For(pat, iter, body?))
            }
            Tok::While => {
                self.next();
                let cond = self.expr(0)?;
                self.expect(Tok::LBrace)?;
                self.loops += 1;
                let body = self.stmts_until(&Tok::RBrace);
                self.loops -= 1;
                Ok(StmtKind::While(cond, body?))
            }
            _ => {
                let e = self.expr(0)?;
                if *self.peek() == Tok::Comma {
                    let mut targets = vec![e];
                    while *self.peek() == Tok::Comma {
                        self.next();
                        targets.push(self.expr(0)?);
                    }
                    self.expect(Tok::Eq)?;
                    let mut values = vec![self.expr(0)?];
                    while *self.peek() == Tok::Comma {
                        self.next();
                        values.push(self.expr(0)?);
                    }
                    if targets.len() != values.len() {
                        return err(Kind::ArityMismatch, format!("line {}:{}: {} targets but {} values", self.line(), self.col(), targets.len(), values.len()));
                    }
                    return Ok(StmtKind::AssignMulti(targets, values));
                }
                if *self.peek() != Tok::Eq {
                    return Ok(StmtKind::Expr(e));
                }
                self.next();
                let value = self.expr(0)?;
                match e {
                    Expr::Ident(name) => Ok(StmtKind::AssignVar(name, value)),
                    Expr::Attr(target, attr) => Ok(StmtKind::AssignAttr(*target, attr, value)),
                    Expr::Index(target, index) => Ok(StmtKind::AssignIndex(*target, *index, value)),
                    _ => err(Kind::UnexpectedToken, format!("line {}:{}: cannot assign to this expression", self.line(), self.col())),
                }
            }
        }
    }

    /// Pratt 構文解析。min_bp より強く結合する演算子だけを取り込む
    fn expr(&mut self, min_bp: u8) -> Result<Expr> {
        let mut lhs = self.prefix()?;
        loop {
            // 数字が大きいほど強く結合する。spec の優先順位表と同じ順
            let (op, l_bp, r_bp) = match self.peek() {
                Tok::Caret => (BinOp::Pow, 9, 9),
                Tok::Star => (BinOp::Mul, 8, 9),
                Tok::Slash => (BinOp::Div, 8, 9),
                Tok::Percent => (BinOp::Rem, 8, 9),
                Tok::Plus => (BinOp::Add, 7, 8),
                Tok::Minus => (BinOp::Sub, 7, 8),
                Tok::DotDot => (BinOp::Range, 6, 7),
                Tok::DotDotEq => (BinOp::RangeInclusive, 6, 7),
                Tok::Lt => (BinOp::Lt, 4, 5),
                Tok::Le => (BinOp::Le, 4, 5),
                Tok::Gt => (BinOp::Gt, 4, 5),
                Tok::Ge => (BinOp::Ge, 4, 5),
                Tok::EqEq => (BinOp::Eq, 4, 5),
                Tok::Ne => (BinOp::Ne, 4, 5),
                Tok::And => (BinOp::And, 3, 4),
                Tok::Or => (BinOp::Or, 2, 3),
                _ => break,
            };
            if l_bp < min_bp {
                break;
            }
            self.next();
            let rhs = self.expr(r_bp)?;
            // 比較は続けて書ける。a < b <= c は (a < b) and (b <= c)
            if compare_op(op) {
                let mut parts = vec![(op, rhs)];
                while let Some(next) = compare_of(self.peek()) {
                    self.next();
                    parts.push((next, self.expr(r_bp)?));
                }
                lhs = Expr::Compare(Box::new(lhs), parts);
                continue;
            }
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn prefix(&mut self) -> Result<Expr> {
        let e = match self.next() {
            Tok::Minus => Expr::Neg(Box::new(self.expr(10)?)),
            Tok::Not => Expr::Not(Box::new(self.expr(10)?)),
            Tok::True => Expr::Bool(true),
            Tok::False => Expr::Bool(false),
            Tok::Number(v) => Expr::Number(v),
            Tok::Duration(v) => Expr::Duration(v),
            Tok::Length(l) => Expr::Length(l),
            Tok::Color(c) => Expr::Color(c),
            Tok::Str(s) => Expr::Str(s),
            Tok::Symbol(s) => Expr::Symbol(s),
            // progress [:x, :y] { ... }。progress は名前としても使えるので、"[" と Symbol が続くときだけ表
            Tok::Ident(name) if name == "progress" && *self.peek() == Tok::LBracket && matches!(self.peek_at(1), Tok::Symbol(_)) => self.progress_expr()?,
            Tok::Ident(name) => Expr::Ident(name),
            Tok::LParen => {
                // () は空の Tuple
                if *self.peek() == Tok::RParen {
                    self.next();
                    return Ok(Expr::Tuple(Vec::new()));
                }
                let first = self.expr(0)?;
                if *self.peek() != Tok::Comma {
                    self.expect(Tok::RParen)?;
                    first
                } else {
                    let mut items = vec![first];
                    while *self.peek() == Tok::Comma {
                        self.next();
                        // (1,) のように最後にコンマを置ける
                        if *self.peek() == Tok::RParen {
                            break;
                        }
                        items.push(self.expr(0)?);
                    }
                    self.expect(Tok::RParen)?;
                    Expr::Tuple(items)
                }
            }
            Tok::LBracket => {
                let mut items = vec![];
                loop {
                    self.skip_newlines();
                    if *self.peek() == Tok::RBracket {
                        self.next();
                        break;
                    }
                    items.push(self.expr(0)?);
                    self.skip_newlines();
                    match self.peek() {
                        Tok::Comma => {
                            self.next();
                        }
                        Tok::RBracket => {}
                        _ => return self.unexpected("\",\" or \"]\""),
                    }
                }
                Expr::List(items)
            }
            Tok::If => self.if_expr()?,
            Tok::LBrace => self.dict_expr()?,
            Tok::Func => self.func_expr()?,
            _ => {
                self.pos -= 1;
                return self.unexpected("expression");
            }
        };
        self.postfix(e)
    }

    fn postfix(&mut self, mut e: Expr) -> Result<Expr> {
        loop {
            match self.peek() {
                Tok::Dot => {
                    self.next();
                    if let Tok::Number(n) = self.peek() {
                        let n = *n;
                        self.next();
                        e = Expr::Index(Box::new(e), Box::new(Expr::Number(n)));
                        continue;
                    }
                    // 属性名には予約語も使える (module.output など)
                    let name = match self.peek() {
                        Tok::Output => {
                            self.next();
                            "output".to_string()
                        }
                        _ => self.ident()?,
                    };
                    e = Expr::Attr(Box::new(e), name);
                }
                Tok::LParen => {
                    self.next();
                    e = Expr::Call(Box::new(e), self.args()?);
                }
                Tok::LBracket => {
                    self.next();
                    let index = self.expr(0)?;
                    self.expect(Tok::RBracket)?;
                    e = Expr::Index(Box::new(e), Box::new(index));
                }
                _ => return Ok(e),
            }
        }
    }

    /// `(` の直後から `)` まで。名前付き引数は `name: expr`
    fn args(&mut self) -> Result<Vec<Arg>> {
        let mut args = Vec::new();
        loop {
            self.skip_newlines();
            if *self.peek() == Tok::RParen {
                self.next();
                return Ok(args);
            }
            let name = match (self.peek(), self.peek_at(1)) {
                (Tok::Ident(n), Tok::Eq) => {
                    let n = n.clone();
                    self.next();
                    self.next();
                    Some(n)
                }
                // 引数の名前に限って、in も書ける (comp.place(..., in = 2s, out = 5s))
                (Tok::In, Tok::Eq) => {
                    self.next();
                    self.next();
                    Some("in".to_string())
                }
                _ => None,
            };
            // ...xs は並びを引数に広げる。名前付きには書けない
            let spread = *self.peek() == Tok::DotDotDot;
            if spread {
                self.next();
                if name.is_some() {
                    return err(Kind::UnexpectedToken, format!("line {}:{}: \"...\" cannot be given a name", self.line(), self.col()));
                }
            }
            args.push(Arg { name, value: self.expr(0)?, spread });
            self.skip_newlines();
            match self.peek() {
                Tok::Comma => {
                    self.next();
                }
                Tok::RParen => {}
                _ => return self.unexpected("\",\" or \")\""),
            }
        }
    }

    fn if_expr(&mut self) -> Result<Expr> {
        let cond = self.expr(0)?;
        self.expect(Tok::LBrace)?;
        let then = self.stmts_until(&Tok::RBrace)?;
        if *self.peek() != Tok::Else {
            return Ok(Expr::If(Box::new(cond), then, None));
        }
        self.next();
        let otherwise = if *self.peek() == Tok::If {
            self.next();
            let line = self.line();
            vec![Stmt { line, kind: StmtKind::Expr(self.if_expr()?) }]
        } else {
            self.expect(Tok::LBrace)?;
            self.stmts_until(&Tok::RBrace)?
        };
        Ok(Expr::If(Box::new(cond), then, Some(otherwise)))
    }

    fn pattern(&mut self) -> Result<Pattern> {
        let (close, make): (Tok, fn(Vec<Pattern>) -> Pattern) = match self.peek() {
            Tok::LParen => (Tok::RParen, Pattern::Tuple),
            Tok::LBracket => (Tok::RBracket, Pattern::List),
            _ => return Ok(Pattern::Name(self.ident()?)),
        };
        self.next();
        let mut items = vec![self.pattern()?];
        while *self.peek() == Tok::Comma {
            self.next();
            items.push(self.pattern()?);
        }
        self.expect(close)?;
        Ok(make(items))
    }

    /// import の元と既定の名前。math / .file / ..parent.file / "path/file.moph"
    /// 先頭の . が 1 つなら同じ場所、2 つなら 1 つ上 (使わないことを勧める)
    /// `{ a, b } from <source>`。import と export のどちらからも使う
    fn named_import(&mut self) -> Result<ImportKind> {
        self.expect(Tok::LBrace)?;
        let mut names = Vec::new();
        loop {
            self.skip_newlines();
            if *self.peek() == Tok::RBrace {
                self.next();
                break;
            }
            names.push(self.ident()?);
            self.skip_newlines();
            match self.peek() {
                Tok::Comma => {
                    self.next();
                }
                Tok::RBrace => {}
                _ => return self.unexpected("\",\" or \"}\""),
            }
        }
        match self.next() {
            Tok::Ident(kw) if kw == "from" => {}
            _ => {
                self.pos -= 1;
                return self.unexpected("\"from\"");
            }
        }
        let (source, _) = self.import_source()?;
        Ok(ImportKind::Names { source, names })
    }

    fn import_source(&mut self) -> Result<(ImportSource, String)> {
        match self.next() {
            Tok::Ident(name) => Ok((ImportSource::Std(name.clone()), name)),
            Tok::Dot | Tok::DotDot => {
                self.pos -= 1;
                let mut up = 0;
                loop {
                    match self.peek() {
                        Tok::Dot => {
                            self.next();
                            break;
                        }
                        Tok::DotDot => {
                            self.next();
                            up += 1;
                            // ".." の直後が識別子ならそこで終わり (.. = 1 つ上)。さらに . が続けばもう 1 つ上
                            if !matches!(self.peek(), Tok::Dot | Tok::DotDot) {
                                break;
                            }
                        }
                        _ => return self.unexpected("module path"),
                    }
                }
                let mut parts = vec![self.ident()?];
                while *self.peek() == Tok::Dot {
                    self.next();
                    parts.push(self.ident()?);
                }
                let prefix = if up == 0 { "./".to_string() } else { "../".repeat(up) };
                let path = format!("{prefix}{}.moph", parts.join("/"));
                let name = parts.last().expect("at least one").clone();
                Ok((ImportSource::File(path), name))
            }
            Tok::Str(path) => {
                let stem = std::path::Path::new(&path).file_stem().and_then(|s| s.to_str()).unwrap_or("module").to_string();
                Ok((ImportSource::File(path), stem))
            }
            // @.slides は root の slides.moph、@utils.format は alias utils の format.moph
            Tok::At => {
                let alias = match self.peek() {
                    Tok::Ident(_) => self.ident()?,
                    _ => String::new(),
                };
                let mut parts = Vec::new();
                while *self.peek() == Tok::Dot {
                    self.next();
                    parts.push(self.ident()?);
                }
                if parts.is_empty() {
                    return self.unexpected("\".\" and a module name after the alias (@.slides, @utils.format)");
                }
                let name = parts.last().expect("at least one").clone();
                Ok((ImportSource::File(format!("@{alias}/{}.moph", parts.join("/"))), name))
            }
            _ => {
                self.pos -= 1;
                self.unexpected("module name, .file, \"file.moph\", or @alias.file")
            }
        }
    }

    /// import の別名。`as name` があればそれ、無ければ既定の名前
    fn import_alias(&mut self, default: String) -> Result<String> {
        if *self.peek() != Tok::As {
            return Ok(default);
        }
        self.next();
        self.ident()
    }

    /// 関数の型 `(A, B) -> R`。宣言 `func (a: A, b: B) -> R` と同じ形
    fn func_type(&mut self) -> Result<TypeAnn> {
        self.expect(Tok::LParen)?;
        let mut params: Vec<String> = Vec::new();
        while *self.peek() != Tok::RParen {
            let mut text = self.type_ann()?.text;
            // 可変長は型の後ろに ...
            if *self.peek() == Tok::DotDot && *self.peek_at(1) == Tok::Dot {
                self.next();
                self.next();
                text.push_str("...");
            }
            params.push(text);
            if *self.peek() != Tok::Comma {
                break;
            }
            self.next();
        }
        self.expect(Tok::RParen)?;
        self.expect(Tok::Arrow)?;
        let returns = self.type_ann()?;
        Ok(TypeAnn { name: "Func".to_string(), args: Vec::new(), text: format!("({}) -> {}", params.join(", "), returns.text) })
    }

    /// 型注釈。`Dict<String, List<Number>>` のように入れ子で読む
    fn type_ann(&mut self) -> Result<TypeAnn> {
        if *self.peek() == Tok::LParen {
            return self.func_type();
        }
        let name = self.ident()?;
        if *self.peek() != Tok::Lt {
            return Ok(TypeAnn::plain(&name));
        }
        self.next();
        let mut args = vec![self.type_arg()?];
        while *self.peek() == Tok::Comma {
            self.next();
            args.push(self.type_arg()?);
        }
        if *self.peek() != Tok::Gt {
            return self.unexpected("\">\"");
        }
        self.next();
        let text = format!("{name}<{}>", args.iter().map(|a| a.text.clone()).collect::<Vec<_>>().join(", "));
        Ok(TypeAnn { name, args, text })
    }

    /// < > の中の 1 つ。型のほか、決まった Symbol と可変長の `Number...` が書ける
    fn type_arg(&mut self) -> Result<TypeAnn> {
        let mut ann = self.type_member()?;
        if *self.peek() == Tok::DotDot && *self.peek_at(1) == Tok::Dot {
            self.next();
            self.next();
            ann.text.push_str("...");
        }
        Ok(ann)
    }

    /// type の右辺の 1 つ。型名、決まった Symbol (":center")、関数の型
    fn type_member(&mut self) -> Result<TypeAnn> {
        if let Tok::Symbol(name) = self.peek().clone() {
            self.next();
            return Ok(TypeAnn::plain(&format!(":{name}")));
        }
        self.type_ann()
    }

    /// struct / record の中身。フィールドと func / method
    fn type_decl(&mut self, immutable: bool, anns: Vec<(String, Vec<Expr>)>) -> Result<StmtKind> {
        let name = self.ident()?;
        let mut decl = TypeDecl { name, immutable, nocopy: false, nodeepcopy: false, deprecated: None, fields: Vec::new(), members: Vec::new() };
        for (ann, args) in &anns {
            match ann.as_str() {
                "immutable" if !immutable => decl.immutable = true,
                "immutable" => return err(Kind::UnexpectedToken, "record is already immutable"),
                "nocopy" => decl.nocopy = true,
                "nodeepcopy" => decl.nodeepcopy = true,
                "deprecated" => {
                    decl.deprecated = Some(match args.first() {
                        Some(Expr::Str(text)) => text.clone(),
                        Some(_) => return err(Kind::ArgumentType, "@deprecated takes a String"),
                        None => String::new(),
                    });
                }
                other => return err(Kind::UndefinedVariable, format!("unknown property @{other}")),
            }
        }
        self.expect(Tok::LBrace)?;
        loop {
            self.skip_newlines();
            if *self.peek() == Tok::RBrace {
                self.next();
                break;
            }
            let private = *self.peek() == Tok::PrivateKw;
            if private {
                self.next();
            }
            match self.peek() {
                Tok::Func | Tok::MethodKw => {
                    let receiver = *self.peek() == Tok::MethodKw;
                    self.next();
                    let member = self.ident()?;
                    let Expr::Func(def) = self.func_expr()? else {
                        return self.unexpected("func body");
                    };
                    let has_self = matches!(def.params.first().map(|p| &p.pattern), Some(Pattern::Name(n)) if n == "self" || n == "_");
                    if receiver && !has_self {
                        return err(Kind::UnexpectedToken, format!("method {member} needs self (or _) as its first parameter"));
                    }
                    if !receiver && has_self {
                        return err(Kind::UnexpectedToken, format!("func {member} must not take self; use method"));
                    }
                    decl.members.push(MemberDecl { name: member, private, receiver, def });
                }
                _ => {
                    let field = self.ident()?;
                    self.expect(Tok::Colon)?;
                    let ann = self.type_ann()?;
                    let default = if *self.peek() == Tok::Eq {
                        self.next();
                        Some(self.expr(0)?)
                    } else {
                        None
                    };
                    decl.fields.push(FieldDecl { name: field, ann, default, private });
                }
            }
            self.skip_newlines();
        }
        Ok(StmtKind::TypeDecl(Rc::new(decl)))
    }

    /// `func` の直後から。(params) { body }
    fn func_expr(&mut self) -> Result<Expr> {
        self.expect(Tok::LParen)?;
        let mut params = Vec::new();
        loop {
            self.skip_newlines();
            if *self.peek() == Tok::RParen {
                self.next();
                break;
            }
            let pattern = self.pattern()?;
            let ann = if *self.peek() == Tok::Colon {
                self.next();
                Some(self.type_ann()?)
            } else {
                None
            };
            let default = if *self.peek() == Tok::Eq {
                self.next();
                Some(self.expr(0)?)
            } else {
                None
            };
            params.push(Param { pattern, ann, default });
            self.skip_newlines();
            match self.peek() {
                Tok::Comma => {
                    self.next();
                }
                Tok::RParen => {}
                _ => return self.unexpected("\",\" or \")\""),
            }
        }
        let returns = if *self.peek() == Tok::Arrow {
            self.next();
            Some(self.type_ann()?)
        } else {
            None
        };
        self.expect(Tok::LBrace)?;
        // 関数の本体は別の場所。外側の for の break はここには効かない
        let outer = std::mem::take(&mut self.loops);
        let body = self.stmts_until(&Tok::RBrace);
        self.loops = outer;
        Ok(Expr::Func(Rc::new(FuncDef { params, returns, body: body? })))
    }

    /// `{` の直後から。{ "k": v, x, ... }
    fn dict_expr(&mut self) -> Result<Expr> {
        let mut entries = Vec::new();
        loop {
            self.skip_newlines();
            if *self.peek() == Tok::RBrace {
                self.next();
                return Ok(Expr::Dict(entries));
            }
            let entry = match self.next() {
                Tok::Str(k) => {
                    self.expect(Tok::Colon)?;
                    (DictKey::Str(k), self.expr(0)?)
                }
                Tok::Ident(name) => (DictKey::Shorthand(name.clone()), Expr::Ident(name)),
                _ => {
                    self.pos -= 1;
                    return self.unexpected("dict key");
                }
            };
            entries.push(entry);
            self.skip_newlines();
            match self.peek() {
                Tok::Comma => {
                    self.next();
                }
                Tok::RBrace => {}
                _ => return self.unexpected("\",\" or \"}\""),
            }
        }
    }

    /// `progress [:x, :y] { 0: 2, 4.5 ... }`。`progress` は読み終えている
    fn progress_expr(&mut self) -> Result<Expr> {
        self.expect(Tok::LBracket)?;
        let mut columns = Vec::new();
        loop {
            let Tok::Symbol(name) = self.next() else {
                self.pos -= 1;
                return self.unexpected("column name like :x");
            };
            columns.push(name);
            match self.next() {
                Tok::Comma => {}
                Tok::RBracket => break,
                _ => {
                    self.pos -= 1;
                    return self.unexpected("\",\" or \"]\"");
                }
            }
        }
        self.expect(Tok::LBrace)?;
        let mut rows = Vec::new();
        loop {
            self.skip_newlines();
            if *self.peek() == Tok::RBrace {
                self.next();
                return Ok(Expr::Progress(Box::new(ProgressDef { columns, rows })));
            }
            let row = self.progress_row()?;
            if row.cells.len() != columns.len() {
                return err(Kind::ArityMismatch, format!("line {}:{}: a progress row has {} values but {} columns are listed", self.line(), self.col(), row.cells.len(), columns.len()));
            }
            rows.push(row);
        }
    }

    /// `0.4: _, 6 :ease`。値の隣の `:ease` は、その列のこの行へ向かう曲線
    fn progress_row(&mut self) -> Result<ProgressRow> {
        let at = self.expr(7)?;
        self.expect(Tok::Colon)?;
        let mut cells = Vec::new();
        loop {
            if matches!(self.peek(), Tok::Ident(n) if n == "_") {
                self.next();
                cells.push(None);
            } else {
                let value = self.expr(0)?;
                let mut curve = None;
                while let Tok::Symbol(name) = self.peek().clone() {
                    self.next();
                    if !crate::lang::curve::CURVES.contains(&name.as_str()) {
                        return err(Kind::OutOfRange, format!("line {}:{}: unknown modifier :{name}", self.line(), self.col()));
                    }
                    curve = Some(name);
                }
                cells.push(Some((value, curve)));
            }
            if *self.peek() != Tok::Comma {
                break;
            }
            self.next();
        }
        match self.peek() {
            Tok::Newline | Tok::RBrace => Ok(ProgressRow { at, cells }),
            _ => self.unexpected("end of row"),
        }
    }
}

fn compare_op(op: BinOp) -> bool {
    matches!(op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne)
}

fn compare_of(tok: &Tok) -> Option<BinOp> {
    Some(match tok {
        Tok::Lt => BinOp::Lt,
        Tok::Le => BinOp::Le,
        Tok::Gt => BinOp::Gt,
        Tok::Ge => BinOp::Ge,
        Tok::EqEq => BinOp::Eq,
        Tok::Ne => BinOp::Ne,
        _ => return None,
    })
}
