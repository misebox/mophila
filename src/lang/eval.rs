use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use std::cell::Cell;

use crate::lang::ast::{Arg, BinOp, DictKey, Expr, ImportKind, ImportSource, MotionDef, Pattern, RowItem, Stmt, StmtKind};
use crate::lang::error::{MophError, Result, err};
use crate::lang::value::{Audio, Clip, Closure, Module, Motion, MotionRowVal, ObjRef, Object, Placed, Record, Scopes, Timeline, TlAssign, TlKeyframe, Track, UserType, Value, new_scope};
use crate::stdlib;

/// 文の実行結果。return で関数を抜けるときに伝える
enum Flow {
    Next(Value),
    Return(Value),
}

impl Flow {
    fn value(self) -> Value {
        match self {
            Flow::Next(v) | Flow::Return(v) => v,
        }
    }
}

pub struct Interp {
    scopes: Scopes,
    pub output: Option<ObjRef>,
    /// type Name = A | B の定義
    types: HashMap<String, Vec<String>>,
    /// フォント検索、テキストレイアウト、描画命令のキャッシュ。初回に必要になったときに作る
    cache: Option<crate::render::text::RenderCache>,
    /// 終わった Timeline の最後の値。キーは (Timeline のポインタ, 絶対開始時刻のビット)
    finished: HashMap<(usize, u64), Vec<(ObjRef, Vec<String>, Value)>>,
    /// 前回描いた時刻。戻ったら finished を捨てる
    last_t: f64,
    /// 実行中のファイルのディレクトリ。import "file" の相対パスの基準
    pub base_dir: PathBuf,
    /// 実行中のファイルが export した名前
    exports: Vec<String>,
    /// 埋め込みバイナリのとき、import はディスクではなくここから読む (正規化したパス → 本文)
    pub sources: HashMap<String, String>,
    /// 埋め込みバイナリのとき、音声などのファイルの取り出し先 (正規化したパス → 実際のファイル)
    pub assets: HashMap<String, PathBuf>,
    /// スクリプト実行直後の全オブジェクトの属性。毎フレームここに戻してから Timeline を適用する
    initial: Vec<(ObjRef, HashMap<String, Value>)>,
    /// 読み込み済みのモジュールと音声 (正規化したパス → 実体)。同じファイルは 1 度しか読まない
    modules: HashMap<String, Value>,
    /// 読み込み中のパス。循環 import の検出用
    loading: Vec<String>,
    /// 式の中 (if のブロックなど) で return した値。外側の文の列がこれを見て関数を抜ける
    returning: Option<Value>,
    /// いま func new を実行中の型。中で型名を呼んだらフィールドから作る
    constructing: Vec<String>,
    /// @deprecated の警告を出した型。1 つにつき 1 度だけ出す
    warned: std::collections::HashSet<String>,
    /// いま本体を実行している型。private が見えるのはこの中だけ
    inside: Vec<String>,
    /// 次の apply がどの型のメンバーか。apply がこれを inside に積む
    calling: Option<String>,
}

impl Interp {
    pub fn new() -> Self {
        // union がまとめる型と、決まった Symbol しか取らない型は docs::TYPES が持っている
        let mut types: HashMap<String, Vec<String>> = HashMap::new();
        for t in crate::docs::TYPES {
            if !t.members.is_empty() {
                types.insert(t.name.to_string(), t.members.iter().map(|m| m.to_string()).collect());
            } else if !t.values.is_empty() {
                types.insert(t.name.to_string(), t.values.iter().map(|(v, _)| v.to_string()).collect());
            }
        }
        Self { scopes: vec![new_scope()], output: None, types, cache: None, finished: HashMap::new(), last_t: f64::NEG_INFINITY, base_dir: PathBuf::from("."), exports: Vec::new(), sources: HashMap::new(), assets: HashMap::new(), initial: Vec::new(), modules: HashMap::new(), loading: Vec::new(), returning: None, constructing: Vec::new(), warned: std::collections::HashSet::new(), inside: Vec::new(), calling: None }
    }

    /// トップレベルの束縛 (LSP のホバー用)
    pub fn globals(&self) -> Vec<(String, Value)> {
        self.scopes.first().map(|s| s.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()
    }

    pub fn cache_mut(&mut self) -> &mut crate::render::text::RenderCache {
        self.cache.get_or_insert_with(crate::render::text::RenderCache::new)
    }

    /// トップレベルの実行。return があればエラー
    pub fn run(&mut self, stmts: &[Stmt]) -> Result<Value> {
        match self.run_block(stmts)? {
            Flow::Next(v) => Ok(v),
            Flow::Return(_) => err("SyntaxError.UnexpectedToken", "return outside of a function"),
        }
    }

    fn run_block(&mut self, stmts: &[Stmt]) -> Result<Flow> {
        let mut last = Value::Nothing;
        for stmt in stmts {
            let flow = self.exec(stmt)?;
            if let Some(v) = self.returning.take() {
                return Ok(Flow::Return(v));
            }
            match flow {
                Flow::Next(v) => last = v,
                ret => return Ok(ret),
            }
        }
        Ok(Flow::Next(last))
    }

    /// 文を実行する。エラーに行番号が無ければ付ける
    fn exec(&mut self, stmt: &Stmt) -> Result<Flow> {
        self.exec_kind(&stmt.kind).map_err(|e| {
            if e.message.starts_with("line ") {
                e
            } else {
                MophError::new(e.kind, format!("line {}: {}", stmt.line, e.message))
            }
        })
    }

    fn exec_kind(&mut self, stmt: &StmtKind) -> Result<Flow> {
        match stmt {
            StmtKind::Let(pat, ann, e) => {
                let mut v = self.eval(e)?;
                if let Some(ann) = ann {
                    self.check_type(&v, &ann.name)?;
                    if let Value::Func(c) = &v {
                        v = Value::Func(Rc::new(Closure { def: c.def.clone(), scopes: c.scopes.clone(), type_text: Some(ann.text.clone()) }));
                    }
                }
                self.bind(pat, v)?;
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::TypeDef(name, members) => {
                if self.scopes.len() != 1 {
                    return err("SyntaxError.UnexpectedToken", "type is only allowed at the top level");
                }
                check_free_name(name)?;
                self.types.insert(name.clone(), members.clone());
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::Import(ImportKind::Module { source, alias }) => {
                let module = self.load_module(source)?;
                self.scopes.first().expect("global scope").borrow_mut().insert(alias.clone(), module);
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::Import(ImportKind::Names { source, names }) => {
                let Value::Module(module) = self.load_module(source)? else {
                    return err("TypeError.ArgumentType", "import { ... } from needs a module, not a file asset");
                };
                for name in names {
                    let Some(v) = module.items.get(name) else {
                        return err("NameError.UndefinedAttribute", format!("module {} does not export \"{name}\"", module.name));
                    };
                    self.scopes.first().expect("global scope").borrow_mut().insert(name.clone(), v.clone());
                }
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::Export(inner) => {
                if self.scopes.len() != 1 {
                    return err("SyntaxError.UnexpectedToken", "export is only allowed at the top level");
                }
                let result = self.exec(inner)?;
                match &inner.kind {
                    StmtKind::Let(pat, ..) => collect_names(pat, &mut self.exports),
                    StmtKind::TypeDecl(decl) => self.exports.push(decl.name.clone()),
                    _ => {}
                }
                Ok(result)
            }
            StmtKind::TypeDecl(decl) => {
                if self.scopes.len() != 1 {
                    return err("SyntaxError.UnexpectedToken", "struct and record are only allowed at the top level");
                }
                check_free_name(&decl.name)?;
                let ty = Rc::new(UserType { decl: decl.clone(), scopes: self.scopes.clone() });
                self.scopes.first().expect("global scope").borrow_mut().insert(decl.name.clone(), Value::Type(ty));
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::AssignMulti(targets, values) => {
                let values = values.iter().map(|e| self.eval(e)).collect::<Result<Vec<_>>>()?;
                for (target, v) in targets.iter().zip(values) {
                    self.assign(target, v)?;
                }
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::AssignIndex(target, index, e) => {
                let target = self.eval(target)?;
                let index = self.eval(index)?;
                let v = self.eval(e)?;
                match (&target, &index) {
                    (Value::Dict(entries), Value::Str(key)) => {
                        let mut entries = entries.borrow_mut();
                        match entries.iter_mut().find(|(k, _)| k == key) {
                            Some(slot) => slot.1 = v,
                            None => entries.push((key.clone(), v)),
                        }
                    }
                    (Value::List(items), Value::Number(i)) => {
                        let mut items = items.borrow_mut();
                        let n = items.len() as i64;
                        let at = if (*i as i64) < 0 { *i as i64 + n } else { *i as i64 };
                        if at < 0 || at >= n {
                            return err("ValueError.OutOfRange", format!("index {i} out of range for length {n}"));
                        }
                        items[at as usize] = v;
                    }
                    _ => return err("TypeError.OperandType", format!("cannot assign into {} with {} index", target.type_name(), index.type_name())),
                }
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::AssignVar(name, e) => {
                let v = self.eval(e)?;
                self.assign_var(name, v)?;
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::AssignAttr(target, attr, e) => {
                let v = self.eval(e)?;
                self.assign(&Expr::Attr(Box::new(target.clone()), attr.clone()), v)?;
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::Output(e) if self.output.is_some() => {
                let _ = e;
                err("SyntaxError.UnexpectedToken", "a file can have only one output")
            }
            StmtKind::Output(e) => {
                self.output = Some(self.eval_object(e)?);
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::For(pat, iter, body) => {
                for item in self.iterate(iter)? {
                    self.scopes.push(new_scope());
                    let result = self.bind(pat, item).and_then(|()| self.run_block(body));
                    self.scopes.pop();
                    if let Flow::Return(v) = result? {
                        return Ok(Flow::Return(v));
                    }
                }
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::Return(e) => Ok(Flow::Return(self.eval(e)?)),
            StmtKind::Expr(e) => Ok(Flow::Next(self.eval(e)?)),
        }
    }

    /// 別名なら、指している型の名前に直す
    fn real_type_name(&self, name: &str) -> String {
        match self.scopes.iter().rev().find_map(|s| s.borrow().get(name).cloned()) {
            Some(Value::Type(t)) => t.decl.name.clone(),
            Some(Value::BuiltinType(n)) => n,
            _ => name.to_string(),
        }
    }

    /// 値が型名に合うか。Union は定義をたどる
    fn check_type(&self, v: &Value, name: &str) -> Result<()> {
        let name = &self.real_type_name(name);
        if self.matches_type(v, name) {
            return Ok(());
        }
        Err(self.wrong_type("TypeError.AttributeType", "value", name, v))
    }

    /// 型が合わないときのエラー。決まった値しか取らない型なら、取れる値を並べる
    fn wrong_type(&self, kind: &'static str, what: &str, expected: &str, v: &Value) -> MophError {
        match self.types.get(expected) {
            Some(values) if values.iter().all(|m| m.starts_with(':')) => {
                MophError::new("ValueError.OutOfRange", format!("{what} is one of {}, found {v}", values.join(" | ")))
            }
            _ => MophError::new(kind, format!("{what} expects {expected}, found {}", v.type_name())),
        }
    }

    /// 属性に値を書く。型は宣言 (struct / record) か schema から引く
    fn set_attr(&self, obj: &ObjRef, attr: &str, value: Value) -> Result<()> {
        let mut o = obj.borrow_mut();
        let expected = match &o.decl {
            Some(ty) => ty.decl.fields.iter().find(|f| f.name == attr).map(|f| f.ann.name.clone()),
            None => schema(&o.kind).and_then(|s| s.iter().find(|(n, _)| *n == attr)).map(|(_, t)| (*t).to_string()),
        };
        let Some(expected) = expected else {
            return err("NameError.UndefinedAttribute", format!("{} has no attribute \"{attr}\"", o.kind));
        };
        if !self.matches_type(&value, &self.real_type_name(&expected)) {
            return Err(self.wrong_type("TypeError.AttributeType", &format!("{}.{attr}", o.kind), &expected, &value));
        }
        o.attrs.insert(attr.to_string(), value);
        Ok(())
    }

    /// 属性パスに値を書く。[position, x] なら position (Pos) の x だけを変える
    fn set_path(&self, obj: &ObjRef, path: &[String], value: Value) -> Result<()> {
        let [attr, rest @ ..] = path else {
            return err("NameError.UndefinedAttribute", "empty attribute path");
        };
        if rest.is_empty() {
            return self.set_attr(obj, attr, value);
        }
        let current = obj.borrow().attrs.get(attr).cloned();
        let Some(current) = current else {
            return err("NameError.UndefinedAttribute", format!("{}.{attr} is not set", obj.borrow().kind));
        };
        self.set_attr(obj, attr, set_field(current, rest, value)?)
    }

    fn matches_type(&self, v: &Value, name: &str) -> bool {
        // ":center" のような要素は、その Symbol そのもの
        if let Some(sym) = name.strip_prefix(':') {
            return matches!(v, Value::Symbol(s) if s == sym);
        }
        let actual = match v {
            Value::Func(_) => "Func".to_string(),
            v => v.type_name(),
        };
        if actual == name {
            return true;
        }
        self.types.get(name).is_some_and(|members| members.iter().any(|m| self.matches_type(v, m)))
    }

    /// 名前だけの import は標準ライブラリ (本体に入っているもの)、. や "" で始まるものはファイル
    fn load_module(&mut self, source: &ImportSource) -> Result<Value> {
        match source {
            ImportSource::Std(name) => match stdlib::find(name) {
                Some(stdlib::Lib::Native(module)) => Ok(Value::Module(Rc::new(module))),
                Some(stdlib::Lib::Script(src)) => self.run_module(format!("std:{name}"), name, src, self.base_dir.clone()),
                None => err("NameError.UndefinedVariable", format!("no module named \"{name}\"")),
            },
            ImportSource::File(path) => self.import_file(path),
        }
    }

    /// .moph ならファイルを別のスコープで実行し、export した束縛と output した View をモジュールにする。
    /// それ以外は音声ファイルとして読む
    fn import_file(&mut self, path: &str) -> Result<Value> {
        let full = self.base_dir.join(path);
        let key = crate::bundle::normalize(&full);
        if let Some(m) = self.modules.get(&key) {
            return Ok(m.clone());
        }
        if !path.ends_with(".moph") {
            let real = self.assets.get(&key).cloned().unwrap_or(full);
            let audio = Value::Audio(Rc::new(load_audio(path, real)?));
            self.modules.insert(key, audio.clone());
            return Ok(audio);
        }
        let src = match self.sources.get(&key) {
            Some(src) => src.clone(),
            None => std::fs::read_to_string(&full)
                .map_err(|e| MophError::new("NameError.UndefinedVariable", format!("cannot read \"{}\": {e}", full.display())))?,
        };
        let dir = full.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        self.run_module(key, path, &src, dir)
    }

    /// モジュールのソースを別のスコープで実行する。同じ key は 1 度だけ実行し、以後は同じ実体を返す。
    /// dir はその中の相対 import の基準
    fn run_module(&mut self, key: String, label: &str, src: &str, dir: PathBuf) -> Result<Value> {
        if let Some(m) = self.modules.get(&key) {
            return Ok(m.clone());
        }
        if self.loading.contains(&key) {
            return err("NameError.UndefinedVariable", format!("circular import of \"{label}\""));
        }
        self.loading.push(key.clone());
        let stmts = crate::lang::parser::parse(src)?;
        let saved_scopes = std::mem::replace(&mut self.scopes, vec![new_scope()]);
        let saved_output = self.output.take();
        let saved_dir = std::mem::replace(&mut self.base_dir, dir);
        let saved_exports = std::mem::take(&mut self.exports);
        let result = self.run(&stmts);
        let scope = std::mem::replace(&mut self.scopes, saved_scopes);
        let output = std::mem::replace(&mut self.output, saved_output);
        let exports = std::mem::replace(&mut self.exports, saved_exports);
        self.base_dir = saved_dir;
        self.loading.pop();
        result.map_err(|e| MophError::new(e.kind, format!("in {label}: {}", e.message)))?;
        // 公開するのは export した名前と output だけ
        let scope = scope[0].borrow();
        let mut items: HashMap<String, Value> = exports.iter().filter_map(|n| scope.get(n).map(|v| (n.clone(), v.clone()))).collect();
        if let Some(view) = output {
            items.insert("output".into(), Value::Object(view));
        }
        let module = Value::Module(Rc::new(Module { name: label.to_string(), items }));
        self.modules.insert(key, module.clone());
        Ok(module)
    }

    fn assign_var(&mut self, name: &str, v: Value) -> Result<()> {
        let Some(scope) = self.scopes.iter().rev().find(|s| s.borrow().contains_key(name)) else {
            return err("NameError.AssignWithoutLet", format!("\"{name}\" is not defined; use \"let {name} = ...\""));
        };
        scope.borrow_mut().insert(name.to_string(), v);
        Ok(())
    }

    /// 代入先の式 (変数・属性) に値を入れる
    fn assign(&mut self, target: &Expr, v: Value) -> Result<()> {
        match target {
            Expr::Ident(name) => {
                self.assign_var(name, v)
            }
            Expr::Attr(obj, attr) => match self.eval(obj)? {
                Value::Object(obj) => {
                    self.field_ok(&obj.borrow().decl, attr)?;
                    let expected = schema(&obj.borrow().kind).and_then(|s| s.iter().find(|(n, _)| *n == attr)).map(|(_, t)| *t);
                    let v = match expected {
                        Some(_) => v,
                        None => v,
                    };
                    self.set_attr(&obj, attr, v)
                }
                Value::Motion(m) if attr == "duration" => set_duration(&m.duration, v),
                Value::Timeline(t) if attr == "duration" => set_duration(&t.duration, v),
                // 値のフィールドへの代入。値は変わらないので、その値を持っている入れ物を更新する
                other => {
                    if let Value::Record(r) = &other {
                        self.field_ok(&r.decl, attr)?;
                    }
                    let updated = set_field(other, std::slice::from_ref(attr), v)?;
                    self.assign(obj, updated)
                }
            },
            _ => err("SyntaxError.UnexpectedToken", "cannot assign to this expression"),
        }
    }

    fn iterate(&mut self, e: &Expr) -> Result<Vec<Value>> {
        match self.eval(e)? {
            Value::List(items) => Ok(items.borrow().clone()),
            Value::Tuple(items) => Ok(items),
            Value::Range(a, b) => Ok((a..b).map(|i| Value::Number(i as f64)).collect()),
            Value::Dict(entries) => Ok(entries.borrow().iter().map(|(k, v)| Value::Tuple(vec![Value::Str(k.clone()), v.clone()])).collect()),
            v => err("TypeError.ArgumentType", format!("cannot iterate over {}", v.type_name())),
        }
    }

    fn bind(&mut self, pat: &Pattern, value: Value) -> Result<()> {
        match pat {
            Pattern::Name(name) => {
                self.scopes.last().expect("scope").borrow_mut().insert(name.clone(), value);
                Ok(())
            }
            Pattern::List(pats) => {
                let Value::List(items) = value else {
                    return err("TypeError.ArgumentType", format!("cannot destructure {} into a list", value.type_name()));
                };
                let items = items.borrow().clone();
                if items.len() != pats.len() {
                    return err("TypeError.ArityMismatch", format!("expected a list of {}, found {}", pats.len(), items.len()));
                }
                for (p, v) in pats.iter().zip(items) {
                    self.bind(p, v)?;
                }
                Ok(())
            }
            Pattern::Tuple(pats) => {
                let Value::Tuple(items) = value else {
                    return err("TypeError.ArgumentType", format!("cannot destructure {} into a tuple", value.type_name()));
                };
                if items.len() != pats.len() {
                    return err("TypeError.ArityMismatch", format!("expected a tuple of {}, found {}", pats.len(), items.len()));
                }
                for (p, v) in pats.iter().zip(items) {
                    self.bind(p, v)?;
                }
                Ok(())
            }
        }
    }

    fn lookup(&self, name: &str) -> Result<Value> {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.borrow().get(name).cloned())
            // 組み込みの型は名前だけで値として引ける (alias 用)
            .or_else(|| Self::is_builtin_type(name).then(|| Value::BuiltinType(name.to_string())))
            .ok_or_else(|| {
                let hint = if stdlib::find(name).is_some() { format!("; add \"import {name}\"") } else { String::new() };
                MophError::new("NameError.UndefinedVariable", format!("\"{name}\" is not defined{hint}"))
            })
    }

    fn eval_object(&mut self, e: &Expr) -> Result<ObjRef> {
        match self.eval(e)? {
            Value::Object(o) => Ok(o),
            v => err("TypeError.AttributeType", format!("expected an object, found {}", v.type_name())),
        }
    }

    pub fn eval(&mut self, e: &Expr) -> Result<Value> {
        match e {
            Expr::Number(v) => Ok(Value::Number(*v)),
            Expr::Duration(v) => Ok(Value::Duration(*v)),
            Expr::Color(c) => Ok(Value::Color(*c)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Symbol(s) => Ok(Value::Symbol(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Ident(name) => self.lookup(name),
            Expr::Neg(inner) => match self.eval(inner)? {
                Value::Number(v) => Ok(Value::Number(-v)),
                Value::Duration(v) => Ok(Value::Duration(-v)),
                Value::Vector(x, y) => Ok(Value::Vector(-x, -y)),
                v => err("TypeError.OperandType", format!("cannot negate {}", v.type_name())),
            },
            Expr::Not(inner) => match self.eval(inner)? {
                Value::Bool(b) => Ok(Value::Bool(!b)),
                v => err("TypeError.OperandType", format!("cannot apply not to {}", v.type_name())),
            },
            Expr::Binary(BinOp::And, l, r) => match self.eval(l)? {
                Value::Bool(false) => Ok(Value::Bool(false)),
                Value::Bool(true) => self.eval_bool(r).map(Value::Bool),
                v => err("TypeError.OperandType", format!("cannot apply and to {}", v.type_name())),
            },
            Expr::Binary(BinOp::Or, l, r) => match self.eval(l)? {
                Value::Bool(true) => Ok(Value::Bool(true)),
                Value::Bool(false) => self.eval_bool(r).map(Value::Bool),
                v => err("TypeError.OperandType", format!("cannot apply or to {}", v.type_name())),
            },
            Expr::Binary(op, l, r) => {
                let l = self.eval(l)?;
                let r = self.eval(r)?;
                binary(*op, l, r)
            }
            Expr::Tuple(items) => Ok(Value::Tuple(items.iter().map(|e| self.eval(e)).collect::<Result<_>>()?)),
            Expr::List(items) => {
                let values = items.iter().map(|e| self.eval(e)).collect::<Result<Vec<_>>>()?;
                Ok(Value::List(Rc::new(RefCell::new(values))))
            }
            Expr::Index(target, index) => {
                let target = self.eval(target)?;
                let index = self.eval(index)?;
                index_value(&target, &index)
            }
            Expr::If(cond, then, otherwise) => {
                let branch = if self.eval_bool(cond)? { Some(then) } else { otherwise.as_ref() };
                match branch {
                    Some(body) => self.block(body),
                    None => Ok(Value::Nothing),
                }
            }
            Expr::Attr(target, attr) if matches!(attr.as_str(), "anchor" | "vector") => match self.eval(target)? {
                Value::Apos(a, x, y) => Ok(if attr == "anchor" { Value::Symbol(a) } else { Value::Vector(x, y) }),
                Value::Object(obj) => self.attr_of(&obj, attr),
                Value::Record(r) => self.record_attr(&r, attr),
                v => err("NameError.UndefinedAttribute", format!("{} has no attribute \"{attr}\"", v.type_name())),
            },
            Expr::Attr(target, attr) if matches!(attr.as_str(), "x" | "y") => {
                match self.eval(target)? {
                    Value::Vector(x, y) | Value::Apos(_, x, y) => Ok(Value::Number(if attr == "x" { x } else { y })),
                    Value::Tuple(items) if items.len() == 2 => Ok(items[if attr == "x" { 0 } else { 1 }].clone()),
                    Value::Object(obj) => self.attr_of(&obj, attr),
                    Value::Record(r) => self.record_attr(&r, attr),
                    v => err("NameError.UndefinedAttribute", format!("{} has no attribute \"{attr}\"", v.type_name())),
                }
            }
            Expr::Attr(target, attr) => match self.eval(target)? {
                Value::Object(obj) => self.attr_of(&obj, attr),
                Value::Audio(a) if attr == "duration" => Ok(Value::Duration(a.length)),
                Value::Audio(a) if attr == "file" => Ok(Value::Str(a.name.clone())),
                // 色の成分。r g b は 0..255、a は 0..1 (new Color { } と同じ単位)
                Value::Color([r, g, b, a]) if matches!(attr.as_str(), "r" | "g" | "b" | "a") => Ok(Value::Number(match attr.as_str() {
                    "r" => (r as f64 * 255.0 * 1000.0).round() / 1000.0,
                    "g" => (g as f64 * 255.0 * 1000.0).round() / 1000.0,
                    "b" => (b as f64 * 255.0 * 1000.0).round() / 1000.0,
                    _ => (a as f64 * 1000.0).round() / 1000.0,
                })),
                Value::Module(m) => m
                    .items
                    .get(attr)
                    .cloned()
                    .ok_or_else(|| MophError::new("NameError.UndefinedAttribute", format!("module {} has no item \"{attr}\"", m.name))),
                Value::Record(r) => self.record_attr(&r, attr),
                v => err("NameError.UndefinedAttribute", format!("{} has no attribute \"{attr}\"", v.type_name())),
            },
            Expr::Call(callee, args) => self.call(callee, args),
            Expr::Context(bindings, body) => {
                let scope = new_scope();
                for (target, alias) in bindings {
                    let v = self.eval(target)?;
                    scope.borrow_mut().insert(alias.clone(), v);
                }
                self.scopes.push(scope);
                let result = self.run_block(body);
                self.scopes.pop();
                // return は関数を抜ける。context で止めない
                match result? {
                    Flow::Return(v) => {
                        self.returning = Some(v.clone());
                        Ok(v)
                    }
                    flow => Ok(flow.value()),
                }
            }
            Expr::Dict(entries) => {
                let mut out = Vec::new();
                for (key, e) in entries {
                    let key = match key {
                        DictKey::Str(k) | DictKey::Shorthand(k) => k.clone(),
                    };
                    out.push((key, self.eval(e)?));
                }
                Ok(Value::Dict(Rc::new(RefCell::new(out))))
            }
            Expr::Func(def) => Ok(Value::Func(Rc::new(Closure { def: def.clone(), scopes: self.scopes.clone(), type_text: None }))),
            Expr::Motion(def) => self.eval_motion(def),
        }
    }

    /// 引数を、宣言の順に並んだフィールド名へ割り当てる。位置と名前を混ぜられる
    /// 名前を付けずに並べた引数だけを取る
    fn positional(&mut self, kind: &str, args: &[Arg]) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            match &a.name {
                Some(n) => return err("TypeError.ArgumentType", format!("{kind} takes no named argument \"{n}\"")),
                None => out.push(self.eval(&a.value)?),
            }
        }
        Ok(out)
    }

    fn resolve_args(&mut self, kind: &str, fields: &[&str], args: &[Arg]) -> Result<HashMap<String, Value>> {
        let mut map: HashMap<String, Value> = HashMap::new();
        let mut next = 0;
        for arg in args {
            let v = self.eval(&arg.value)?;
            let name = match &arg.name {
                Some(n) => {
                    if !fields.contains(&n.as_str()) {
                        return err("NameError.UndefinedAttribute", format!("{kind} has no field \"{n}\""));
                    }
                    n.clone()
                }
                None => {
                    let Some(f) = fields.get(next) else {
                        return err("TypeError.ArityMismatch", format!("{kind} takes {} fields, more were given", fields.len()));
                    };
                    next += 1;
                    (*f).to_string()
                }
            };
            if map.insert(name.clone(), v).is_some() {
                return err("TypeError.ArgumentType", format!("{kind}.{name} is given twice"));
            }
        }
        Ok(map)
    }

    /// 呼んでは作れない名前。組み込みの型なら書き方を示す
    fn cannot_construct(&self, kind: &str) -> Result<Value> {
        let Some(t) = crate::docs::TYPES.iter().find(|t| t.name == kind) else {
            return match self.types.contains_key(kind) {
                true => err("TypeError.ArgumentType", format!("{kind} is a union of types, so it cannot be built")),
                false => err("NameError.UndefinedVariable", format!("type \"{kind}\" is not defined")),
            };
        };
        if !t.members.is_empty() {
            return err("TypeError.ArgumentType", format!("{kind} is a union of {}, so it cannot be built", t.members.join(" | ")));
        }
        match (t.make.lines().next().filter(|f| !f.is_empty()), t.values.first()) {
            (Some(form), _) => err("TypeError.ArgumentType", format!("{kind} cannot be called; write it like {form}")),
            (_, Some((v, _))) => err("TypeError.ArgumentType", format!("{kind} only takes fixed values; write one like {v}")),
            _ => err("TypeError.ArgumentType", format!("{kind} cannot be called")),
        }
    }

    /// Pos の anchor。書かなければ :center
    fn anchor_of(&self, v: Option<&Value>) -> Result<String> {
        let Some(v) = v else { return Ok("center".to_string()) };
        self.check_type(v, "Anchor")?;
        match v {
            Value::Symbol(a) => Ok(a.clone()),
            v => Err(self.wrong_type("TypeError.ArgumentType", "Pos.anchor", "Anchor", v)),
        }
    }

    /// 型名を呼んで値を作る
    fn construct(&mut self, kind: &str, args: &[Arg]) -> Result<Value> {
        match kind {
            "Vector" => {
                let map = self.resolve_args(kind, &["x", "y"], args)?;
                match (map.get("x"), map.get("y")) {
                    (Some(Value::Number(x)), Some(Value::Number(y))) => Ok(Value::Vector(*x, *y)),
                    _ => err("TypeError.ArgumentType", "Vector needs x and y (Number)"),
                }
            }
            "Pos" => {
                // Pos(x, y, anchor = :center) と Pos(v: Vector, anchor = :center)
                if let Some(Arg { name: None, value }) = args.first() {
                    if let Value::Vector(x, y) = self.eval(value)? {
                        let rest = self.resolve_args(kind, &["anchor"], &args[1..])?;
                        let anchor = self.anchor_of(rest.get("anchor"))?;
                        return Ok(Value::Apos(anchor, x, y));
                    }
                }
                let map = self.resolve_args(kind, &["x", "y", "anchor"], args)?;
                let anchor = self.anchor_of(map.get("anchor"))?;
                match (map.get("x"), map.get("y")) {
                    (Some(Value::Number(x)), Some(Value::Number(y))) => Ok(Value::Apos(anchor, *x, *y)),
                    _ => err("TypeError.ArgumentType", "Pos needs x and y (Number)"),
                }
            }
            "Color" => {
                let map = self.resolve_args(kind, &["r", "g", "b", "a"], args)?;
                let ch = |name: &str, scale: f64, default: Option<f64>| -> Result<f32> {
                    match (map.get(name), default) {
                        (Some(Value::Number(v)), _) => Ok((v / scale) as f32),
                        (Some(v), _) => err("TypeError.ArgumentType", format!("Color.{name} expects Number, found {}", v.type_name())),
                        (None, Some(d)) => Ok(d as f32),
                        (None, None) => err("TypeError.ArityMismatch", format!("Color needs {name}")),
                    }
                };
                Ok(Value::Color([ch("r", 255.0, None)?, ch("g", 255.0, None)?, ch("b", 255.0, None)?, ch("a", 1.0, Some(1.0))?]))
            }
            "Tuple" => Ok(Value::Tuple(self.positional(kind, args)?)),
            "List" => Ok(Value::List(Rc::new(RefCell::new(self.positional(kind, args)?)))),
            "Dict" => {
                let mut items: Vec<(String, Value)> = Vec::new();
                for a in args {
                    let Some(key) = a.name.clone() else {
                        return err("TypeError.ArgumentType", "Dict takes named arguments, as Dict(k = v); write { \"k\": v } for a key that is not a name");
                    };
                    let v = self.eval(&a.value)?;
                    match items.iter_mut().find(|(k, _)| *k == key) {
                        Some(slot) => slot.1 = v,
                        None => items.push((key, v)),
                    }
                }
                Ok(Value::Dict(Rc::new(RefCell::new(items))))
            }
            "Range" => {
                let map = self.resolve_args(kind, &["start", "end"], args)?;
                match (map.get("start"), map.get("end")) {
                    (Some(Value::Number(a)), Some(Value::Number(b))) => Ok(Value::Range(*a as i64, *b as i64)),
                    _ => err("TypeError.ArgumentType", "Range needs start and end (Number)"),
                }
            }
            _ => {
                if let Some(sch) = schema(kind) {
                    let fields: Vec<&str> = sch.iter().map(|(n, _)| *n).collect();
                    let map = self.resolve_args(kind, &fields, args)?;
                    let mut attrs = HashMap::new();
                    for (name, v) in map {
                        let expected = sch.iter().find(|(n, _)| *n == name).map(|(_, t)| *t).expect("field exists");
                        if !self.matches_type(&v, expected) {
                            return Err(self.wrong_type("TypeError.ArgumentType", &format!("{kind}.{name}"), expected, &v));
                        }
                        if kind == "TextArea" && name == "font" {
                            if let Value::Str(family) = &v {
                                if !self.cache_mut().family_exists(family) {
                                    return err("RuntimeError.FontNotFound", format!("font \"{family}\" not found"));
                                }
                            }
                        }
                        attrs.insert(name, v);
                    }
                    return Ok(Value::Object(Rc::new(RefCell::new(Object { kind: kind.to_string(), decl: None, attrs, children: vec![], tracks: vec![] }))));
                }
                let Some(ty) = self.lookup_type(kind) else {
                    return self.cannot_construct(kind);
                };
                self.construct_user(&ty, args)
            }
        }
    }

    /// 宣言した型の func / method / 複製のメソッドを呼ぶ。当てはまらなければ None
    fn user_member(&mut self, receiver: &Value, name: &str, args: &[Arg]) -> Result<Option<Value>> {
        let (ty, self_value) = match receiver {
            Value::Type(t) => (t.clone(), None),
            Value::Record(r) => match &r.decl {
                Some(t) => (t.clone(), Some(receiver.clone())),
                None => return Ok(None),
            },
            Value::Object(o) => match &o.borrow().decl {
                Some(t) => (t.clone(), Some(receiver.clone())),
                None => return Ok(None),
            },
            _ => return Ok(None),
        };
        if let Some(v) = self_value.clone() {
            if let Some(out) = self.copy_method(&ty, &v, name, args)? {
                return Ok(Some(out));
            }
        }
        let Some(member) = ty.decl.members.iter().find(|m| m.name == name && m.receiver == self_value.is_some()) else {
            return Ok(None);
        };
        if member.private && !self.is_inside(&ty.decl.name) {
            return err("NameError.UndefinedAttribute", format!("{}.{name} is private", ty.decl.name));
        }
        let mut values: Vec<(Option<String>, Value)> = Vec::new();
        if let Some(v) = self_value {
            values.push((None, v));
        }
        for a in args {
            values.push((a.name.clone(), self.eval(&a.value)?));
        }
        let closure = Closure { def: member.def.clone(), scopes: ty.scopes.clone(), type_text: None };
        // 型名から new を呼んだときも、その中の型名はフィールドから作る (new を呼び直さない)
        let is_new = !member.receiver && name == "new";
        if is_new {
            self.constructing.push(ty.decl.name.clone());
        }
        self.calling = Some(ty.decl.name.clone());
        let out = self.apply(&closure, values);
        if is_new {
            self.constructing.pop();
        }
        out.map(Some)
    }

    /// いまその型の本体を実行しているか。private はこのときだけ見える
    fn is_inside(&self, name: &str) -> bool {
        self.inside.last().is_some_and(|n| n == name)
    }

    /// private なフィールドを外から触っていないか
    fn field_ok(&self, decl: &Option<Rc<UserType>>, field: &str) -> Result<()> {
        let Some(ty) = decl else { return Ok(()) };
        match ty.decl.fields.iter().find(|f| f.name == field) {
            Some(f) if f.private && !self.is_inside(&ty.decl.name) => {
                err("NameError.UndefinedAttribute", format!("{}.{field} is private", ty.decl.name))
            }
            _ => Ok(()),
        }
    }

    fn record_attr(&self, r: &Rc<Record>, field: &str) -> Result<Value> {
        self.field_ok(&r.decl, field)?;
        record_field(r, field)
    }

    /// copy / shallowCopy / deepCopy
    fn copy_method(&mut self, ty: &Rc<UserType>, receiver: &Value, name: &str, args: &[Arg]) -> Result<Option<Value>> {
        let wanted = matches!(name, "copy" | "shallowCopy" | "deepCopy");
        if !wanted || ty.decl.members.iter().any(|m| m.name == name && m.receiver) {
            return Ok(None);
        }
        if ty.decl.nocopy {
            return err("NameError.UndefinedAttribute", format!("{} is @nocopy, so it cannot be copied", ty.decl.name));
        }
        match (receiver, name) {
            (Value::Record(r), "copy") => {
                let changed = self.resolve_args(&ty.decl.name, &r.fields.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>(), args)?;
                let fields = r.fields.iter().map(|(f, v)| (f.clone(), changed.get(f).cloned().unwrap_or_else(|| v.clone()))).collect();
                Ok(Some(Value::Record(Rc::new(Record { name: r.name.clone(), decl: r.decl.clone(), fields }))))
            }
            (Value::Object(o), "shallowCopy") | (Value::Object(o), "deepCopy") => {
                if name == "deepCopy" && ty.decl.nodeepcopy {
                    return err("NameError.UndefinedAttribute", format!("{} is @nodeepcopy, so it has no deepCopy", ty.decl.name));
                }
                if !args.is_empty() {
                    return err("TypeError.ArityMismatch", format!("{name} takes no arguments"));
                }
                let src = o.borrow();
                let attrs = if name == "deepCopy" { src.attrs.iter().map(|(k, v)| (k.clone(), deep_copy(v))).collect() } else { src.attrs.clone() };
                Ok(Some(Value::Object(Rc::new(RefCell::new(Object {
                    kind: src.kind.clone(),
                    decl: src.decl.clone(),
                    attrs,
                    children: src.children.clone(),
                    tracks: src.tracks.clone(),
                })))))
            }
            _ => Ok(None),
        }
    }

    /// スコープに束縛された型を引く
    fn lookup_type(&self, name: &str) -> Option<Rc<UserType>> {
        match self.scopes.iter().rev().find_map(|sc| sc.borrow().get(name).cloned()) {
            Some(Value::Type(t)) => Some(t),
            _ => None,
        }
    }

    /// フィールドの宣言から値を作る。private で既定値の無いものがあれば作れない
    fn build_from_fields(&mut self, ty: &Rc<UserType>, args: Vec<(Option<String>, Value)>) -> Result<Value> {
        let decl = ty.decl.clone();
        let open: Vec<&str> = decl.fields.iter().filter(|f| !f.private).map(|f| f.name.as_str()).collect();
        if let Some(f) = decl.fields.iter().find(|f| f.private && f.default.is_none()) {
            return err("TypeError.ArityMismatch", format!("{}.{} is private and has no default, so write \"func new\"", decl.name, f.name));
        }
        let mut given: HashMap<String, Value> = HashMap::new();
        let mut next = 0;
        for (name, v) in args {
            let field = match name {
                Some(n) => {
                    if !open.contains(&n.as_str()) {
                        return err("NameError.UndefinedAttribute", format!("{} has no field \"{n}\"", decl.name));
                    }
                    n
                }
                None => match open.get(next) {
                    Some(f) => {
                        next += 1;
                        (*f).to_string()
                    }
                    None => return err("TypeError.ArityMismatch", format!("{} takes {} fields, more were given", decl.name, open.len())),
                },
            };
            if given.insert(field.clone(), v).is_some() {
                return err("TypeError.ArgumentType", format!("{}.{field} is given twice", decl.name));
            }
        }
        let mut fields: Vec<(String, Value)> = Vec::new();
        for f in &decl.fields {
            let v = match given.remove(&f.name) {
                Some(v) => v,
                None => match &f.default {
                    Some(d) => self.eval(d)?,
                    None => return err("TypeError.ArityMismatch", format!("{}.{} is not given", decl.name, f.name)),
                },
            };
            self.check_type(&v, &f.ann.name)?;
            fields.push((f.name.clone(), v));
        }
        Ok(if decl.immutable {
            Value::Record(Rc::new(Record { name: decl.name.clone(), decl: Some(ty.clone()), fields }))
        } else {
            Value::Object(Rc::new(RefCell::new(Object {
                kind: decl.name.clone(),
                decl: Some(ty.clone()),
                attrs: fields.into_iter().collect(),
                children: vec![],
                tracks: vec![],
            })))
        })
    }

    /// ユーザーが宣言した型を作る。func new があればそれを呼び、無ければフィールドから作る
    fn construct_user(&mut self, ty: &Rc<UserType>, args: &[Arg]) -> Result<Value> {
        if let Some(note) = &ty.decl.deprecated {
            if self.warned.insert(ty.decl.name.clone()) {
                let tail = if note.is_empty() { String::new() } else { format!(": {note}") };
                eprintln!("warning: {} is deprecated{tail}", ty.decl.name);
            }
        }
        let news: Vec<&crate::lang::ast::MemberDecl> = ty.decl.members.iter().filter(|m| !m.receiver && m.name == "new").collect();
        // func new の中で型名を呼んだら、フィールドから作る (new を呼び直さない)
        if news.is_empty() || self.constructing.last().is_some_and(|n| *n == ty.decl.name) {
            let values = args.iter().map(|a| self.eval(&a.value).map(|v| (a.name.clone(), v))).collect::<Result<Vec<_>>>()?;
            return self.build_from_fields(ty, values);
        }
        let values = args.iter().map(|a| self.eval(&a.value).map(|v| (a.name.clone(), v))).collect::<Result<Vec<_>>>()?;
        // 必須の数が合うものを選ぶ
        let count = values.iter().filter(|(n, _)| n.is_none()).count();
        let fits = |m: &&&crate::lang::ast::MemberDecl| {
            let required = m.def.params.iter().filter(|p| p.default.is_none()).count();
            count >= required && count <= m.def.params.len()
        };
        let Some(member) = news.iter().find(fits).or_else(|| news.first()) else {
            return err("TypeError.ArityMismatch", format!("no new of {} takes {count} arguments", ty.decl.name));
        };
        let closure = Closure { def: member.def.clone(), scopes: ty.scopes.clone(), type_text: None };
        self.constructing.push(ty.decl.name.clone());
        self.calling = Some(ty.decl.name.clone());
        let out = self.apply(&closure, values);
        self.constructing.pop();
        out
    }

    /// 組み込みの型の名前か
    fn is_builtin_type(name: &str) -> bool {
        crate::docs::TYPES.iter().any(|t| t.name == name)
    }

    /// 大文字で始まる名前は型
    fn is_type(&self, name: &str) -> bool {
        name.starts_with(char::is_uppercase) && (Self::is_builtin_type(name) || self.types.contains_key(name) || self.lookup_type(name).is_some())
    }

    fn call(&mut self, callee: &Expr, args: &[Arg]) -> Result<Value> {
        match callee {
            Expr::Ident(name) if self.is_type(name) => {
                let name = name.clone();
                self.construct(&name, args)
            }
            Expr::Ident(name) if name == "log" => {
                let parts = args.iter().map(|a| self.eval(&a.value).map(|v| v.to_string())).collect::<Result<Vec<_>>>()?;
                eprintln!("{}", parts.join(" "));
                Ok(Value::Nothing)
            }
            Expr::Ident(name) if name == "type_of" => {
                let [arg] = args else {
                    return err("TypeError.ArityMismatch", format!("type_of takes 1 argument, {} given", args.len()));
                };
                Ok(Value::Str(self.eval(&arg.value)?.type_name()))
            }
            Expr::Attr(target, method) if method == "format" => {
                let Value::Str(template) = self.eval(target)? else {
                    return err("TypeError.ArgumentType", "format is a method of String");
                };
                let values = args.iter().map(|a| self.eval(&a.value)).collect::<Result<Vec<_>>>()?;
                format(&template, &values)
            }
            Expr::Attr(target, method) => {
                let receiver = self.eval(target)?;
                // 型から func を呼ぶ / 値から method を呼ぶ
                if let Some(out) = self.user_member(&receiver, method, args)? {
                    return Ok(out);
                }
                if let Value::Module(m) = &receiver {
                    let Some(item) = m.items.get(method).cloned() else {
                        return err("NameError.UndefinedAttribute", format!("module {} has no item \"{method}\"", m.name));
                    };
                    let values = args.iter().map(|a| self.eval(&a.value)).collect::<Result<Vec<_>>>()?;
                    return match item {
                        Value::Builtin(name) => stdlib::math::call(name, &values),
                        Value::Func(closure) => self.apply(&closure, values.into_iter().map(|v| (None, v)).collect()),
                        v => err("TypeError.ArgumentType", format!("{} is not callable", v.type_name())),
                    };
                }
                let values = args.iter().map(|a| self.eval(&a.value).map(|v| (a.name.clone(), v))).collect::<Result<Vec<_>>>()?;
                match receiver {
                    Value::Object(obj) => self.method(&obj, method, values),
                    Value::Motion(m) if method == "apply" => self.apply_motion(&m, values),
                    Value::Motion(m) if method == "reverse" && values.is_empty() => Ok(Value::Motion(Rc::new(m.reverse()))),
                    Value::Timeline(t) if method == "reverse" && values.is_empty() => Ok(Value::Timeline(Rc::new(t.reverse()))),
                    other => self.collection_method(&other, method, values),
                }
            }
            callee => match self.eval(callee)? {
                Value::BuiltinType(name) => self.construct(&name, args),
                Value::Type(ty) => self.construct_user(&ty, args),
                Value::Func(closure) => {
                    let values = args.iter().map(|a| self.eval(&a.value).map(|v| (a.name.clone(), v))).collect::<Result<Vec<_>>>()?;
                    self.apply(&closure, values)
                }
                Value::Builtin(name) => {
                    let values = args.iter().map(|a| self.eval(&a.value)).collect::<Result<Vec<_>>>()?;
                    stdlib::math::call(name, &values)
                }
                v => err("TypeError.ArgumentType", format!("{} is not callable", v.type_name())),
            },
        }
    }

    fn eval_motion(&mut self, def: &MotionDef) -> Result<Value> {
        let is_assign = def.rows.iter().flat_map(|r| &r.items).any(|i| matches!(i, RowItem::Assign(..)));
        let is_value = def.rows.iter().flat_map(|r| &r.items).any(|i| matches!(i, RowItem::Value(_)));
        if is_assign && is_value {
            return err("SyntaxError.UnexpectedToken", "a motion cannot mix assignments and values");
        }
        let relative = def.rows.first().is_some_and(|r| r.relative);
        if def.rows.iter().any(|r| r.relative != relative) {
            return err("ValueError.DurationRequired", "keyframe times must be all Duration or all Number (0..1)");
        }
        if relative && def.rows.iter().any(|r| !(0.0..=1.0).contains(&r.time)) {
            return err("ValueError.OutOfRange", "relative keyframe time must be within 0..1");
        }
        // 対象と属性パスを列挙した形 → Timeline
        if let Some((target_expr, paths)) = &def.target {
            let target = self.eval_object(target_expr)?;
            let mut keyframes = Vec::new();
            for row in &def.rows {
                if row.items.len() > paths.len() {
                    return err("TypeError.ArityMismatch", format!("keyframe at {}s has {} values but {} attributes are listed", row.time, row.items.len(), paths.len()));
                }
                let assigns = row
                    .items
                    .iter()
                    .zip(paths)
                    .map(|(item, path)| {
                        let RowItem::Value(e) = item else { unreachable!() };
                        TlAssign { target: target.clone(), path: path.clone(), expr: e.clone(), scopes: self.scopes.clone() }
                    })
                    .collect();
                keyframes.push(TlKeyframe { time: row.time, end: row.end, assigns, ease: row.ease.clone() });
            }
            return Ok(Value::Timeline(Rc::new(Timeline { param: "t".into(), keyframes, relative, duration: Cell::new(None) }.normalize())));
        }
        // 属性への割り当て → Timeline
        if is_assign {
            let param = def.params.first().cloned().unwrap_or_else(|| "t".into());
            let mut keyframes = Vec::new();
            for row in &def.rows {
                let mut assigns = Vec::new();
                for item in &row.items {
                    let RowItem::Assign(obj, path, e) = item else { unreachable!() };
                    if path.is_empty() {
                        return err("SyntaxError.UnexpectedToken", "keyframe must assign to an attribute");
                    }
                    let target = self.eval_object(obj)?;
                    assigns.push(TlAssign { target, path: path.clone(), expr: e.clone(), scopes: self.scopes.clone() });
                }
                keyframes.push(TlKeyframe { time: row.time, end: row.end, assigns, ease: row.ease.clone() });
            }
            return Ok(Value::Timeline(Rc::new(Timeline { param, keyframes, relative, duration: Cell::new(None) }.normalize())));
        }
        // 値の表 → Motion。params[0] は行の時刻、以降は左の列
        if def.rows.iter().any(|r| r.end.is_some()) {
            return err("SyntaxError.UnexpectedToken", "a keyframe range (0..1:) needs a target; use it with assignments or motion target [...]");
        }
        let mut rows = Vec::new();
        for row in &def.rows {
            self.scopes.push(new_scope());
            let result = (|| {
                if let Some(t) = def.params.first() {
                    self.scopes.last().expect("scope").borrow_mut().insert(t.clone(), Value::Number(row.time));
                }
                let mut values = Vec::new();
                for (i, item) in row.items.iter().enumerate() {
                    let RowItem::Value(e) = item else { unreachable!() };
                    let v = self.eval(e)?;
                    if let Some(name) = def.params.get(i + 1) {
                        self.scopes.last().expect("scope").borrow_mut().insert(name.clone(), v.clone());
                    }
                    values.push(v);
                }
                Ok(values)
            })();
            self.scopes.pop();
            rows.push(MotionRowVal { time: row.time, values: result?, ease: row.ease.clone() });
        }
        Ok(Value::Motion(Rc::new(Motion { rows, relative, duration: Cell::new(None) }.normalize())))
    }

    /// Motion の各行で f(target, t, [列...]) を呼び、その中で target の属性に代入された値をキーフレームとして記録する
    fn apply_motion(&mut self, motion: &Motion, args: Vec<(Option<String>, Value)>) -> Result<Value> {
        let [(None, Value::Object(target)), (None, Value::Func(f))] = args.as_slice() else {
            return err("TypeError.ArgumentType", "Motion.apply expects (target, func)");
        };
        let mut keyframes = Vec::new();
        for row in &motion.rows {
            let before = target.borrow().attrs.clone();
            let cols_value = Value::List(Rc::new(RefCell::new(row.values.clone())));
            self.apply(f, vec![(None, Value::Object(target.clone())), (None, Value::Duration(row.time)), (None, cols_value)])?;
            let after = target.borrow().attrs.clone();
            let assigns = after
                .into_iter()
                .filter(|(k, v)| before.get(k).is_none_or(|old| !equals(old, v)))
                .map(|(attr, v)| TlAssign { target: target.clone(), path: vec![attr], expr: Expr::from_value(&v), scopes: vec![] })
                .collect();
            target.borrow_mut().attrs = before;
            keyframes.push(TlKeyframe { time: row.time, end: None, assigns, ease: row.ease.clone() });
        }
        Ok(Value::Timeline(Rc::new(Timeline { param: "t".into(), keyframes, relative: motion.relative, duration: Cell::new(motion.duration.get()) }.normalize())))
    }

    fn method(&mut self, obj: &ObjRef, method: &str, args: Vec<(Option<String>, Value)>) -> Result<Value> {
        let kind = obj.borrow().kind.clone();
        match (kind.as_str(), method) {
            ("View", "place") => match args.first() {
                Some((None, Value::Object(child))) if child.borrow().kind == "View" => {
                    if Rc::ptr_eq(child, obj) {
                        return err("ValueError.OutOfRange", "a View cannot be placed in itself");
                    }
                    // 置き先での位置と大きさは子 View の属性として持つ
                    for (name, v) in &args[1..] {
                        match name.as_deref() {
                            Some("at") => self.set_attr(child, "position", v.clone())?,
                            Some("w") => self.set_attr(child, "w", v.clone())?,
                            Some("h") => self.set_attr(child, "h", v.clone())?,
                            Some(other) => return err("TypeError.ArgumentType", format!("View.place has no argument \"{other}\"")),
                            None => return err("TypeError.ArgumentType", "View.place takes one positional argument"),
                        }
                    }
                    let c = child.borrow();
                    if !c.attrs.contains_key("position") || !(c.attrs.contains_key("w") || c.attrs.contains_key("h")) {
                        return err("TypeError.ArityMismatch", "placing a View needs at: and w: or h:");
                    }
                    drop(c);
                    obj.borrow_mut().children.push(child.clone());
                    Ok(Value::Nothing)
                }
                Some((None, Value::Object(child))) if self.matches_type(&Value::Object(child.clone()), "Placeable") => {
                    // 図形は自分の position で置くので、at / w / h は受け取らない
                    if let Some((name, _)) = args.get(1) {
                        let what = name.clone().unwrap_or_else(|| "a second value".to_string());
                        let kind = child.borrow().kind.clone();
                        return err("TypeError.ArgumentType", format!("View.place has no argument \"{what}\" for a {kind}; set its position"));
                    }
                    obj.borrow_mut().children.push(child.clone());
                    Ok(Value::Nothing)
                }
                Some((None, Value::Motion(_))) => err("TypeError.NotPlaceable", "Motion cannot be placed; apply it to make a Timeline"),
                Some((None, v)) => err("TypeError.ArgumentType", format!("View.place expects Placeable, found {}", v.type_name())),
                _ => err("TypeError.ArgumentType", "View.place expects a Shape as the first argument"),
            },
            ("View", "addTrack") | ("Timeline", "place") => {
                let who = format!("{kind}.{method}");
                let track = match args.first() {
                    Some((None, Value::Timeline(tl))) if tl.needs_duration() => {
                        return err("ValueError.DurationRequired", "a timeline with relative (0..1) keyframes needs duration; set tl.duration = 8s");
                    }
                    Some((None, Value::Timeline(tl))) => Track::Timeline(tl.clone()),
                    Some((None, Value::Object(o))) if matches!(o.borrow().kind.as_str(), "Timeline" | "View") => Track::Container(o.clone()),
                    Some((None, Value::Object(o))) if o.borrow().kind == "Subtitle" => {
                        if !o.borrow().attrs.contains_key("duration") {
                            return err("ValueError.DurationRequired", "a Subtitle needs duration before it is placed");
                        }
                        Track::Subtitle(o.clone())
                    }
                    Some((None, Value::Audio(a))) => Track::Audio(a.clone(), Clip { cut: None, volume: 1.0, looping: false }),
                    Some((None, Value::Motion(_))) => return err("TypeError.NotPlaceable", "Motion cannot be placed; apply it to make a Timeline"),
                    Some((None, v)) => return err("TypeError.ArgumentType", format!("{who} expects Timeline, Audio or Subtitle, found {}", v.type_name())),
                    _ => return err("TypeError.ArgumentType", format!("{who} expects a Timeline as the first argument")),
                };
                let mut placed = Placed { track, at: 0.0, fade_in: 0.0, fade_out: 0.0 };
                for (name, v) in &args[1..] {
                    let arg = name.as_deref().ok_or_else(|| MophError::new("TypeError.ArgumentType", format!("{who} takes one positional argument")))?;
                    // 音声だけの引数
                    if let Track::Audio(_, clip) = &mut placed.track {
                        match (arg, v) {
                            ("duration", Value::Duration(d)) => {
                                clip.cut = Some(*d);
                                continue;
                            }
                            ("volume", Value::Number(n)) => {
                                clip.volume = *n;
                                continue;
                            }
                            ("loop", Value::Bool(b)) => {
                                clip.looping = *b;
                                continue;
                            }
                            ("duration" | "volume" | "loop", v) => {
                                let expected = match arg {
                                    "volume" => "Number",
                                    "loop" => "Bool",
                                    _ => "Duration",
                                };
                                return err("TypeError.ArgumentType", format!("{who} {arg} expects {expected}, found {}", v.type_name()));
                            }
                            _ => {}
                        }
                    }
                    let slot = match arg {
                        "at" => &mut placed.at,
                        "fadeIn" | "fadeOut" if matches!(placed.track, Track::Subtitle(_)) => {
                            return err("TypeError.ArgumentType", format!("{who}: a Subtitle has no {arg}; set its duration"));
                        }
                        "fadeIn" => &mut placed.fade_in,
                        "fadeOut" => &mut placed.fade_out,
                        other => return err("TypeError.ArgumentType", format!("{who} has no argument \"{other}\"")),
                    };
                    let Value::Duration(d) = v else {
                        return err("TypeError.ArgumentType", format!("{who} {arg} expects Duration, found {}", v.type_name()));
                    };
                    *slot = *d;
                }
                obj.borrow_mut().tracks.push(placed);
                Ok(Value::Nothing)
            }
            _ => err("NameError.UndefinedAttribute", format!("{kind} has no method \"{method}\"")),
        }
    }

    fn attr_of(&self, obj: &ObjRef, attr: &str) -> Result<Value> {
        let obj = obj.borrow();
        self.field_ok(&obj.decl, attr)?;
        obj.attrs
            .get(attr)
            .cloned()
            .ok_or_else(|| MophError::new("NameError.UndefinedAttribute", format!("{} has no attribute \"{attr}\"", obj.kind)))
    }

    fn eval_bool(&mut self, e: &Expr) -> Result<bool> {
        match self.eval(e)? {
            Value::Bool(b) => Ok(b),
            v => err("TypeError.OperandType", format!("expected Bool, found {}", v.type_name())),
        }
    }

    /// 新しいスコープで文を実行し、最後の値を返す。return は値として扱う (関数の中で if から return する用)
    /// 式としてのブロック (if の枝など)。中で return したら値を覚えておき、外側の文の列が関数を抜ける
    fn block(&mut self, body: &[Stmt]) -> Result<Value> {
        self.scopes.push(new_scope());
        let result = self.run_block(body);
        self.scopes.pop();
        match result? {
            Flow::Next(v) => Ok(v),
            Flow::Return(v) => {
                self.returning = Some(v);
                Ok(Value::Nothing)
            }
        }
    }

    /// 関数を呼ぶ。名前付き引数はパラメータ名で、残りは位置で対応させる
    pub fn apply(&mut self, closure: &Closure, args: Vec<(Option<String>, Value)>) -> Result<Value> {
        // private が見えるのは、その型のメンバーの本体だけ。ふつうの呼び出しは空の枠を積む
        let owner = self.calling.take().unwrap_or_default();
        self.inside.push(owner);
        let out = self.apply_inner(closure, args);
        self.inside.pop();
        out
    }

    fn apply_inner(&mut self, closure: &Closure, args: Vec<(Option<String>, Value)>) -> Result<Value> {
        let params = &closure.def.params;
        let (named, positional): (Vec<_>, Vec<_>) = args.into_iter().partition(|(n, _)| n.is_some());
        let mut positional = positional.into_iter().map(|(_, v)| v);
        let mut named: HashMap<String, Value> = named.into_iter().map(|(n, v)| (n.expect("named"), v)).collect();

        let saved = std::mem::replace(&mut self.scopes, closure.scopes.clone());
        self.scopes.push(new_scope());
        let result = (|| {
            for param in params {
                let by_name = match &param.pattern {
                    Pattern::Name(n) => named.remove(n),
                    _ => None,
                };
                let value = match by_name.or_else(|| positional.next()) {
                    Some(v) => v,
                    None => match &param.default {
                        Some(d) => self.eval(d)?,
                        None => return err("TypeError.ArityMismatch", format!("{} is not given", pattern_text(&param.pattern))),
                    },
                };
                // 型を書いてあれば、渡された値を確かめる
                if let Some(ann) = &param.ann {
                    if !self.matches_type(&value, &self.real_type_name(&ann.name)) {
                        return Err(self.wrong_type("TypeError.ArgumentType", &pattern_text(&param.pattern), &ann.name, &value));
                    }
                }
                self.bind(&param.pattern, value)?;
            }
            if positional.next().is_some() {
                return err("TypeError.ArityMismatch", format!("function takes {} arguments, more were given", params.len()));
            }
            if let Some(name) = named.keys().next() {
                return err("TypeError.ArgumentType", format!("unknown named argument \"{name}\""));
            }
            let out = self.run_block(&closure.def.body).map(Flow::value)?;
            if let Some(ann) = &closure.def.returns {
                if !self.matches_type(&out, &self.real_type_name(&ann.name)) {
                    return Err(self.wrong_type("TypeError.ArgumentType", "the return value", &ann.name, &out));
                }
            }
            Ok(out)
        })();
        self.scopes = saved;
        result
    }

    /// 出力する View から辿れる全オブジェクトの属性を覚える。描画を始める前に 1 度呼ぶ
    pub fn snapshot(&mut self, view: &ObjRef) {
        let mut seen: Vec<ObjRef> = Vec::new();
        collect_objects(view, &mut seen);
        self.initial = seen.into_iter().map(|o| { let attrs = o.borrow().attrs.clone(); (o, attrs) }).collect();
    }

    /// 描画の前に呼ぶ。全オブジェクトを初期状態に戻す (状態を時刻の関数にする)。時刻が戻っていたら、終わった Timeline の記憶を捨てる
    pub fn begin_frame(&mut self, t: f64) {
        for (obj, attrs) in &self.initial {
            obj.borrow_mut().attrs = attrs.clone();
        }
        if t < self.last_t {
            self.finished.clear();
        }
        self.last_t = t;
    }

    /// 置かれた Timeline を、親の時間軸の時刻 t で対象に書き込む。
    /// 開始前は何もしない。終了後は最後の状態を保つ (duration で切る)。
    /// origin は親の絶対開始時刻で、終わった Timeline を区別する鍵に使う
    pub fn apply_track(&mut self, placed: &Placed, t: f64) -> Result<()> {
        self.apply_track_from(placed, t, 0.0)
    }

    fn apply_track_from(&mut self, placed: &Placed, t: f64, origin: f64) -> Result<()> {
        let duration = placed.track.duration();
        let local = (t - placed.at).min(duration);
        if local < 0.0 {
            return Ok(());
        }
        let origin = origin + placed.at;
        match &placed.track {
            // 音声と字幕は描画には関わらない (render が動画に付ける)
            Track::Audio(..) | Track::Subtitle(_) => {}
            Track::Container(obj) => {
                let children = obj.borrow().tracks.clone();
                for child in &children {
                    self.apply_track_from(child, local, origin)?;
                }
            }
            Track::Timeline(tl) if local >= duration && placed.fade_out <= 0.0 => {
                // 終わった Timeline。最後の値を 1 度だけ評価し、以後は書き込みだけにする (書き込み順は保つ)
                let key = (Rc::as_ptr(tl) as usize, origin.to_bits());
                if let Some(values) = self.finished.get(&key) {
                    for (target, path, value) in values.clone() {
                        self.set_path(&target, &path, value)?;
                    }
                    return Ok(());
                }
                self.apply_timeline(tl, local)?;
                let mut values = Vec::new();
                for a in tl.keyframes.iter().flat_map(|k| &k.assigns) {
                    if values.iter().any(|(o, p, _): &(ObjRef, Vec<String>, Value)| Rc::ptr_eq(o, &a.target) && *p == a.path) {
                        continue;
                    }
                    if let Some(v) = get_path(&a.target, &a.path) {
                        values.push((a.target.clone(), a.path.clone(), v));
                    }
                }
                self.finished.insert(key, values);
            }
            Track::Timeline(tl) => {
                self.apply_timeline(tl, local)?;
                let fade = match (placed.fade_in > 0.0 && local < placed.fade_in, placed.fade_out > 0.0 && local > duration - placed.fade_out) {
                    (true, _) => Some(local / placed.fade_in),
                    (_, true) => Some((duration - local) / placed.fade_out),
                    _ => None,
                };
                if let Some(factor) = fade {
                    for target in timeline_targets(tl) {
                        self.set_attr(&target, "opacity", Value::Number(factor))?;
                    }
                }
            }
        }
        Ok(())
    }

    /// 時刻 t における Timeline の値を対象に書き込む
    pub fn apply_timeline(&mut self, tl: &Timeline, t: f64) -> Result<()> {
        // (対象, 属性パス) ごとにキーフレームを集める
        let mut by_attr: Vec<(ObjRef, &[String], Vec<(&TlKeyframe, &TlAssign)>)> = vec![];
        for kf in &tl.keyframes {
            for a in &kf.assigns {
                match by_attr.iter_mut().find(|(o, path, _)| Rc::ptr_eq(o, &a.target) && *path == a.path.as_slice()) {
                    Some((_, _, list)) => list.push((kf, a)),
                    None => by_attr.push((a.target.clone(), &a.path, vec![(kf, a)])),
                }
            }
        }
        // 書かれた時刻 × scale が実際の時刻。t は実際の時刻なので、書かれた時刻の軸に戻して比べる
        let t = t / tl.time_scale();
        for (target, path, list) in by_attr {
            // 範囲の行の中なら、その時刻で式を評価するだけ
            if let Some((_, a)) = list.iter().find(|(kf, _)| kf.end.is_some_and(|e| kf.time <= t && t <= e)) {
                let value = self.eval_assign(tl, a, t)?;
                let value = match (path.first(), schema(&target.borrow().kind)) {
                    (Some(attr), Some(sch)) if path.len() == 1 => match sch.iter().find(|(n, _)| n == attr) {
                        Some(_) => value,
                        None => value,
                    },
                    _ => value,
                };
                self.set_path(&target, path, value)?;
                continue;
            }
            // 点の列。範囲の行は始点と終点の 2 点になる
            let points: Vec<(f64, &TlKeyframe, &TlAssign)> = list
                .iter()
                .flat_map(|&(kf, a)| match kf.end {
                    Some(e) => vec![(kf.time, kf, a), (e, kf, a)],
                    None => vec![(kf.time, kf, a)],
                })
                .collect();
            let prev = points.iter().rev().find(|(time, ..)| *time <= t).or(points.first());
            let next = points.iter().find(|(time, ..)| *time > t);
            let Some(&(t0, _, a0)) = prev else { continue };
            let v0 = self.eval_assign(tl, a0, t0)?;
            // 修飾子は区間 (kf0, kf1) のもので、kf1 に付いている
            let value = match next {
                Some(&(t1, kf1, a1)) if t0 < t => {
                    let v1 = self.eval_assign(tl, a1, t1)?;
                    interpolate(&v0, &v1, ease(kf1.ease.as_deref(), (t - t0) / (t1 - t0)))
                }
                _ => v0,
            };
            let value = match (path.first(), schema(&target.borrow().kind)) {
                (Some(attr), Some(sch)) if path.len() == 1 => match sch.iter().find(|(n, _)| n == attr) {
                    Some(_) => value,
                    None => value,
                },
                _ => value,
            };
            self.set_path(&target, path, value)?;
        }
        Ok(())
    }

    /// report 用。キーフレームの式をその行の時刻で評価する
    pub fn eval_assign_pub(&mut self, tl: &Timeline, a: &TlAssign, time: f64) -> Result<Value> {
        self.eval_assign(tl, a, time)
    }

    fn eval_assign(&mut self, tl: &Timeline, a: &TlAssign, time: f64) -> Result<Value> {
        let saved = std::mem::replace(&mut self.scopes, a.scopes.clone());
        let scope = new_scope();
        scope.borrow_mut().insert(tl.param.clone(), Value::Number(time));
        self.scopes.push(scope);
        let result = self.eval(&a.expr);
        self.scopes = saved;
        result
    }
}

/// "{name}" を順に引数で置き換える。名前は説明用で、対応は位置で決まる
fn format(template: &str, values: &[Value]) -> Result<Value> {
    let mut out = String::new();
    let mut rest = template;
    let mut index = 0;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}') else {
            return err("ValueError.OutOfRange", format!("unterminated {{ in {template:?}"));
        };
        let Some(v) = values.get(index) else {
            return err("TypeError.ArityMismatch", format!("format expects {} arguments, {} given", index + 1, values.len()));
        };
        out.push_str(&v.to_string());
        index += 1;
        rest = &rest[start + end + 1..];
    }
    if index < values.len() {
        return err("TypeError.ArityMismatch", format!("format expects {index} arguments, {} given", values.len()));
    }
    out.push_str(rest);
    Ok(Value::Str(out))
}

/// パターンが束縛する名前を集める
fn collect_names(pat: &Pattern, out: &mut Vec<String>) {
    match pat {
        Pattern::Name(n) => out.push(n.clone()),
        Pattern::Tuple(items) | Pattern::List(items) => items.iter().for_each(|p| collect_names(p, out)),
    }
}

/// 標準ライブラリ。import で束縛する
/// 音声ファイルを読む。長さは ffprobe で調べる (ffmpeg に付属)
fn load_audio(name: &str, path: std::path::PathBuf) -> Result<Audio> {
    if !path.is_file() {
        return err("NameError.UndefinedVariable", format!("cannot read \"{name}\": {} is not a file", path.display()));
    }
    // 音声ストリームがあることと、全体の長さを調べる。出力は "audio" の行 (ストリームごと) と長さの行
    let output = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "a", "-show_entries", "stream=codec_type:format=duration", "-of", "default=nw=1:nk=1"])
        .arg(&path)
        .output()
        .map_err(|e| MophError::new("RuntimeError.AudioUnreadable", format!("ffprobe is needed to read \"{name}\": {e}")))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let length = match lines.as_slice() {
        [streams @ .., duration] if streams.contains(&"audio") => duration.parse::<f64>().ok(),
        _ => None,
    };
    let Some(length) = length else {
        return err("RuntimeError.AudioUnreadable", format!("\"{name}\" has no audio stream ffprobe can read {}", String::from_utf8_lossy(&output.stderr).trim()));
    };
    Ok(Audio { name: name.to_string(), path, length })
}

fn index_value(target: &Value, index: &Value) -> Result<Value> {
    if let Value::Dict(entries) = target {
        let Value::Str(key) = index else {
            return err("TypeError.OperandType", format!("Dict key must be String, found {}", index.type_name()));
        };
        return entries
            .borrow()
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| MophError::new("ValueError.OutOfRange", format!("key {key:?} not found")));
    }
    let items: Vec<Value> = match target {
        Value::List(items) => items.borrow().clone(),
        Value::Tuple(items) => items.clone(),
        v => return err("TypeError.OperandType", format!("cannot index {}", v.type_name())),
    };
    if let Value::Range(a, b) = index {
        let n = items.len() as i64;
        let (a, b) = ((*a).clamp(0, n) as usize, (*b).clamp(0, n) as usize);
        return Ok(Value::List(Rc::new(RefCell::new(items[a.min(b)..b].to_vec()))));
    }
    let Value::Number(i) = index else {
        return err("TypeError.OperandType", format!("index must be Number, found {}", index.type_name()));
    };
    let n = items.len() as i64;
    let i = *i as i64;
    let at = if i < 0 { i + n } else { i };
    if at < 0 || at >= n {
        return err("ValueError.OutOfRange", format!("index {i} out of range for length {n}"));
    }
    Ok(items[at as usize].clone())
}

impl Interp {
    /// List / Range / Tuple のメソッド
    fn collection_method(&mut self, receiver: &Value, method: &str, args: Vec<(Option<String>, Value)>) -> Result<Value> {
    let list = |v: Vec<Value>| Value::List(Rc::new(RefCell::new(v)));
    if let Value::Str(text) = receiver {
        return match (method, args.as_slice()) {
            ("len", []) => Ok(Value::Number(text.chars().count() as f64)),
            ("replace", [(None, Value::Str(from)), (None, Value::Str(to))]) => Ok(Value::Str(text.replace(from.as_str(), to))),
            _ => err("NameError.UndefinedAttribute", format!("String has no method \"{method}\" with {} arguments", args.len())),
        };
    }
    if let Value::Dict(entries) = receiver {
        let entries = entries.borrow();
        return match (method, args.as_slice()) {
            ("keys", []) => Ok(list(entries.iter().map(|(k, _)| Value::Str(k.clone())).collect())),
            ("values", []) => Ok(list(entries.iter().map(|(_, v)| v.clone()).collect())),
            ("has", [(None, Value::Str(key))]) => Ok(Value::Bool(entries.iter().any(|(k, _)| k == key))),
            ("len", []) => Ok(Value::Number(entries.len() as f64)),
            _ => err("NameError.UndefinedAttribute", format!("Dict has no method \"{method}\" with {} arguments", args.len())),
        };
    }
    // 中身を複製しないで済むもの (長い List に何度も呼ぶ)
    if let Value::List(target) = receiver {
        match (method, args.as_slice()) {
            ("push", [(None, v)]) => {
                target.borrow_mut().push(v.clone());
                return Ok(Value::Nothing);
            }
            ("len", []) => return Ok(Value::Number(target.borrow().len() as f64)),
            _ => {}
        }
    }
    let items: Vec<Value> = match receiver {
        Value::List(items) => items.borrow().clone(),
        Value::Tuple(items) => items.clone(),
        Value::Range(a, b) => (*a..*b).map(|i| Value::Number(i as f64)).collect(),
        v => return err("NameError.UndefinedAttribute", format!("{} has no method \"{method}\"", v.type_name())),
    };
    match (method, args.as_slice()) {
        ("len", []) => Ok(Value::Number(items.len() as f64)),
        ("enumerate", []) => Ok(list(items.into_iter().enumerate().map(|(i, v)| Value::Tuple(vec![Value::Number(i as f64), v])).collect())),
        ("reverse", []) => Ok(list(items.into_iter().rev().collect())),
        ("to_list", []) => Ok(list(items)),
        ("push", [(None, v)]) => match receiver {
            Value::List(target) => {
                target.borrow_mut().push(v.clone());
                Ok(Value::Nothing)
            }
            _ => err("TypeError.ArgumentType", "push is a method of List"),
        },
        ("contains", [(None, v)]) => Ok(Value::Bool(items.iter().any(|x| equals(x, v)))),
        ("sum", []) => items.iter().try_fold(0.0, |acc, v| match v {
            Value::Number(n) => Ok(acc + n),
            v => err("TypeError.OperandType", format!("cannot sum {}", v.type_name())),
        }).map(Value::Number),
        ("index_of", [(None, v)]) => Ok(Value::Number(items.iter().position(|x| equals(x, v)).map_or(-1.0, |i| i as f64))),
        ("join", [(None, Value::Str(sep))]) => Ok(Value::Str(items.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(sep))),
        ("map", [(None, Value::Func(f))]) => {
            let out = items.into_iter().map(|v| self.apply(f, vec![(None, v)])).collect::<Result<Vec<_>>>()?;
            Ok(list(out))
        }
        ("filter", [(None, Value::Func(f))]) => {
            let mut out = vec![];
            for v in items {
                match self.apply(f, vec![(None, v.clone())])? {
                    Value::Bool(true) => out.push(v),
                    Value::Bool(false) => {}
                    other => return err("TypeError.ArgumentType", format!("filter expects Bool, found {}", other.type_name())),
                }
            }
            Ok(list(out))
        }
        ("reduce", [(None, init), (None, Value::Func(f))]) => {
            let mut acc = init.clone();
            for v in items {
                acc = self.apply(f, vec![(None, acc), (None, v)])?;
            }
            Ok(acc)
        }
        ("sort", []) => {
            let mut out = items;
            let mut failed = None;
            out.sort_by(|a, b| cmp(a, b).unwrap_or_else(|| {
                failed = Some(format!("cannot compare {} and {}", a.type_name(), b.type_name()));
                std::cmp::Ordering::Equal
            }));
            match failed {
                Some(msg) => err("TypeError.OperandType", msg),
                None => Ok(list(out)),
            }
        }
        ("zip", [(None, other)]) => {
            let other = match other {
                Value::List(o) => o.borrow().clone(),
                Value::Tuple(o) => o.clone(),
                v => return err("TypeError.ArgumentType", format!("zip expects List, found {}", v.type_name())),
            };
            Ok(list(items.into_iter().zip(other).map(|(a, b)| Value::Tuple(vec![a, b])).collect()))
        }
        ("steps", [(None, Value::Number(n))]) => match receiver {
            Value::Range(a, b) => {
                // 端を含めて n 等分。Range は end を含まないので、..= で作ったものは b-1 が終端
                let (a, b, n) = (*a as f64, (*b - 1) as f64, *n);
                let out = (0..=n as i64).map(|i| Value::Number(a + (b - a) * i as f64 / n)).collect();
                Ok(list(out))
            }
            _ => err("TypeError.ArgumentType", "steps is a method of Range"),
        },
        _ => err("NameError.UndefinedAttribute", format!("{} has no method \"{method}\" with {} arguments", receiver.type_name(), args.len())),
    }
    }
}

fn equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) | (Value::Duration(x), Value::Duration(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Str(x), Value::Str(y)) | (Value::Symbol(x), Value::Symbol(y)) => x == y,
        (Value::Tuple(x), Value::Tuple(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| equals(a, b)),
        (Value::Vector(x0, y0), Value::Vector(x1, y1)) => x0 == x1 && y0 == y1,
        (Value::Apos(a0, x0, y0), Value::Apos(a1, x1, y1)) => a0 == a1 && x0 == x1 && y0 == y1,
        // record は名前とフィールドが同じなら等しい
        (Value::Record(x), Value::Record(y)) => {
            x.name == y.name && x.fields.len() == y.fields.len() && x.fields.iter().zip(&y.fields).all(|((f0, v0), (f1, v1))| f0 == f1 && equals(v0, v1))
        }
        (Value::Color(x), Value::Color(y)) => x == y,
        (Value::Range(x0, y0), Value::Range(x1, y1)) => x0 == x1 && y0 == y1,
        // 入れ物は中身が同じなら等しい。Dict は並び順を見ない
        (Value::List(x), Value::List(y)) => Rc::ptr_eq(x, y) || {
            let (x, y) = (x.borrow(), y.borrow());
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| equals(a, b))
        },
        (Value::Dict(x), Value::Dict(y)) => Rc::ptr_eq(x, y) || {
            let (x, y) = (x.borrow(), y.borrow());
            x.len() == y.len() && x.iter().all(|(k, v)| y.iter().any(|(k2, v2)| k == k2 && equals(v, v2)))
        },
        // 型は同じ型なら等しい
        (Value::Type(x), Value::Type(y)) => Rc::ptr_eq(x, y),
        (Value::BuiltinType(x), Value::BuiltinType(y)) => x == y,
        (Value::Nothing, Value::Nothing) => true,
        // オブジェクトや Timeline は同一性 (同じ実体か)
        (Value::Object(x), Value::Object(y)) => Rc::ptr_eq(x, y),
        (Value::Timeline(x), Value::Timeline(y)) => Rc::ptr_eq(x, y),
        (Value::Motion(x), Value::Motion(y)) => Rc::ptr_eq(x, y),
        (Value::Func(x), Value::Func(y)) => Rc::ptr_eq(x, y),
        (Value::Audio(x), Value::Audio(y)) => Rc::ptr_eq(x, y),
        (Value::Module(x), Value::Module(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// Timeline が書き込む対象の一覧 (重複なし)
fn timeline_targets(tl: &Timeline) -> Vec<ObjRef> {
    let mut out: Vec<ObjRef> = Vec::new();
    for a in tl.keyframes.iter().flat_map(|k| &k.assigns) {
        if !out.iter().any(|o| Rc::ptr_eq(o, &a.target)) {
            out.push(a.target.clone());
        }
    }
    out
}

fn ease(name: Option<&str>, k: f64) -> f64 {
    match name {
        Some("ease_in") => k * k,
        Some("ease_out") => 1.0 - (1.0 - k) * (1.0 - k),
        Some("ease") => {
            if k < 0.5 { 2.0 * k * k } else { 1.0 - 2.0 * (1.0 - k) * (1.0 - k) }
        }
        _ => k,
    }
}

fn set_duration(slot: &Cell<Option<f64>>, v: Value) -> Result<()> {
    match v {
        Value::Duration(d) => {
            slot.set(Some(d));
            Ok(())
        }
        v => err("TypeError.AttributeType", format!("duration expects Duration, found {}", v.type_name())),
    }
}

/// View から辿れるオブジェクト (子、入れ子の View、Timeline の対象) を重複なく集める
fn collect_objects(obj: &ObjRef, out: &mut Vec<ObjRef>) {
    if out.iter().any(|o| Rc::ptr_eq(o, obj)) {
        return;
    }
    out.push(obj.clone());
    let o = obj.borrow();
    for child in &o.children {
        collect_objects(child, out);
    }
    for placed in &o.tracks {
        match &placed.track {
            Track::Audio(..) => {}
            Track::Container(c) | Track::Subtitle(c) => collect_objects(c, out),
            Track::Timeline(tl) => {
                for a in tl.keyframes.iter().flat_map(|k| &k.assigns) {
                    collect_objects(&a.target, out);
                }
            }
        }
    }
}

/// 属性パスの値を読む
fn get_path(obj: &ObjRef, path: &[String]) -> Option<Value> {
    let [attr, rest @ ..] = path else { return None };
    let mut v = obj.borrow().attrs.get(attr).cloned()?;
    for field in rest {
        v = match (v, field.as_str()) {
            (Value::Vector(x, _), "x") | (Value::Apos(_, x, _), "x") => Value::Number(x),
            (Value::Vector(_, y), "y") | (Value::Apos(_, _, y), "y") => Value::Number(y),
            (Value::Apos(_, x, y), "vector") => Value::Vector(x, y),
            (Value::Apos(a, _, _), "anchor") => Value::Symbol(a),
            _ => return None,
        };
    }
    Some(v)
}

/// エラーに出す引数の名前
fn pattern_text(p: &Pattern) -> String {
    match p {
        Pattern::Name(n) => n.clone(),
        Pattern::Tuple(items) => format!("({})", items.iter().map(pattern_text).collect::<Vec<_>>().join(", ")),
        Pattern::List(items) => format!("[{}]", items.iter().map(pattern_text).collect::<Vec<_>>().join(", ")),
    }
}

/// deepCopy 用。中の実体も新しく作る
fn deep_copy(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            let src = o.borrow();
            Value::Object(Rc::new(RefCell::new(Object {
                kind: src.kind.clone(),
                decl: src.decl.clone(),
                attrs: src.attrs.iter().map(|(k, v)| (k.clone(), deep_copy(v))).collect(),
                children: src.children.clone(),
                tracks: src.tracks.clone(),
            })))
        }
        Value::List(items) => Value::List(Rc::new(RefCell::new(items.borrow().iter().map(deep_copy).collect()))),
        Value::Dict(items) => Value::Dict(Rc::new(RefCell::new(items.borrow().iter().map(|(k, v)| (k.clone(), deep_copy(v))).collect()))),
        Value::Record(r) => Value::Record(Rc::new(Record {
            name: r.name.clone(),
            decl: r.decl.clone(),
            fields: r.fields.iter().map(|(f, v)| (f.clone(), deep_copy(v))).collect(),
        })),
        Value::Tuple(items) => Value::Tuple(items.iter().map(deep_copy).collect()),
        other => other.clone(),
    }
}

/// record のフィールドを読む
fn record_field(r: &Rc<Record>, field: &str) -> Result<Value> {
    r.fields
        .iter()
        .find(|(f, _)| f == field)
        .map(|(_, v)| v.clone())
        .ok_or_else(|| MophError::new("NameError.UndefinedAttribute", format!("{} has no field \"{field}\"", r.name)))
}

fn set_field(current: Value, path: &[String], value: Value) -> Result<Value> {
    let [field, rest @ ..] = path else { return Ok(value) };
    let number = |v: &Value| match v {
        Value::Number(n) => Ok(*n),
        v => err("TypeError.AttributeType", format!("{field} expects Number, found {}", v.type_name())),
    };
    match (current, field.as_str()) {
        (Value::Vector(x, y), "x") => Ok(Value::Vector(number(&set_field(Value::Number(x), rest, value)?)?, y)),
        (Value::Vector(x, y), "y") => Ok(Value::Vector(x, number(&set_field(Value::Number(y), rest, value)?)?)),
        (Value::Apos(a, x, y), "x") => Ok(Value::Apos(a, number(&set_field(Value::Number(x), rest, value)?)?, y)),
        (Value::Apos(a, x, y), "y") => Ok(Value::Apos(a, x, number(&set_field(Value::Number(y), rest, value)?)?)),
        (Value::Apos(a, x, y), "vector") => match set_field(Value::Vector(x, y), rest, value)? {
            Value::Vector(nx, ny) => Ok(Value::Apos(a, nx, ny)),
            v => err("TypeError.AttributeType", format!("vector expects Vector, found {}", v.type_name())),
        },
        (Value::Record(r), field) if r.fields.iter().any(|(f, _)| f == field) => {
            let fields = r
                .fields
                .iter()
                .map(|(f, v)| if f == field { set_field(v.clone(), rest, value.clone()).map(|nv| (f.clone(), nv)) } else { Ok((f.clone(), v.clone())) })
                .collect::<Result<Vec<_>>>()?;
            Ok(Value::Record(Rc::new(crate::lang::value::Record { name: r.name.clone(), decl: r.decl.clone(), fields })))
        }
        (Value::Apos(_, x, y), "anchor") => match set_field(Value::Symbol(String::new()), rest, value)? {
            Value::Symbol(s) => Ok(Value::Apos(s, x, y)),
            v => err("TypeError.AttributeType", format!("anchor expects Anchor, found {}", v.type_name())),
        },
        (v, _) => err("NameError.UndefinedAttribute", format!("{} has no field \"{field}\"", v.type_name())),
    }
}

/// 属性の型に値が合うか。組み込みの Union (Paint = Color | Shader) もここで見る
/// 組み込み型の名前 (補完用)
pub const KINDS: &[&str] = &["Circle", "Ellipse", "Rect", "Line", "Polygon", "Path", "TextArea", "View", "Timeline", "Subtitle", "Shader", "Gradient", "Color"];

/// 組み込み型の属性と型
/// 型の名前は大文字で始まり、組み込みの型と union の名前は使えない
fn check_free_name(name: &str) -> Result<()> {
    if !name.starts_with(char::is_uppercase) {
        return err("NameError.Reserved", format!("a type name must start with an uppercase letter; \"{name}\" does not"));
    }
    if crate::docs::TYPES.iter().any(|t| t.name == name) {
        return err("NameError.Reserved", format!("\"{name}\" is a builtin type and cannot be redeclared"));
    }
    Ok(())
}

pub fn schema(kind: &str) -> Option<&'static [(&'static str, &'static str)]> {
    const SHAPE: [(&str, &str); 10] = [
        ("fill", "Paint"),
        ("stroke", "Color"),
        ("strokeWidth", "Number"),
        ("strokeCap", "StrokeCap"),
        ("strokeJoin", "StrokeJoin"),
        ("dash", "List"),
        ("dashOffset", "Number"),
        ("opacity", "Number"),
        ("rotation", "Number"),
        ("blend", "Blend"),
    ];
    macro_rules! with_shape {
        ($($extra:expr),*) => {{
            const ATTRS: &[(&str, &str)] = &[$($extra,)* SHAPE[0], SHAPE[1], SHAPE[2], SHAPE[3], SHAPE[4], SHAPE[5], SHAPE[6], SHAPE[7], SHAPE[8], SHAPE[9]];
            ATTRS
        }};
    }
    Some(match kind {
        "Circle" => with_shape!(("position", "Pos"), ("radius", "Number")),
        "Ellipse" => with_shape!(("position", "Pos"), ("rx", "Number"), ("ry", "Number")),
        "Rect" => with_shape!(("position", "Pos"), ("w", "Number"), ("h", "Number"), ("radius", "Number")),
        "Line" => with_shape!(("from", "Vector"), ("to", "Vector")),
        "Polygon" => with_shape!(("points", "List")),
        "Path" => with_shape!(("from", "Vector"), ("segments", "List"), ("closed", "Bool")),
        "TextArea" => with_shape!(("position", "Pos"), ("text", "String"), ("w", "Number"), ("font", "String"), ("fontSize", "Number"), ("align", "Align")),
        "View" => &[("box", "Vector"), ("position", "Pos"), ("w", "Number"), ("h", "Number"), ("opacity", "Number"), ("blend", "Blend")],
        "Timeline" => &[("duration", "Duration")],
        "Subtitle" => &[("text", "String"), ("duration", "Duration")],
        "Shader" => &[("color", "Func"), ("args", "List"), ("samples", "Number")],
        "Gradient" => &[("kind", "GradientKind"), ("from", "Vector"), ("to", "Vector"), ("radius", "Number"), ("stops", "List")],
        _ => return None,
    })
}

fn interpolate(a: &Value, b: &Value, k: f64) -> Value {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Value::Number(x + (y - x) * k),
        (Value::Duration(x), Value::Duration(y)) => Value::Duration(x + (y - x) * k),
        (Value::Vector(x0, y0), Value::Vector(x1, y1)) => Value::Vector(x0 + (x1 - x0) * k, y0 + (y1 - y0) * k),
        (Value::Apos(an, x0, y0), Value::Apos(_, x1, y1)) => Value::Apos(an.clone(), x0 + (x1 - x0) * k, y0 + (y1 - y0) * k),
        (Value::Color(c0), Value::Color(c1)) => {
            let k = k as f32;
            Value::Color(std::array::from_fn(|i| c0[i] + (c1[i] - c0[i]) * k))
        }
        _ => a.clone(),
    }
}


fn binary(op: BinOp, l: Value, r: Value) -> Result<Value> {
    use Value::{Bool, Duration, Number};
    // Vector は実数 2 つの Tuple と同じに計算し、結果を Vector に戻す (Vector ± Vector / (x, y)、Vector × ÷ Number)
    let is_vector = |v: &Value| matches!(v, Value::Vector(..));
    if (is_vector(&l) || is_vector(&r)) && matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) {
        let as_tuple = |v: &Value| match v {
            Value::Vector(x, y) => Value::Tuple(vec![Number(*x), Number(*y)]),
            other => other.clone(),
        };
        let result = binary(op, as_tuple(&l), as_tuple(&r)).map_err(|e| match e.kind {
            "TypeError.OperandType" => MophError::new("TypeError.OperandType", format!("cannot {} {} and {}", verb(op), l.type_name(), r.type_name())),
            _ => e,
        })?;
        return Ok(match result {
            Value::Tuple(items) => match items.as_slice() {
                [Number(x), Number(y)] => Value::Vector(*x, *y),
                _ => Value::Tuple(items),
            },
            other => other,
        });
    }
    let num = |f: fn(f64, f64) -> f64| -> Option<Value> {
        match (&l, &r) {
            (Number(a), Number(b)) => Some(Number(f(*a, *b))),
            _ => None,
        }
    };
    let result = match op {
        BinOp::Add => match (&l, &r) {
            (Duration(a), Duration(b)) => Some(Duration(a + b)),
            (Value::Tuple(a), Value::Tuple(b)) => zip_tuples(a, b, BinOp::Add)?,
            (Value::Color(a), Value::Color(b)) => check_color(Some(Value::Color(std::array::from_fn(|i| a[i] + b[i]))))?,
            (Value::List(a), Value::List(b)) => {
                let mut out = a.borrow().clone();
                out.extend(b.borrow().iter().cloned());
                Some(Value::List(Rc::new(RefCell::new(out))))
            }
            (Value::Str(a), Value::Str(b)) => Some(Value::Str(format!("{a}{b}"))),
            _ => num(|a, b| a + b),
        },
        BinOp::Sub => match (&l, &r) {
            (Duration(a), Duration(b)) => Some(Duration(a - b)),
            (Value::Tuple(a), Value::Tuple(b)) => zip_tuples(a, b, BinOp::Sub)?,
            _ => num(|a, b| a - b),
        },
        BinOp::Mul => match (&l, &r) {
            (Duration(a), Number(b)) | (Number(b), Duration(a)) => Some(Duration(a * b)),
            (Value::Tuple(items), Number(k)) | (Number(k), Value::Tuple(items)) => scale_tuple(items, *k, BinOp::Mul)?,
            (Value::Color(c), Number(k)) | (Number(k), Value::Color(c)) => {
                let k = *k as f32;
                check_color(Some(Value::Color(std::array::from_fn(|i| c[i] * k))))?
            }
            _ => num(|a, b| a * b),
        },
        BinOp::Div => match (&l, &r) {
            (_, Number(b)) | (_, Duration(b)) if *b == 0.0 => return err("RuntimeError.DivisionByZero", "division by zero"),
            (Duration(a), Number(b)) => Some(Duration(a / b)),
            (Duration(a), Duration(b)) => Some(Number(a / b)),
            (Value::Tuple(items), Number(k)) => scale_tuple(items, *k, BinOp::Div)?,
            _ => num(|a, b| a / b),
        },
        BinOp::Rem => match (&l, &r) {
            (_, Number(b)) if *b == 0.0 => return err("RuntimeError.DivisionByZero", "division by zero"),
            _ => num(|a, b| a % b),
        },
        BinOp::Pow => num(f64::powf),
        BinOp::Range | BinOp::RangeInclusive => match (&l, &r) {
            (Number(a), Number(b)) => Some(Value::Range(*a as i64, *b as i64 + if op == BinOp::RangeInclusive { 1 } else { 0 })),
            _ => None,
        },
        BinOp::Lt => cmp(&l, &r).map(|o| Bool(o.is_lt())),
        BinOp::Le => cmp(&l, &r).map(|o| Bool(o.is_le())),
        BinOp::Gt => cmp(&l, &r).map(|o| Bool(o.is_gt())),
        BinOp::Ge => cmp(&l, &r).map(|o| Bool(o.is_ge())),
        BinOp::Eq => Some(Bool(equals(&l, &r))),
        BinOp::Ne => Some(Bool(!equals(&l, &r))),
        BinOp::And | BinOp::Or => unreachable!("handled in eval"),
    };
    result.ok_or_else(|| MophError::new("TypeError.OperandType", format!("cannot {} {} and {}", verb(op), l.type_name(), r.type_name())))
}

pub fn verb(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "add",
        BinOp::Sub => "subtract",
        BinOp::Mul => "multiply",
        BinOp::Div => "divide",
        BinOp::Rem => "take remainder of",
        BinOp::Pow => "raise",
        BinOp::Range | BinOp::RangeInclusive => "make a range of",
        _ => "compare",
    }
}

/// Tuple 同士の要素ごとの演算。長さが違えばエラー
fn zip_tuples(a: &[Value], b: &[Value], op: BinOp) -> Result<Option<Value>> {
    if a.len() != b.len() {
        return err("TypeError.ArityMismatch", format!("tuples have different lengths: {} and {}", a.len(), b.len()));
    }
    let items = a.iter().zip(b).map(|(x, y)| binary(op, x.clone(), y.clone())).collect::<Result<Vec<_>>>()?;
    Ok(Some(Value::Tuple(items)))
}

fn scale_tuple(items: &[Value], k: f64, op: BinOp) -> Result<Option<Value>> {
    let items = items.iter().map(|x| binary(op, x.clone(), Value::Number(k))).collect::<Result<Vec<_>>>()?;
    Ok(Some(Value::Tuple(items)))
}

/// Color の各チャンネルが 0..1 に収まっているか
fn check_color(v: Option<Value>) -> Result<Option<Value>> {
    if let Some(Value::Color(c)) = &v {
        if c.iter().any(|x| !(0.0..=1.0).contains(x)) {
            return err("ValueError.OutOfRange", "color channel exceeds 1.0");
        }
    }
    Ok(v)
}

fn cmp(l: &Value, r: &Value) -> Option<std::cmp::Ordering> {
    match (l, r) {
        (Value::Number(a), Value::Number(b)) | (Value::Duration(a), Value::Duration(b)) => a.partial_cmp(b),
        _ => None,
    }
}
