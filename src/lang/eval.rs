use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use crate::lang::ast::{Arg, BinOp, DictKey, Expr, ImportKind, ImportSource, Pattern, ProgressDef, Stmt, StmtKind, TypeAnn};
use crate::lang::error::{Kind, MophError, Result, err};
use crate::lang::value::{Audio, Closure, Layer, Module, ObjRef, Object, ProgressTable, Ratio, Record, Scopes, UserType, Value, next_object_id, new_scope};
use crate::stdlib;

/// 文の実行結果。return で関数を抜けるときに伝える
enum Flow {
    Next(Value),
    Return(Value),
    /// for を抜ける。for が受け止めるので、関数の外には出ない
    Break,
}

impl Flow {
    fn value(self) -> Value {
        match self {
            Flow::Next(v) | Flow::Return(v) => v,
            Flow::Break => Value::Nothing,
        }
    }
}

pub struct Interp {
    scopes: Scopes,
    pub output: Option<ObjRef>,
    /// type Name = A | B の定義
    types: HashMap<String, Vec<TypeAnn>>,
    /// フォント検索、テキストレイアウト、描画命令のキャッシュ。初回に必要になったときに作る
    cache: Option<crate::render::text::RenderCache>,
    /// 実行中のファイルのディレクトリ。import "file" の相対パスの基準
    pub base_dir: PathBuf,
    /// 実行中のファイルが export した名前
    exports: Vec<String>,
    /// 埋め込みバイナリのとき、import はディスクではなくここから読む (正規化したパス → 本文)
    pub sources: HashMap<String, String>,
    /// 埋め込みバイナリのとき、音声などのファイルの取り出し先 (正規化したパス → 実際のファイル)
    pub assets: HashMap<String, PathBuf>,
    /// 読み込み済みのモジュールと音声 (正規化したパス → 実体)。同じファイルは 1 度しか読まない
    modules: HashMap<String, Value>,
    /// 読み込み中のパス。循環 import の検出用
    loading: Vec<String>,
    /// 式の中 (if のブロックなど) で return した値。外側の文の列がこれを見て関数を抜ける
    returning: Option<Value>,
    /// if の枝の中で break したことを、外側の文の列に伝える
    breaking: bool,
    /// いま func new を実行中の型。中で型名を呼んだらフィールドから作る
    constructing: Vec<String>,
    /// @deprecated の警告を出した型。1 つにつき 1 度だけ出す
    warned: std::collections::HashSet<String>,
    /// いま本体を実行している型。private が見えるのはこの中だけ
    inside: Vec<String>,
    /// 次の apply がどの型のメンバーか。apply がこれを inside に積む
    calling: Option<String>,
    /// mophila.yaml。@ で始まる import と `import config` に使う
    project: Option<Rc<crate::project::Project>>,
}

impl Interp {
    pub fn new() -> Self {
        // union がまとめる型と、決まった Symbol しか取らない型は docs::TYPES が持っている
        let mut types: HashMap<String, Vec<TypeAnn>> = HashMap::new();
        for t in crate::docs::types() {
            if !t.members.is_empty() {
                types.insert(t.name.to_string(), t.members.iter().map(|m| TypeAnn::plain(m)).collect());
            } else if !t.values.is_empty() {
                types.insert(t.name.to_string(), t.values.iter().map(|(v, _)| TypeAnn::plain(v)).collect());
            }
        }
        Self { scopes: vec![root_scope()], output: None, types, cache: None, base_dir: PathBuf::from("."), exports: Vec::new(), sources: HashMap::new(), assets: HashMap::new(), modules: HashMap::new(), loading: Vec::new(), returning: None, breaking: false, constructing: Vec::new(), warned: std::collections::HashSet::new(), inside: Vec::new(), calling: None, project: None }
    }

    /// mophila.yaml を渡す。@ の解決と `import config` がこれを見る
    pub fn set_project(&mut self, project: Rc<crate::project::Project>) {
        self.project = Some(project);
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
            Flow::Return(_) => err(Kind::UnexpectedToken, "return outside of a function"),
            Flow::Break => err(Kind::UnexpectedToken, "break outside of a for"),
        }
    }

    fn run_block(&mut self, stmts: &[Stmt]) -> Result<Flow> {
        let mut last = Value::Nothing;
        for stmt in stmts {
            let flow = self.exec(stmt)?;
            if let Some(v) = self.returning.take() {
                return Ok(Flow::Return(v));
            }
            if self.breaking {
                return Ok(Flow::Break);
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
                let v = self.eval(e)?;
                let v = match ann {
                    Some(ann) => {
                        let v = self.coerced(v, ann);
                        self.check_ann(&v, ann, "value")?;
                        v
                    }
                    None => v,
                };
                self.bind(pat, v)?;
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::TypeDef(name, members) => {
                self.top_level("type")?;
                check_free_name(name)?;
                self.types.insert(name.clone(), members.clone());
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::Import(ImportKind::Module { source, alias }) => {
                self.top_level("import")?;
                let module = self.load_module(source)?;
                self.scopes.first().expect("global scope").borrow_mut().insert(alias.clone(), module);
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::Import(ImportKind::Names { source, names }) => {
                self.top_level("import")?;
                let Value::Module(module) = self.load_module(source)? else {
                    return err(Kind::ArgumentType, "import { ... } from needs a module, not a file asset");
                };
                for name in names {
                    let Some(v) = module.items.get(name) else {
                        return err(Kind::UndefinedAttribute, format!("module {} does not export \"{name}\"", module.name));
                    };
                    self.scopes.first().expect("global scope").borrow_mut().insert(name.clone(), v.clone());
                }
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::Export(inner) => {
                if self.scopes.len() != 1 {
                    return err(Kind::UnexpectedToken, "export is only allowed at the top level");
                }
                let result = self.exec(inner)?;
                match &inner.kind {
                    StmtKind::Let(pat, ..) => collect_names(pat, &mut self.exports),
                    StmtKind::TypeDecl(decl) => self.exports.push(decl.name.clone()),
                    // export { a, b } from mod — 取り込んだ名前をそのまま外に出す
                    StmtKind::Import(ImportKind::Names { names, .. }) => self.exports.extend(names.iter().cloned()),
                    _ => {}
                }
                Ok(result)
            }
            StmtKind::TypeDecl(decl) => {
                if self.scopes.len() != 1 {
                    return err(Kind::UnexpectedToken, "struct and record are only allowed at the top level");
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
                    (Value::List(items), Value::Number(i, _)) => {
                        let mut items = items.borrow_mut();
                        let n = items.len() as i64;
                        let i = whole(*i, "an index")?;
                        let at = if i < 0 { i + n } else { i };
                        if at < 0 || at >= n {
                            return err(Kind::OutOfRange, format!("index {i} out of range for length {n}"));
                        }
                        items[at as usize] = v;
                    }
                    _ => return err(Kind::OperandType, format!("cannot assign into {} with {} index", target.type_name(), index.type_name())),
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
                err(Kind::UnexpectedToken, "a file can have only one output")
            }
            StmtKind::Output(e) => {
                let comp = self.eval_object(e)?;
                if comp.borrow().kind != "Composition" {
                    return err(Kind::ArgumentType, format!("output takes a Composition, found {}", comp.borrow().kind));
                }
                self.output = Some(comp);
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::For(pat, iter, body) => {
                for item in self.iterate(iter)? {
                    self.scopes.push(new_scope());
                    let result = self.bind(pat, item).and_then(|()| self.run_block(body));
                    self.scopes.pop();
                    match result? {
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Break => {
                            self.breaking = false;
                            break;
                        }
                        Flow::Next(_) => {}
                    }
                }
                Ok(Flow::Next(Value::Nothing))
            }
            StmtKind::While(cond, body) => {
                // 止まらない while を見つけるための上限。真面目な探索がこの回数を要ることは無い
                for _ in 0..LOOP_LIMIT {
                    if !self.eval_bool(cond)? {
                        return Ok(Flow::Next(Value::Nothing));
                    }
                    self.scopes.push(new_scope());
                    let result = self.run_block(body);
                    self.scopes.pop();
                    match result? {
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Break => {
                            self.breaking = false;
                            return Ok(Flow::Next(Value::Nothing));
                        }
                        Flow::Next(_) => {}
                    }
                }
                err(Kind::EndlessLoop, format!("this while ran {LOOP_LIMIT} times without its condition becoming false"))
            }
            StmtKind::Break => Ok(Flow::Break),
            StmtKind::Return(e) => Ok(Flow::Return(self.eval(e)?)),
            StmtKind::Expr(e) => Ok(Flow::Next(self.eval(e)?)),
        }
    }

    /// 別名なら、指している型の名前に直す。別名でなければ借りたまま返す (毎フレーム通るので確保しない)
    fn real_type_name<'a>(&self, name: &'a str) -> std::borrow::Cow<'a, str> {
        match self.scopes.iter().rev().find_map(|s| s.borrow().get(name).cloned()) {
            Some(Value::Type(t)) => std::borrow::Cow::Owned(t.decl.name.clone()),
            Some(Value::BuiltinType(n)) => std::borrow::Cow::Owned(n),
            _ => std::borrow::Cow::Borrowed(name),
        }
    }

    /// 文がいちばん外側にあるか。import と type は中に書けない
    fn top_level(&self, what: &str) -> Result<()> {
        match self.scopes.len() {
            1 => Ok(()),
            _ => err(Kind::UnexpectedToken, format!("{what} is only allowed at the top level")),
        }
    }

    /// その名前が型として存在するか。書き間違いをここで止める
    fn known_type(&self, name: &str) -> bool {
        // 関数の型と、決まった Symbol は名前で引かない
        name.contains("->")
            || name.starts_with(':')
            || name == "Func"
            || self.types.contains_key(name)
            || crate::docs::types().iter().any(|t| t.name == name)
            || matches!(self.scopes.iter().rev().find_map(|s| s.borrow().get(name).cloned()), Some(Value::Type(_) | Value::BuiltinType(_)))
    }

    /// フィールドに入れる値。エラーには型名とフィールド名を出す
    fn check_field(&self, owner: &str, field: &str, v: &Value, ann: &TypeAnn) -> Result<()> {
        self.check_ann(v, ann, &format!("{owner}.{field}"))
    }

    /// 値が型名に合うか。Union は定義をたどり、< > を書いてあれば要素まで見る
    fn check_ann(&self, v: &Value, ann: &TypeAnn, what: &str) -> Result<()> {
        let head = self.real_type_name(&ann.name);
        if !self.known_type(&head) {
            return err(Kind::UndefinedVariable, format!("{what}: \"{head}\" is not a type"));
        }
        if self.matches_ann(v, ann) {
            return Ok(());
        }
        // 入れ物は合っていて中身が違うなら、違う要素を指す ("found List" では直せない)
        if let ([want], Some(bad)) = (&ann.args[..], self.first_mismatch(v, ann)) {
            return err(Kind::AttributeType, format!("{what} expects {}, found {} in it (expected {})", ann.text, bad.type_name(), want.text));
        }
        Err(self.wrong_type(Kind::AttributeType, what, &ann.text, v))
    }

    /// schema に書いた型に合わせて Tuple を作り直す。
    /// `position = (8, 4.5)` や `points = [(0, 0), (1, 0)]` と書けるのはここ
    fn fitted_attr(&self, ty: &'static str, v: Value) -> Value {
        match schema_anns().get(ty) {
            Some(ann) => self.coerced(v, ann),
            // < > の無い型。毎フレーム通るので、Tuple のときだけ作り直しを試す
            None => match &v {
                Value::Tuple(cells) => crate::lang::method::value_from(&self.real_type_name(ty), cells).unwrap_or(v),
                _ => v,
            },
        }
    }

    /// schema に書いた型に合うか
    fn attr_ok(&self, ty: &'static str, v: &Value) -> bool {
        match schema_anns().get(ty) {
            Some(ann) => self.matches_ann(v, ann),
            None => self.matches_type(v, &self.real_type_name(ty)),
        }
    }

    /// 書いた型に合わせて、Tuple を名前付きの型にする。
    /// 変換するのは Tuple からだけで、型どうしの変換は入れない (間違いが黙って通る)。
    /// 変えるものが無ければ元の値をそのまま返す (List の実体を差し替えない)
    fn coerced(&self, v: Value, ann: &TypeAnn) -> Value {
        self.converted(&v, ann).unwrap_or(v)
    }

    /// 作り直したものだけを返す。変えるものが無ければ None (入れ物の実体を差し替えない)
    fn converted(&self, v: &Value, ann: &TypeAnn) -> Option<Value> {
        // 別名 (type Points = List<Vector3>) は、指している型で見る
        if ann.args.is_empty() {
            if let Some([only]) = self.types.get(&*self.real_type_name(&ann.name)).map(|m| &m[..]) {
                return self.converted(v, &only.clone());
            }
        }
        if let Value::Tuple(cells) = v {
            if let Ok(made) = crate::lang::method::value_from(&self.real_type_name(&ann.name), cells) {
                return Some(made);
            }
        }
        if ann.args.is_empty() {
            return None;
        }
        // 1 つだけ書いてあれば全部その型、並べて書いてあれば 1 つずつ
        let made = |xs: &[Value]| -> Option<Vec<Value>> {
            let at = |i: usize| match &ann.args[..] {
                [inner] => Some(inner),
                args => args.get(i),
            };
            let out: Vec<Option<Value>> = xs.iter().enumerate().map(|(i, x)| at(i).and_then(|a| self.converted(x, a))).collect();
            out.iter()
                .any(Option::is_some)
                .then(|| out.into_iter().zip(xs).map(|(made, x)| made.unwrap_or_else(|| x.clone())).collect())
        };
        match v {
            Value::List(xs) => made(&xs.borrow()).map(|xs| Value::List(Rc::new(RefCell::new(xs)))),
            Value::Tuple(xs) => made(xs).map(Value::Tuple),
            // Dict はキーを触らず値だけ。キーは必ず String
            Value::Dict(items) => {
                let want = match &ann.args[..] {
                    [v] => v,
                    [_, v] => v,
                    _ => return None,
                };
                let items = items.borrow();
                let out: Vec<Option<Value>> = items.iter().map(|(_, x)| self.converted(x, want)).collect();
                out.iter().any(Option::is_some).then(|| {
                    let pairs = out.into_iter().zip(items.iter()).map(|(made, (k, x))| (k.clone(), made.unwrap_or_else(|| x.clone()))).collect();
                    Value::Dict(Rc::new(RefCell::new(pairs)))
                })
            }
            _ => None,
        }
    }

    /// 並びの中で、最初に型が合わない要素
    fn first_mismatch(&self, v: &Value, ann: &TypeAnn) -> Option<Value> {
        let [want] = &ann.args[..] else { return None };
        if !self.matches_type(v, &self.real_type_name(&ann.name)) {
            return None;
        }
        let bad = |xs: &[Value]| xs.iter().find(|x| !self.matches_ann(x, want)).cloned();
        match v {
            Value::List(xs) => bad(&xs.borrow()),
            Value::Tuple(xs) => bad(xs),
            Value::Dict(items) => items.borrow().iter().map(|(_, x)| x).find(|x| !self.matches_ann(x, want)).cloned(),
            _ => None,
        }
    }

    /// 型が合わないときのエラー。決まった値しか取らない型なら、取れる値を並べる
    fn wrong_type(&self, kind: Kind, what: &str, expected: &str, v: &Value) -> MophError {
        // 名前は同じでも宣言が違うとき
        if v.type_name() == self.real_type_name(expected) {
            return MophError::new(kind, format!("{what} expects the {expected} declared here, but this one was declared somewhere else"));
        }
        match self.types.get(expected) {
            Some(values) if values.iter().all(|m| m.name.starts_with(':')) => {
                let names = values.iter().map(|m| m.text.clone()).collect::<Vec<_>>().join(" | ");
                MophError::new(Kind::OutOfRange, format!("{what} is one of {names}, found {v}"))
            }
            _ => MophError::new(kind, format!("{what} expects {expected}, found {}", v.type_name())),
        }
    }

    /// struct の属性に値を書く。型は宣言から引く。図形などの組み込みの値は不変で、書けない
    pub(crate) fn set_attr(&self, obj: &ObjRef, attr: &str, value: Value) -> Result<()> {
        let mut o = obj.borrow_mut();
        let Some(ty) = &o.decl else {
            return err(Kind::AttributeType, format!("{} is immutable; make a changed one with copy({attr} = ...)", o.kind));
        };
        let Some(f) = ty.decl.fields.iter().find(|f| f.name == attr) else {
            return err(Kind::UndefinedAttribute, format!("{} has no attribute \"{attr}\"", o.kind));
        };
        // 型に合わせて Tuple を作り直してから確かめる。合わなければ、どこが合わないかを出す
        let value = self.coerced(value, &f.ann);
        if !self.matches_ann(&value, &f.ann) {
            let (kind, expected) = (o.kind.clone(), f.ann.text.clone());
            drop(o);
            return Err(self.wrong_type(Kind::AttributeType, &format!("{kind}.{attr}"), &expected, &value));
        }
        // 既にある属性は入れ替えるだけ。名前を作り直さない
        match o.attrs.get_mut(attr) {
            Some(slot) => *slot = value,
            None => {
                o.attrs.insert(attr.to_string(), value);
            }
        }
        Ok(())
    }

    fn matches_type(&self, v: &Value, name: &str) -> bool {
        // ":center" のような要素は、その Symbol そのもの
        if let Some(sym) = name.strip_prefix(':') {
            return matches!(v, Value::Symbol(s) if s == sym);
        }
        // 関数の型。中身までは見ない
        if name.contains("->") {
            return matches!(v, Value::Func(_) | Value::Builtin(_));
        }
        // 長さの属性は、単位なしの数 (箱の単位) も取る
        if name == "Length" {
            return matches!(v, Value::Number(..) | Value::Length(_));
        }
        // 型の名前を作らずに比べる。Object と Record だけは実体に名前が入っている
        let actual: std::borrow::Cow<str> = match v.type_name_ref() {
            Some(n) => std::borrow::Cow::Borrowed(n),
            None => std::borrow::Cow::Owned(v.type_name()),
        };
        if actual == name {
            // 名前が同じでも、別の宣言なら別の型。import した先の同名の型と混ざらない
            return match (self.declared(v), self.lookup_type(name)) {
                (Some(a), Some(b)) => Rc::ptr_eq(&a, &b),
                (Some(_), None) => false,
                _ => true,
            };
        }
        self.types.get(name).is_some_and(|members| members.iter().any(|m| self.matches_ann(v, m)))
    }

    /// 注釈に合うか。`List<Vector>` のように < > を書いてあれば、要素まで見る
    fn matches_ann(&self, v: &Value, ann: &TypeAnn) -> bool {
        if !self.matches_type(v, &self.real_type_name(&ann.name)) {
            return false;
        }
        ann.args.is_empty() || self.matches_args(v, &ann.args)
    }

    /// < > の中を見る。要素の数だけ検査が増えるので、書いたときだけ走る
    fn matches_args(&self, v: &Value, args: &[TypeAnn]) -> bool {
        let every = |xs: &[Value]| args.len() == 1 && xs.iter().all(|x| self.matches_ann(x, &args[0]));
        match v {
            Value::List(xs) => every(&xs.borrow()),
            Value::Array(_) => args.len() == 1 && args[0].name == "Number",
            // Tuple は 1 つ書いたら全部その型、並べて書いたら 1 つずつ
            Value::Tuple(xs) => match args.len() {
                1 => every(xs),
                n => xs.len() == n && xs.iter().zip(args).all(|(x, a)| self.matches_ann(x, a)),
            },
            // Dict のキーは必ず String なので、Dict<V> とも Dict<String, V> とも書ける
            Value::Dict(items) => {
                let value_type = match args {
                    [v] => v,
                    [k, v] if k.name == "String" => v,
                    _ => return false,
                };
                items.borrow().iter().all(|(_, x)| self.matches_ann(x, value_type))
            }
            Value::Range(..) => args.len() == 1 && args[0].name == "Number",
            // 中身を見ない型 (Func など) は、頭の名前が合っていればよい
            _ => true,
        }
    }

    /// font に書いた候補から、この機械にある最初の名前を選ぶ。1 つも無ければエラー。
    /// 選んだ後は 1 つの名前になるので、読み返すとどれが使われたか分かる
    fn pick_font(&mut self, v: &Value) -> Result<Value> {
        let names = font_names(v);
        if names.is_empty() {
            return Ok(v.clone());
        }
        if let Some(found) = self.cache_mut().first_family(&names) {
            return Ok(Value::Str(found));
        }
        // 何が使えるか分からないと直せないので、近い名前を並べる
        let near = self.cache_mut().nearest(&names[0]);
        let hint = match near.is_empty() {
            true => "; \"mophila fonts\" lists the ones this machine has".to_string(),
            false => format!("; did you mean {}? (\"mophila fonts\" lists them all)", near.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(", ")),
        };
        let wanted = names.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(", ");
        match names.len() {
            1 => err(Kind::FontNotFound, format!("font {wanted} not found{hint}")),
            _ => err(Kind::FontNotFound, format!("none of the fonts {wanted} are on this machine{hint}")),
        }
    }

    /// その値が struct / record なら、その宣言
    fn declared(&self, v: &Value) -> Option<Rc<UserType>> {
        match v {
            Value::Record(r) => r.decl.clone(),
            Value::Object(o) => o.borrow().decl.clone(),
            _ => None,
        }
    }

    /// 名前だけの import は標準ライブラリ (本体に入っているもの)、. や "" で始まるものはファイル
    fn load_module(&mut self, source: &ImportSource) -> Result<Value> {
        match source {
            // import config — mophila.yaml の config: をモジュールとして読む
            ImportSource::Std(name) if name == "config" => {
                let Some(p) = &self.project else {
                    return err(Kind::UndefinedVariable, "\"import config\" needs a mophila.yaml");
                };
                let items = p.config.iter().map(|(k, v)| (k.clone(), v.to_value())).collect();
                Ok(Value::Module(Rc::new(Module { name: "config".into(), items })))
            }
            ImportSource::Std(name) => match stdlib::find(name) {
                Some(stdlib::Lib::Native(module)) => Ok(Value::Module(Rc::new(module))),
                // 標準ライブラリの中の相対 import が解けるように、埋め込んだ置き場を基準にする
                Some(stdlib::Lib::Script(path, src)) => {
                    let full = PathBuf::from(stdlib::ROOT).join(path);
                    let dir = full.parent().map(|d| d.to_path_buf()).unwrap_or_default();
                    self.run_module(crate::bundle::normalize(&full), name, src, dir)
                }
                None => err(Kind::UndefinedVariable, format!("no module named \"{name}\"")),
            },
            ImportSource::File(path) => self.import_file(path),
        }
    }

    /// .moph ならファイルを別のスコープで実行し、export した束縛と output した View をモジュールにする。
    /// それ以外は音声ファイルとして読む
    fn import_file(&mut self, path: &str) -> Result<Value> {
        // @ で始まるものは mophila.yaml の root / aliases から引く。設定の中のパスは設定ファイルからの相対
        let full = match path.starts_with('@') {
            false => self.base_dir.join(path),
            true => match &self.project {
                Some(p) => p.resolve(path).map_err(|e| MophError::new(Kind::UndefinedVariable, e.to_string()))?,
                None => {
                    return err(Kind::UndefinedVariable, format!("reading \"{path}\" needs a mophila.yaml with that alias"));
                }
            },
        };
        let key = crate::bundle::normalize(&full);
        if let Some(m) = self.modules.get(&key) {
            return Ok(m.clone());
        }
        if !path.ends_with(".moph") {
            let real = self.assets.get(&key).cloned().unwrap_or(full);
            // 拡張子で音声か画像かを決める
            let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            let asset = match matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff") {
                true => Value::Image(Rc::new(load_image(path, real)?)),
                false => Value::Audio(Rc::new(load_audio(path, real)?)),
            };
            self.modules.insert(key, asset.clone());
            return Ok(asset);
        }
        let src = match self.sources.get(&key) {
            Some(src) => src.clone(),
            // 標準ライブラリのファイルは実行ファイルの中にあり、ディスクには無い
            None => match stdlib::embedded(&key) {
                Some(src) => src.to_string(),
                None => std::fs::read_to_string(&full)
                    .map_err(|e| MophError::new(Kind::UndefinedVariable, format!("cannot read \"{}\": {e}", full.display())))?,
            },
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
            return err(Kind::UndefinedVariable, format!("circular import of \"{label}\""));
        }
        self.loading.push(key.clone());
        let stmts = crate::lang::parser::parse(src)?;
        let saved_scopes = std::mem::replace(&mut self.scopes, vec![root_scope()]);
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
            return err(Kind::AssignWithoutLet, format!("\"{name}\" is not defined; use \"let {name} = ...\""));
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
            Expr::Attr(obj, attr) => {
                let target = self.eval(obj)?;
                let updated = self.write_path(target.clone(), std::slice::from_ref(attr), v)?;
                // 値型は作り直しになるので、その値を持っている入れ物を更新する
                match same_place(&target, &updated) {
                    true => Ok(()),
                    false => self.assign(obj, updated),
                }
            }
            _ => err(Kind::UnexpectedToken, "cannot assign to this expression"),
        }
    }

    fn iterate(&mut self, e: &Expr) -> Result<Vec<Value>> {
        match self.eval(e)? {
            Value::List(items) => Ok(items.borrow().clone()),
            Value::Tuple(items) => Ok(items),
            Value::Array(a) => Ok(a.nums.iter().map(|x| Value::num(*x)).collect()),
            Value::Range(a, b) => Ok((a..b).map(|i| Value::num(i as f64)).collect()),
            Value::Dict(entries) => Ok(entries.borrow().iter().map(|(k, v)| Value::Tuple(vec![Value::Str(k.clone()), v.clone()])).collect()),
            v => err(Kind::ArgumentType, format!("cannot iterate over {}", v.type_name())),
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
                    return err(Kind::ArgumentType, format!("cannot destructure {} into a list", value.type_name()));
                };
                let items = items.borrow().clone();
                if items.len() != pats.len() {
                    return err(Kind::ArityMismatch, format!("expected a list of {}, found {}", pats.len(), items.len()));
                }
                for (p, v) in pats.iter().zip(items) {
                    self.bind(p, v)?;
                }
                Ok(())
            }
            Pattern::Tuple(pats) => {
                let Value::Tuple(items) = value else {
                    return err(Kind::ArgumentType, format!("cannot destructure {} into a tuple", value.type_name()));
                };
                if items.len() != pats.len() {
                    return err(Kind::ArityMismatch, format!("expected a tuple of {}, found {}", pats.len(), items.len()));
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
            // center は描く先の箱の中心。箱が決まるまで数にならない長さの Vector
            .or_else(|| {
                use crate::lang::length::Length;
                (name == "center").then(|| Value::LVec(Length::with_unit("vx", 50.0).expect("unit"), Length::with_unit("vy", 50.0).expect("unit")))
            })
            // builtin の型は名前だけで値として引ける (alias 用)。module の中にだけある型は、module から取る
            .or_else(|| {
                (Self::is_builtin_type(name) && !crate::docs::MODULE_ONLY.contains(&name)).then(|| Value::BuiltinType(name.to_string()))
            })
            .ok_or_else(|| {
                let hint = if stdlib::find(name).is_some() { format!("; add \"import {name}\"") } else { String::new() };
                MophError::new(Kind::UndefinedVariable, format!("\"{name}\" is not defined{hint}"))
            })
    }

    fn eval_object(&mut self, e: &Expr) -> Result<ObjRef> {
        match self.eval(e)? {
            Value::Object(o) => Ok(o),
            v => err(Kind::AttributeType, format!("expected an object, found {}", v.type_name())),
        }
    }

    pub fn eval(&mut self, e: &Expr) -> Result<Value> {
        match e {
            Expr::Number(v) => Ok(Value::num(*v)),
            Expr::Duration(v) => Ok(Value::Duration(*v)),
            Expr::Length(l) => Ok(Value::Length(*l)),
            Expr::Color(c) => Ok(Value::Color(*c)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Symbol(s) => Ok(Value::Symbol(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Ident(name) => self.lookup(name),
            Expr::Neg(inner) => match self.eval(inner)? {
                Value::Number(v, r) => Ok(match r.and_then(|r| Ratio::new(-r.num, r.den)) {
                    Some(r) => Value::Number(-v, Some(r)),
                    None => Value::num(-v),
                }),
                Value::Duration(v) => Ok(Value::Duration(-v)),
                Value::Length(l) => Ok(Value::Length(l.scaled(-1.0))),
                Value::LVec(x, y) => Ok(Value::LVec(x.scaled(-1.0), y.scaled(-1.0))),
                Value::Vector(x, y) => Ok(Value::Vector(-x, -y)),
                Value::Vector3(x, y, z) => Ok(Value::Vector3(-x, -y, -z)),
                v => err(Kind::OperandType, format!("cannot negate {}", v.type_name())),
            },
            Expr::Not(inner) => match self.eval(inner)? {
                Value::Bool(b) => Ok(Value::Bool(!b)),
                v => err(Kind::OperandType, format!("cannot apply not to {}", v.type_name())),
            },
            Expr::Binary(BinOp::And, l, r) => match self.eval(l)? {
                Value::Bool(false) => Ok(Value::Bool(false)),
                Value::Bool(true) => self.eval_bool(r).map(Value::Bool),
                v => err(Kind::OperandType, format!("cannot apply and to {}", v.type_name())),
            },
            Expr::Binary(BinOp::Or, l, r) => match self.eval(l)? {
                Value::Bool(true) => Ok(Value::Bool(true)),
                Value::Bool(false) => self.eval_bool(r).map(Value::Bool),
                v => err(Kind::OperandType, format!("cannot apply or to {}", v.type_name())),
            },
            // a < b <= c。真ん中は 1 度だけ評価し、偽が出たらそこで止める
            Expr::Compare(first, rest) => {
                let mut left = self.eval(first)?;
                for (op, e) in rest {
                    let right = self.eval(e)?;
                    match binary(*op, left, right.clone())? {
                        Value::Bool(true) => {}
                        v => return Ok(v),
                    }
                    left = right;
                }
                Ok(Value::Bool(true))
            }
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
            Expr::Attr(target, attr) => {
                let target = self.eval(target)?;
                match self.attr(&target, attr) {
                    Some(found) => found.get(),
                    None => err(Kind::UndefinedAttribute, no_attr(&target, attr)),
                }
            }
            Expr::Call(callee, args) => self.call(callee, args),
            Expr::Dict(entries) => {
                let mut out: Vec<(String, Value)> = Vec::new();
                for (key, e) in entries {
                    let key = match key {
                        DictKey::Str(k) | DictKey::Shorthand(k) => k.clone(),
                    };
                    let v = self.eval(e)?;
                    // 同じキーを 2 回書いたら後が勝つ。Dict(k = v) と同じ
                    match out.iter_mut().find(|(k, _)| *k == key) {
                        Some(slot) => slot.1 = v,
                        None => out.push((key, v)),
                    }
                }
                Ok(Value::Dict(Rc::new(RefCell::new(out))))
            }
            Expr::Func(def) => Ok(Value::Func(Rc::new(Closure { def: def.clone(), scopes: self.scopes.clone() }))),
            Expr::Progress(def) => self.eval_progress(def),
        }
    }

    /// progress の表。行の位置と値はここで評価する。列ごとの `_` は飛ばす
    fn eval_progress(&mut self, def: &ProgressDef) -> Result<Value> {
        let mut tracks: Vec<Vec<(f64, Value, Option<String>)>> = vec![Vec::new(); def.columns.len()];
        let mut previous = f64::NEG_INFINITY;
        for row in &def.rows {
            let at = match self.eval(&row.at)? {
                Value::Number(n, _) if (0.0..=1.0).contains(&n) => n,
                v => return err(Kind::OutOfRange, format!("a progress row is at a Number within 0..1, found {v}")),
            };
            if at <= previous {
                return err(Kind::OutOfRange, format!("progress rows go forward, found {at} after {previous}"));
            }
            previous = at;
            for (track, cell) in tracks.iter_mut().zip(&row.cells) {
                if let Some((e, curve)) = cell {
                    track.push((at, self.eval(e)?, curve.clone()));
                }
            }
        }
        for (name, track) in def.columns.iter().zip(&tracks) {
            let ends = (track.first().map(|k| k.0), track.last().map(|k| k.0));
            if ends != (Some(0.0), Some(1.0)) {
                return err(Kind::OutOfRange, format!("progress column :{name} needs values at 0 and 1"));
            }
        }
        Ok(Value::Progress(Rc::new(ProgressTable { columns: def.columns.clone(), tracks })))
    }

    /// 引数を、宣言の順に並んだフィールド名へ割り当てる。位置と名前を混ぜられる
    /// 関数として呼ぶ。書いた関数でも math の関数でも同じように呼べる
    pub(crate) fn call_func(&mut self, f: &Value, args: Vec<Value>) -> Result<Value> {
        match f {
            Value::Func(c) => self.apply(c, args.into_iter().map(|v| (None, v)).collect()),
            Value::Builtin(name) => call_native(name, &args),
            Value::Progress(table) => progress_at(table, &args),
            v => err(Kind::ArgumentType, format!("{} is not a function", v.type_name())),
        }
    }

    /// 名前を付けずに並べた引数だけを取る
    fn positional(&mut self, kind: &str, args: &[Arg]) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(args.len());
        for (name, v) in self.eval_args(args)? {
            match name {
                Some(n) => return err(Kind::ArgumentType, format!("{kind} takes no named argument \"{n}\"")),
                None => out.push(v),
            }
        }
        Ok(out)
    }

    /// 引数を評価する。`...xs` は並びを 1 つずつの引数に広げる
    fn eval_args(&mut self, args: &[Arg]) -> Result<Vec<(Option<String>, Value)>> {
        let mut out = Vec::new();
        for arg in args {
            let v = self.eval(&arg.value)?;
            if !arg.spread {
                out.push((arg.name.clone(), v));
                continue;
            }
            match &v {
                Value::List(items) => out.extend(items.borrow().iter().map(|i| (None, i.clone()))),
                Value::Array(a) => out.extend(a.nums.iter().map(|x| (None, Value::num(*x)))),
                Value::Tuple(items) => out.extend(items.iter().map(|i| (None, i.clone()))),
                Value::Range(a, b) => out.extend((*a..*b).map(|i| (None, Value::num(i as f64)))),
                // Dict は キー = 値 の名前付き引数になる
                Value::Dict(entries) => out.extend(entries.borrow().iter().map(|(k, v)| (Some(k.clone()), v.clone()))),
                v => return err(Kind::ArgumentType, format!("... expects List, Tuple, Range or Dict, found {}", v.type_name())),
            }
        }
        Ok(out)
    }

    fn resolve_args(&mut self, kind: &str, fields: &[&str], args: &[Arg]) -> Result<HashMap<String, Value>> {
        let mut map: HashMap<String, Value> = HashMap::new();
        let mut next = 0;
        for (given, v) in self.eval_args(args)? {
            let name = match &given {
                Some(n) => {
                    if !fields.contains(&n.as_str()) {
                        return err(Kind::UndefinedAttribute, format!("{kind} has no field \"{n}\""));
                    }
                    n.clone()
                }
                None => {
                    let Some(f) = fields.get(next) else {
                        return err(Kind::ArityMismatch, format!("{kind} takes {} fields, more were given", fields.len()));
                    };
                    next += 1;
                    (*f).to_string()
                }
            };
            if map.insert(name.clone(), v).is_some() {
                return err(Kind::ArgumentType, format!("{kind}.{name} is given twice"));
            }
        }
        Ok(map)
    }

    /// 呼んでは作れない名前。builtin の型なら書き方を示す
    fn cannot_construct(&self, kind: &str) -> Result<Value> {
        let Some(t) = crate::docs::types().iter().find(|t| t.name == kind) else {
            return match self.types.contains_key(kind) {
                true => err(Kind::ArgumentType, format!("{kind} is a union of types, so it cannot be built")),
                false => err(Kind::UndefinedVariable, format!("type \"{kind}\" is not defined")),
            };
        };
        if !t.members.is_empty() {
            return err(Kind::ArgumentType, format!("{kind} is a union of {}, so it cannot be built", t.members.join(" | ")));
        }
        match (t.make.lines().next().filter(|f| !f.is_empty()), t.values.first()) {
            (Some(form), _) => err(Kind::ArgumentType, format!("{kind} cannot be called; write it like {form}")),
            (_, Some((v, _))) => err(Kind::ArgumentType, format!("{kind} only takes fixed values; write one like {v}")),
            _ => err(Kind::ArgumentType, format!("{kind} cannot be called")),
        }
    }

    /// Group(...) の名前の無い引数。Node だけを受ける。`...xs` で並びを広げられる
    fn group_items(&mut self, args: &[Arg]) -> Result<Vec<ObjRef>> {
        let unnamed: Vec<Arg> = args.iter().filter(|a| a.name.is_none()).cloned().collect();
        let mut items = Vec::new();
        for (_, v) in self.eval_args(&unnamed)? {
            items.push(self.group_item(v)?);
        }
        Ok(items)
    }

    /// Group の中身は図形と Group
    fn group_item(&self, v: Value) -> Result<ObjRef> {
        match v {
            Value::Object(o) if self.matches_type(&Value::Object(o.clone()), "Shape") => Ok(o),
            v => err(Kind::ArgumentType, format!("Group holds shapes and Groups, found {}", v.type_name())),
        }
    }

    /// 型名を呼んで値を作る
    /// 素の名前からの生成。モジュールの中にだけある型はここでは作れない
    fn construct(&mut self, kind: &str, args: &[Arg]) -> Result<Value> {
        if crate::docs::MODULE_ONLY.contains(&kind) {
            return self.cannot_construct(kind);
        }
        self.construct_kind(kind, args)
    }

    /// モジュール経由の生成 (space3d.PerspectiveCamera(...) など)
    fn construct_kind(&mut self, kind: &str, args: &[Arg]) -> Result<Value> {
        match kind {
            "Vector" => {
                let map = self.resolve_args(kind, &["x", "y"], args)?;
                match (map.get("x"), map.get("y")) {
                    (Some(Value::Number(x, _)), Some(Value::Number(y, _))) => Ok(Value::Vector(*x, *y)),
                    (Some(x), Some(y)) if x.as_length().is_some() && y.as_length().is_some() => {
                        Ok(Value::LVec(x.as_length().expect("length"), y.as_length().expect("length")))
                    }
                    _ => err(Kind::ArgumentType, "Vector needs x and y (Number or Length)"),
                }
            }
            "Vector3" => {
                let map = self.resolve_args(kind, &["x", "y", "z"], args)?;
                match (map.get("x"), map.get("y"), map.get("z")) {
                    (Some(Value::Number(x, _)), Some(Value::Number(y, _)), Some(Value::Number(z, _))) => Ok(Value::Vector3(*x, *y, *z)),
                    _ => err(Kind::ArgumentType, "Vector3 needs x, y and z (Number)"),
                }
            }
            "Color" => {
                let map = self.resolve_args(kind, &["r", "g", "b", "a"], args)?;
                let ch = |name: &str, scale: f64, default: Option<f64>| -> Result<f32> {
                    match (map.get(name), default) {
                        (Some(Value::Number(v, _)), _) => Ok((v / scale) as f32),
                        (Some(v), _) => err(Kind::ArgumentType, format!("Color.{name} expects Number, found {}", v.type_name())),
                        (None, Some(d)) => Ok(d as f32),
                        (None, None) => err(Kind::ArityMismatch, format!("Color needs {name}")),
                    }
                };
                Ok(Value::Color([ch("r", 255.0, None)?, ch("g", 255.0, None)?, ch("b", 255.0, None)?, ch("a", 1.0, Some(1.0))?]))
            }
            "Tuple" => Ok(Value::Tuple(self.positional(kind, args)?)),
            "List" => Ok(Value::List(Rc::new(RefCell::new(self.positional(kind, args)?)))),
            "Array" => {
                let items = self.positional(kind, args)?;
                // 1 つだけ渡した並びは、その中身
                let items = match items.as_slice() {
                    [Value::List(xs)] => xs.borrow().clone(),
                    [Value::Tuple(xs)] => xs.clone(),
                    [Value::Array(a)] => return Ok(Value::Array(Rc::clone(a))),
                    [Value::Range(a, b)] => (*a..*b).map(|i| Value::num(i as f64)).collect(),
                    _ => items,
                };
                let nums = items
                    .iter()
                    .map(|v| match v {
                        Value::Number(x, _) => Ok(*x),
                        other => err(Kind::ArgumentType, format!("Array holds Numbers only, found {}", other.type_name())),
                    })
                    .collect::<Result<Vec<f64>>>()?;
                Ok(Value::Array(Rc::new(crate::lang::value::Array::new(nums))))
            }
            "Dict" => {
                let mut items: Vec<(String, Value)> = Vec::new();
                for a in args {
                    let Some(key) = a.name.clone() else {
                        return err(Kind::ArgumentType, "Dict takes named arguments, as Dict(k = v); write { \"k\": v } for a key that is not a name");
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
                    (Some(Value::Number(a, _)), Some(Value::Number(b, _))) => {
                        Ok(Value::Range(whole(*a, "Range.start")?, whole(*b, "Range.end")?))
                    }
                    _ => err(Kind::ArgumentType, "Range needs start and end (Number)"),
                }
            }
            _ => {
                // Group は名前の無い引数が中身で、名前のある引数が属性
                let items = match kind {
                    "Group" => self.group_items(args)?,
                    _ => Vec::new(),
                };
                let named: Vec<Arg>;
                let args = match kind {
                    "Group" => {
                        named = args.iter().filter(|a| a.name.is_some()).cloned().collect();
                        &named[..]
                    }
                    _ => args,
                };
                if let Some(sch) = schema(kind) {
                    let fields: Vec<&str> = sch.iter().map(|a| a.name).collect();
                    let map = self.resolve_args(kind, &fields, args)?;
                    let mut attrs = HashMap::new();
                    for (name, v) in map {
                        let expected = sch.iter().find(|a| a.name == name).map(|a| a.ty).expect("field exists");
                        let v = self.fitted_attr(expected, v);
                        if !self.attr_ok(expected, &v) {
                            return Err(self.wrong_type(Kind::ArgumentType, &format!("{kind}.{name}"), expected, &v));
                        }
                        // シェーダは関数の中身を GPU 向けに変換するので、書いた func しか受け取れない。
                        // 変換できるかはここで見る。描くまで待たずに run で分かる
                        if kind == "Shader" && name == "color" {
                            let Value::Func(closure) = &v else {
                                return err(Kind::ArgumentType, "Shader.color expects a func written in the script, like func (x, y, t) { ... }");
                            };
                            // camera を入れると引数が 1 つ増えるので、そのつもりで検査する
                            let has_camera = args.iter().any(|a| a.name.as_deref() == Some("camera"));
                            crate::render::shader::compile(closure, has_camera)?;
                        }
                        // 候補を並べて書けるので、この機械にある最初のものに決める
                        let v = match kind == "TextArea" && name == "font" {
                            true => self.pick_font(&v)?,
                            false => v,
                        };
                        attrs.insert(name, v);
                    }
                    // 書かなかった属性も、既定のある分は入れておく。描く側と読む側が同じ値を見る
                    for (name, value) in defaults(kind) {
                        attrs.entry((*name).to_string()).or_insert_with(|| value.clone());
                    }
                    check_text_anchor(kind, &attrs)?;
                    if kind == "ZoomPath" {
                        attrs.insert("scale".into(), self.zoom_table(&attrs)?);
                    }
                    if let (true, Some(Value::Number(k, _))) = (kind == "Camera", attrs.get("scale")) {
                        if *k <= 0.0 {
                            return err(Kind::OutOfRange, format!("Camera.scale must be above 0, found {k}"));
                        }
                    }
                    return Ok(Value::Object(Rc::new(RefCell::new(Object { id: next_object_id(), kind: kind.to_string(), decl: None, attrs, children: items, layers: vec![] }))));
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
            return err(Kind::UndefinedAttribute, format!("{}.{name} is private", ty.decl.name));
        }
        let mut values: Vec<(Option<String>, Value)> = Vec::new();
        if let Some(v) = self_value {
            values.push((None, v));
        }
        values.extend(self.eval_args(args)?);
        let closure = Closure { def: member.def.clone(), scopes: ty.scopes.clone() };
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
    pub(crate) fn field_ok(&self, decl: &Option<Rc<UserType>>, field: &str) -> Result<()> {
        let Some(ty) = decl else { return Ok(()) };
        match ty.decl.fields.iter().find(|f| f.name == field) {
            Some(f) if f.private && !self.is_inside(&ty.decl.name) => {
                err(Kind::UndefinedAttribute, format!("{}.{field} is private", ty.decl.name))
            }
            _ => Ok(()),
        }
    }

    /// copy / shallow_copy / deep_copy
    fn copy_method(&mut self, ty: &Rc<UserType>, receiver: &Value, name: &str, args: &[Arg]) -> Result<Option<Value>> {
        let wanted = matches!(name, "copy" | "shallow_copy" | "deep_copy");
        if !wanted || ty.decl.members.iter().any(|m| m.name == name && m.receiver) {
            return Ok(None);
        }
        if ty.decl.nocopy {
            return err(Kind::UndefinedAttribute, format!("{} is @nocopy, so it cannot be copied", ty.decl.name));
        }
        // func new を書いた型は、そこで値を決めている。copy で中身を差し替えると、そこを通らない
        if !args.is_empty() && ty.decl.members.iter().any(|m| !m.receiver && m.name == "new") {
            return err(
                Kind::ArgumentType,
                format!("{}.{name} cannot change fields because {} declares func new; call {}(...) instead", ty.decl.name, ty.decl.name, ty.decl.name),
            );
        }
        match (receiver, name) {
            (Value::Record(r), "copy") => {
                let changed = self.resolve_args(&ty.decl.name, &r.fields.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>(), args)?;
                let fields = r.fields.iter().map(|(f, v)| (f.clone(), changed.get(f).cloned().unwrap_or_else(|| v.clone()))).collect();
                Ok(Some(Value::Record(Rc::new(Record { name: r.name.clone(), decl: r.decl.clone(), fields }))))
            }
            (Value::Object(o), "shallow_copy") | (Value::Object(o), "deep_copy") => {
                if name == "deep_copy" && ty.decl.nodeepcopy {
                    return err(Kind::UndefinedAttribute, format!("{} is @nodeepcopy, so it has no deep_copy", ty.decl.name));
                }
                if !args.is_empty() {
                    return err(Kind::ArityMismatch, format!("{name} takes no arguments"));
                }
                let src = o.borrow();
                let attrs = if name == "deep_copy" { src.attrs.iter().map(|(k, v)| (k.clone(), deep_copy(v))).collect() } else { src.attrs.clone() };
                Ok(Some(Value::Object(Rc::new(RefCell::new(Object {
                    id: next_object_id(),
                    kind: src.kind.clone(),
                    decl: src.decl.clone(),
                    attrs,
                    children: src.children.clone(),
                    layers: src.layers.clone(),
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
            return err(Kind::ArityMismatch, format!("{}.{} is private and has no default, so write \"func new\"", decl.name, f.name));
        }
        let mut given: HashMap<String, Value> = HashMap::new();
        let mut next = 0;
        for (name, v) in args {
            let field = match name {
                Some(n) => {
                    if !open.contains(&n.as_str()) {
                        return err(Kind::UndefinedAttribute, format!("{} has no field \"{n}\"", decl.name));
                    }
                    n
                }
                None => match open.get(next) {
                    Some(f) => {
                        next += 1;
                        (*f).to_string()
                    }
                    None => return err(Kind::ArityMismatch, format!("{} takes {} fields, more were given", decl.name, open.len())),
                },
            };
            if given.insert(field.clone(), v).is_some() {
                return err(Kind::ArgumentType, format!("{}.{field} is given twice", decl.name));
            }
        }
        let mut fields: Vec<(String, Value)> = Vec::new();
        for f in &decl.fields {
            let v = match given.remove(&f.name) {
                Some(v) => v,
                None => match &f.default {
                    Some(d) => self.eval(d)?,
                    None => return err(Kind::ArityMismatch, format!("{}.{} is not given", decl.name, f.name)),
                },
            };
            let v = self.coerced(v, &f.ann);
            self.check_field(&decl.name, &f.name, &v, &f.ann)?;
            fields.push((f.name.clone(), v));
        }
        Ok(if decl.immutable {
            Value::Record(Rc::new(Record { name: decl.name.clone(), decl: Some(ty.clone()), fields }))
        } else {
            Value::Object(Rc::new(RefCell::new(Object {
                id: next_object_id(),
                kind: decl.name.clone(),
                decl: Some(ty.clone()),
                attrs: fields.into_iter().collect(),
                children: vec![],
                layers: vec![],
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
            let values = self.eval_args(args)?;
            return self.build_from_fields(ty, values);
        }
        let values = self.eval_args(args)?;
        // 必須の数が合うものを選ぶ
        let count = values.iter().filter(|(n, _)| n.is_none()).count();
        let fits = |m: &&&crate::lang::ast::MemberDecl| {
            let required = m.def.params.iter().filter(|p| p.default.is_none()).count();
            count >= required && count <= m.def.params.len()
        };
        let Some(member) = news.iter().find(fits).or_else(|| news.first()) else {
            return err(Kind::ArityMismatch, format!("no new of {} takes {count} arguments", ty.decl.name));
        };
        let closure = Closure { def: member.def.clone(), scopes: ty.scopes.clone() };
        self.constructing.push(ty.decl.name.clone());
        self.calling = Some(ty.decl.name.clone());
        let out = self.apply(&closure, values);
        self.constructing.pop();
        out
    }

    /// builtin の型の名前か
    fn is_builtin_type(name: &str) -> bool {
        crate::docs::types().iter().any(|t| t.name == name)
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
            Expr::Attr(target, method) => {
                let receiver = self.eval(target)?;
                // 型から func を呼ぶ / 値から method を呼ぶ
                if let Some(out) = self.user_member(&receiver, method, args)? {
                    return Ok(out);
                }
                if let Value::Module(m) = &receiver {
                    let Some(item) = m.items.get(method).cloned() else {
                        return err(Kind::UndefinedAttribute, format!("module {} has no item \"{method}\"", m.name));
                    };
                    // mod.TypeName(...) は、その型を作る
                    if let Value::Type(ty) = &item {
                        let ty = ty.clone();
                        return self.construct_user(&ty, args);
                    }
                    if let Value::BuiltinType(name) = &item {
                        let name = name.clone();
                        return self.construct_kind(&name, args);
                    }
                    let given = self.eval_args(args)?;
                    return match item {
                        Value::Builtin(name) => call_builtin(name, given.into_iter().map(|(_, v)| v).collect()),
                        Value::Func(closure) => self.apply(&closure, given),
                        Value::Progress(table) => progress_at(&table, &given.into_iter().map(|(_, v)| v).collect::<Vec<_>>()),
                        v => err(Kind::ArgumentType, format!("{} is not callable", v.type_name())),
                    };
                }
                let values = self.eval_args(args)?;
                if let Value::Object(obj) = &receiver {
                    let obj = obj.clone();
                    return self.method(&obj, method, values);
                }
                match crate::lang::method::find(&receiver.type_name(), method) {
                    Some(m) => (m.call)(self, receiver, values),
                    None => err(Kind::UndefinedAttribute, format!("{} has no method \"{method}\"", receiver.type_name())),
                }
            }
            callee => match self.eval(callee)? {
                // 値として持っている型は、module から取り出したものなので作れる (let V = space3d.Vector3)
                Value::BuiltinType(name) => self.construct_kind(&name, args),
                Value::Type(ty) => self.construct_user(&ty, args),
                Value::Func(closure) => {
                    let values = self.eval_args(args)?;
                    self.apply(&closure, values)
                }
                Value::Builtin(name) => {
                    let values = self.eval_args(args)?.into_iter().map(|(_, v)| v).collect::<Vec<_>>();
                    call_builtin(name, values)
                }
                Value::Progress(table) => {
                    let values = self.eval_args(args)?.into_iter().map(|(_, v)| v).collect::<Vec<_>>();
                    progress_at(&table, &values)
                }
                v => err(Kind::ArgumentType, format!("{} is not callable", v.type_name())),
            },
        }
    }

    /// ZoomPath の表。倍率 1 のときの 1 箱単位あたりの長さ (unit) を zoom(t) で割り、
    /// その対数を duration まで等間隔に並べる。描くときに 1 フレームずつ呼び直せないので、
    /// 作るときに 1 度だけ並べておく
    fn zoom_table(&mut self, attrs: &HashMap<String, Value>) -> Result<Value> {
        const STEPS: usize = 20_000;
        let (Some(Value::Func(zoom)), Some(Value::Duration(duration))) = (attrs.get("zoom"), attrs.get("duration")) else {
            return err(Kind::AttributeType, "ZoomPath needs zoom (func (t) -> Number) and duration");
        };
        if *duration <= 0.0 {
            return err(Kind::OutOfRange, "ZoomPath.duration must be above 0");
        }
        let unit = match attrs.get("unit") {
            Some(Value::Number(u, _)) if *u > 0.0 => *u,
            Some(Value::Number(u, _)) => return err(Kind::OutOfRange, format!("ZoomPath.unit must be above 0, found {u}")),
            _ => 1.0,
        };
        let (zoom, duration) = (zoom.clone(), *duration);
        let mut out = Vec::with_capacity(STEPS + 1);
        for i in 0..=STEPS {
            let t = duration * i as f64 / STEPS as f64;
            // t は秒の Number。同じ関数がシェーダの中でも走るので Duration にはできない
            match self.apply(&zoom, vec![(None, Value::num(t))])? {
                Value::Number(k, _) if k > 0.0 => out.push(Value::num((unit / k).ln())),
                Value::Number(k, _) => return err(Kind::OutOfRange, format!("ZoomPath.zoom must stay above 0, found {k} at {t}s")),
                other => return err(Kind::AttributeType, format!("ZoomPath.zoom must return Number, found {}", other.type_name())),
            }
        }
        Ok(Value::List(Rc::new(RefCell::new(out))))
    }

    /// 点の並びをつないだ Path。segments の組を手で書かずに済む
    pub(crate) fn path_through(&mut self, points: &[Value], closed: bool) -> Result<Value> {
        let [from, rest @ ..] = points else {
            return err(Kind::OutOfRange, "Path.through needs at least one point");
        };
        if !matches!(from, Value::Vector(..)) {
            return err(Kind::ArgumentType, format!("Path.through expects Vectors, found {}", from.type_name()));
        }
        let mut segments = Vec::new();
        for p in rest {
            if !matches!(p, Value::Vector(..)) {
                return err(Kind::ArgumentType, format!("Path.through expects Vectors, found {}", p.type_name()));
            }
            segments.push(Value::Tuple(vec![Value::Symbol("line".into()), p.clone()]));
        }
        let mut attrs: HashMap<String, Value> = HashMap::new();
        attrs.insert("from".into(), from.clone());
        attrs.insert("segments".into(), Value::List(Rc::new(RefCell::new(segments))));
        attrs.insert("closed".into(), Value::Bool(closed));
        for (name, value) in defaults("Path") {
            attrs.entry((*name).to_string()).or_insert_with(|| value.clone());
        }
        Ok(Value::Object(Rc::new(RefCell::new(Object { id: next_object_id(), kind: "Path".into(), decl: None, attrs, children: vec![], layers: vec![] }))))
    }

    /// `comp.place(name, content, in = , out = )`。置いた順が積み重ねの順
    fn place_layer(&mut self, comp: &ObjRef, args: &[(Option<String>, Value)]) -> Result<Value> {
        if let Some((Some(other), _)) = args.iter().find(|(n, _)| n.as_deref().is_some_and(|n| n != "in" && n != "out")) {
            return err(Kind::ArgumentType, format!("Composition.place has no argument \"{other}\""));
        }
        let mut positional = args.iter().filter(|(n, _)| n.is_none()).map(|(_, v)| v);
        let (Some(Value::Str(name)), Some(content), None) = (positional.next(), positional.next(), positional.next()) else {
            return err(Kind::ArityMismatch, "Composition.place takes a name, what to place, and in = and out =");
        };
        let time = |key: &str| match args.iter().find(|(n, _)| n.as_deref() == Some(key)) {
            Some((_, Value::Duration(d))) => Ok(*d),
            Some((_, v)) => err(Kind::ArgumentType, format!("Composition.place {key} expects Duration, found {}", v.type_name())),
            None => err(Kind::ArityMismatch, format!("Composition.place needs {key} =")),
        };
        let (at, out) = (time("in")?, time("out")?);
        if !(at < out) {
            return err(Kind::OutOfRange, format!("Composition.place: out must be after in, found in = {at}s and out = {out}s"));
        }
        match content {
            Value::Func(_) | Value::Audio(_) => {}
            Value::Object(o) if Rc::ptr_eq(o, comp) => return err(Kind::OutOfRange, "a Composition cannot be placed in itself"),
            Value::Object(o) if o.borrow().kind == "Composition" || o.borrow().kind == "Narration" || self.matches_type(content, "Shape") => {}
            v => return err(Kind::NotPlaceable, format!("Composition.place expects a Node or func (t), found {}", v.type_name())),
        }
        if comp.borrow().layers.iter().any(|l| l.name == *name) {
            return err(Kind::AlreadyPlaced, format!("a layer named \"{name}\" is already placed"));
        }
        comp.borrow_mut().layers.push(Layer { name: name.clone(), content: content.clone(), at, out });
        Ok(Value::Nothing)
    }

    /// 組み込みの値の複製。名前付きの引数で書いた属性だけが変わる
    fn copy_builtin(&mut self, obj: &ObjRef, args: Vec<(Option<String>, Value)>) -> Result<Value> {
        let kind = obj.borrow().kind.clone();
        let sch = schema(&kind).expect("a builtin kind has a schema");
        let mut attrs = obj.borrow().attrs.clone();
        for (name, v) in args {
            let Some(name) = name else {
                return err(Kind::ArityMismatch, format!("{kind}.copy takes named arguments, like copy(radius = 2)"));
            };
            let Some(a) = sch.iter().find(|a| a.name == name) else {
                return err(Kind::UndefinedAttribute, format!("{kind} has no attribute \"{name}\""));
            };
            let v = self.fitted_attr(a.ty, v);
            if !self.attr_ok(a.ty, &v) {
                return Err(self.wrong_type(Kind::ArgumentType, &format!("{kind}.{name}"), a.ty, &v));
            }
            attrs.insert(name, v);
        }
        check_text_anchor(&kind, &attrs)?;
        let src = obj.borrow();
        Ok(Value::Object(Rc::new(RefCell::new(Object { id: next_object_id(), kind, decl: None, attrs, children: src.children.clone(), layers: src.layers.clone() }))))
    }

    /// 動画の長さ。output した Composition の duration
    pub fn duration_of(comp: &ObjRef) -> Result<f64> {
        match comp.borrow().attrs.get("duration") {
            Some(Value::Duration(d)) => Ok(*d),
            _ => err(Kind::AttributeType, "Composition.duration is not set"),
        }
    }

    /// 描くのに GPU が要る塗りが、どこかの時刻にあるか。Layer の始まりと真ん中の時刻で調べる
    pub fn uses_gpu_fill(&mut self, comp: &ObjRef) -> Result<bool> {
        let mut times: Vec<f64> = comp.borrow().layers.iter().flat_map(|l| [l.at, (l.at + l.out) / 2.0]).collect();
        times.sort_by(f64::total_cmp);
        times.dedup();
        for t in times {
            let frame = self.frame(comp, t)?;
            if crate::render::scene::uses_gpu_fill(&frame) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 時刻 t の Composition を、その時刻に存在する Node だけを集めた木にする。
    /// 描くのは、返した木。Layer の関数は `t - in` で呼ぶ。同じ t には同じ木が返る
    pub fn frame(&mut self, comp: &ObjRef, t: f64) -> Result<ObjRef> {
        let nodes = self.nodes_at(comp, t)?;
        let attrs = comp.borrow().attrs.clone();
        Ok(Rc::new(RefCell::new(Object { id: next_object_id(), kind: "View".into(), decl: None, attrs, children: nodes, layers: vec![] })))
    }

    /// Composition の Layer のうち、t に存在するものの Node。積み重ねの順
    fn nodes_at(&mut self, comp: &ObjRef, t: f64) -> Result<Vec<ObjRef>> {
        let layers: Vec<Layer> = comp.borrow().layers.iter().filter(|l| l.exists_at(t)).cloned().collect();
        let mut out = Vec::new();
        for layer in layers {
            let local = t - layer.at;
            let node = match &layer.content {
                Value::Func(f) => match self.apply(f, vec![(None, Value::Duration(local))])? {
                    Value::Object(o) => o,
                    v => return err(Kind::ArgumentType, format!("layer \"{}\": the function must return a Node, found {}", layer.name, v.type_name())),
                },
                Value::Object(o) => o.clone(),
                // 音と読み上げは描かない
                _ => continue,
            };
            let kind = node.borrow().kind.clone();
            match kind.as_str() {
                "Narration" => {}
                // 自分の duration の外には存在しない
                "Composition" if (0.0..Self::duration_of(&node)?).contains(&local) => out.push(self.nested(&node, local)?),
                "Composition" => {}
                _ => out.push(node),
            }
        }
        Ok(out)
    }

    /// 入れ子の Composition。自分の箱を基準にした Group として描く
    fn nested(&mut self, comp: &ObjRef, t: f64) -> Result<ObjRef> {
        if !matches!(comp.borrow().attrs.get("box"), Some(Value::Vector(..))) {
            return err(Kind::AttributeType, "Composition.box must be a Vector");
        }
        let children = self.nodes_at(comp, t)?;
        let attrs = comp.borrow().attrs.clone();
        Ok(Rc::new(RefCell::new(Object { id: next_object_id(), kind: "Group".into(), decl: None, attrs, children, layers: vec![] })))
    }

    pub(crate) fn method(&mut self, obj: &ObjRef, method: &str, args: Vec<(Option<String>, Value)>) -> Result<Value> {
        let kind = obj.borrow().kind.clone();
        match (kind.as_str(), method) {
            // 置く前に「この文字が何ユニットになるか」を知るためのもの。
            // 重なりを避けて並べたり、はみ出すなら fontSize を下げたりできる
            // 線の長さ。dash で少しずつ描き出すときに、全長を自分で足さずに済む
            ("Path" | "Polygon" | "Line" | "Circle" | "Ellipse" | "Rect", "length") => {
                if !args.is_empty() {
                    return err(Kind::ArityMismatch, format!("{kind}.length takes no arguments"));
                }
                let (path, _) = crate::render::scene::outline(&obj.borrow())?;
                Ok(Value::num(vello::kurbo::Shape::perimeter(&path, 1e-4)))
            }
            // 輪郭の途中の点。u は長さの割合。ペン先を線の先に合わせるときに使う
            ("Path" | "Polygon" | "Line" | "Circle" | "Ellipse" | "Rect", "point_at") => {
                let [(None, Value::Number(u, _))] = args.as_slice() else {
                    return err(Kind::ArgumentType, format!("{kind}.point_at takes one Number (0..1)"));
                };
                let (path, _) = crate::render::scene::outline(&obj.borrow())?;
                Ok(point_at(&path, *u))
            }
            ("TextArea", "size") => {
                if !args.is_empty() {
                    return err(Kind::ArityMismatch, "TextArea.size takes no arguments");
                }
                let (text, size, font, wrap, align) = {
                    let o = obj.borrow();
                    let Some(Value::Str(text)) = o.attrs.get("text") else {
                        return err(Kind::UndefinedAttribute, "TextArea.text is not set");
                    };
                    let Some(Value::Number(size, _)) = o.attrs.get("fontSize") else {
                        return err(Kind::UndefinedAttribute, "TextArea.fontSize is not set");
                    };
                    // 候補が並んでいることもあるので、描くときと同じ選び方をする
                    let font = o.attrs.get("font").map(font_names).unwrap_or_default();
                    let wrap = o.attrs.get("w").and_then(Value::as_length).map(|w| w.value() as f32);
                    let align = match o.attrs.get("align") {
                        Some(Value::Symbol(a)) => Some(a.clone()),
                        _ => None,
                    };
                    (text.clone(), *size, font, wrap, align)
                };
                // 箱の単位のまま組む (1 ユニット = 1 ピクセルとして測り、そのまま返す)
                let align = crate::render::text::alignment(align.as_deref());
                let family = self.cache_mut().first_family(&font);
                let layout = self.cache_mut().layout(&text, family.as_deref(), size as f32, wrap, align);
                let (w, h) = (f64::from(layout.width()), f64::from(layout.height()));
                Ok(Value::Vector(wrap.map_or(w, f64::from), h))
            }
            ("Composition", "place") => self.place_layer(obj, &args),
            // 組み込みの値は不変。変えたものは copy で作る
            (_, "copy") if obj.borrow().decl.is_none() && schema(&kind).is_some() => self.copy_builtin(obj, args),
            // カメラは投影の計算だけで、インタプリタの状態に触らない
            ("PerspectiveCamera" | "OrthographicCamera" | "IsometricCamera", _) => stdlib::space3d::camera_method(&obj.borrow(), method, &args),
            ("Transform3", _) => stdlib::space3d::transform_method(&obj.borrow(), method, &args),
            ("Mesh", _) => stdlib::space3d::mesh_method(&obj.borrow(), method, &args),
            _ => err(Kind::UndefinedAttribute, format!("{kind} has no method \"{method}\"")),
        }
    }

    fn eval_bool(&mut self, e: &Expr) -> Result<bool> {
        match self.eval(e)? {
            Value::Bool(b) => Ok(b),
            v => err(Kind::OperandType, format!("expected Bool, found {}", v.type_name())),
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
            Flow::Break => {
                self.breaking = true;
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
                        None => return err(Kind::ArityMismatch, format!("{} is not given", crate::lang::ast::pattern_text(&param.pattern))),
                    },
                };
                // 型を書いてあれば、渡された値を確かめる
                let value = match &param.ann {
                    Some(ann) => {
                        let value = self.coerced(value, ann);
                        if !self.matches_ann(&value, ann) {
                            return Err(self.wrong_type(Kind::ArgumentType, &crate::lang::ast::pattern_text(&param.pattern), &ann.text, &value));
                        }
                        value
                    }
                    None => value,
                };
                self.bind(&param.pattern, value)?;
            }
            if positional.next().is_some() {
                return err(Kind::ArityMismatch, format!("function takes {} arguments, more were given", params.len()));
            }
            if let Some(name) = named.keys().next() {
                return err(Kind::ArgumentType, format!("unknown named argument \"{name}\""));
            }
            let out = self.run_block(&closure.def.body).map(Flow::value)?;
            let Some(ann) = &closure.def.returns else { return Ok(out) };
            let out = self.coerced(out, ann);
            if !self.matches_ann(&out, ann) {
                return Err(self.wrong_type(Kind::ArgumentType, "the return value", &ann.text, &out));
            }
            Ok(out)
        })();
        self.scopes = saved;
        result
    }

}

/// "{name}" を順に引数で置き換える。名前は説明用で、対応は位置で決まる
/// "{a} {b}".format(...)。Dict を 1 つ渡したときは { } の中の名前で引き、
/// そうでなければ左から順に引数で置き換える
pub(crate) fn format(template: &str, values: &[Value]) -> Result<Value> {
    let by_name = match values {
        [Value::Dict(d)] => Some(d.borrow().clone()),
        _ => None,
    };
    let mut out = String::new();
    let mut rest = template;
    let mut index = 0;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}') else {
            return err(Kind::OutOfRange, format!("unterminated {{ in {template:?}"));
        };
        let name = &rest[start + 1..start + end];
        match &by_name {
            Some(entries) => match entries.iter().find(|(k, _)| k == name) {
                Some((_, v)) => out.push_str(&v.to_string()),
                None => return err(Kind::UndefinedAttribute, format!("format has no key \"{name}\"")),
            },
            None => match values.get(index) {
                Some(v) => out.push_str(&v.to_string()),
                None => return err(Kind::ArityMismatch, format!("format expects {} arguments, {} given", index + 1, values.len())),
            },
        }
        index += 1;
        rest = &rest[start + end + 1..];
    }
    if by_name.is_none() && index < values.len() {
        return err(Kind::ArityMismatch, format!("format expects {index} arguments, {} given", values.len()));
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
/// 画像の大きさだけを読む。中身 (ピクセル) は描くときに読む
fn load_image(name: &str, path: std::path::PathBuf) -> Result<crate::lang::value::ImageFile> {
    if !path.is_file() {
        return err(Kind::UndefinedVariable, format!("cannot read \"{name}\": {} is not a file", path.display()));
    }
    let (width, height) = image::image_dimensions(&path).map_err(|e| MophError::new(Kind::ImageUnreadable, format!("cannot read \"{name}\": {e}")))?;
    Ok(crate::lang::value::ImageFile { name: name.to_string(), path, width: f64::from(width), height: f64::from(height) })
}

fn load_audio(name: &str, path: std::path::PathBuf) -> Result<Audio> {
    if !path.is_file() {
        return err(Kind::UndefinedVariable, format!("cannot read \"{name}\": {} is not a file", path.display()));
    }
    // 音声ストリームがあることと、全体の長さを調べる。出力は "audio" の行 (ストリームごと) と長さの行
    let output = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "a", "-show_entries", "stream=codec_type:format=duration", "-of", "default=nw=1:nk=1"])
        .arg(&path)
        .output()
        .map_err(|e| MophError::new(Kind::AudioUnreadable, format!("ffprobe is needed to read \"{name}\": {e}")))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let length = match lines.as_slice() {
        [streams @ .., duration] if streams.contains(&"audio") => duration.parse::<f64>().ok(),
        _ => None,
    };
    let Some(length) = length else {
        return err(Kind::AudioUnreadable, format!("\"{name}\" has no audio stream ffprobe can read {}", String::from_utf8_lossy(&output.stderr).trim()));
    };
    Ok(Audio { name: name.to_string(), path, length, volume: 1.0, looping: false, fade_in: 0.0, fade_out: 0.0 })
}

fn index_value(target: &Value, index: &Value) -> Result<Value> {
    if let Value::Dict(entries) = target {
        let Value::Str(key) = index else {
            return err(Kind::OperandType, format!("Dict key must be String, found {}", index.type_name()));
        };
        return entries
            .borrow()
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| MophError::new(Kind::OutOfRange, format!("key {key:?} not found")));
    }
    // 文字列は文字の並びとして扱う (バイトではない)。s[0] は 1 文字の String
    if let Value::Str(text) = target {
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len() as i64;
        if let Value::Range(a, b) = index {
            let (a, b) = ((*a).clamp(0, n) as usize, (*b).clamp(0, n) as usize);
            return Ok(Value::Str(chars[a.min(b)..b].iter().collect()));
        }
        let Value::Number(i, _) = index else {
            return err(Kind::OperandType, format!("index must be Number, found {}", index.type_name()));
        };
        let i = whole(*i, "an index")?;
        let at = if i < 0 { i + n } else { i };
        if at < 0 || at >= n {
            return err(Kind::OutOfRange, format!("index {i} out of range for length {n}"));
        }
        return Ok(Value::Str(chars[at as usize].to_string()));
    }
    if let Value::Array(a) = target {
        let n = a.nums.len() as i64;
        if let Value::Range(x, y) = index {
            let (x, y) = ((*x).clamp(0, n) as usize, (*y).clamp(0, n) as usize);
            return Ok(Value::Array(Rc::new(crate::lang::value::Array::new(a.nums[x.min(y)..y].to_vec()))));
        }
        let Value::Number(i, _) = index else {
            return err(Kind::OperandType, format!("index must be Number, found {}", index.type_name()));
        };
        let i = whole(*i, "an index")?;
        let at = if i < 0 { i + n } else { i };
        if at < 0 || at >= n {
            return err(Kind::OutOfRange, format!("index {i} out of range for length {n}"));
        }
        return Ok(Value::num(a.nums[at as usize]));
    }
    // 借りたまま読む。ここで並び全体を複製すると、xs[i] を n 回で O(n²) になる
    let borrowed;
    let items: &[Value] = match target {
        Value::List(items) => {
            borrowed = items.borrow();
            &borrowed
        }
        Value::Tuple(items) => items,
        v => return err(Kind::OperandType, format!("cannot index {}", v.type_name())),
    };
    if let Value::Range(a, b) = index {
        let n = items.len() as i64;
        let (a, b) = ((*a).clamp(0, n) as usize, (*b).clamp(0, n) as usize);
        return Ok(Value::List(Rc::new(RefCell::new(items[a.min(b)..b].to_vec()))));
    }
    let Value::Number(i, _) = index else {
        return err(Kind::OperandType, format!("index must be Number, found {}", index.type_name()));
    };
    let n = items.len() as i64;
    let i = whole(*i, "an index")?;
    let at = if i < 0 { i + n } else { i };
    if at < 0 || at >= n {
        return err(Kind::OutOfRange, format!("index {i} out of range for length {n}"));
    }
    Ok(items[at as usize].clone())
}

pub(crate) fn equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x, _), Value::Number(y, _)) | (Value::Duration(x), Value::Duration(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Str(x), Value::Str(y)) | (Value::Symbol(x), Value::Symbol(y)) => x == y,
        (Value::Tuple(x), Value::Tuple(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| equals(a, b)),
        (Value::Vector(x0, y0), Value::Vector(x1, y1)) => x0 == x1 && y0 == y1,
        (Value::Length(x), Value::Length(y)) => x == y,
        (Value::LVec(x0, y0), Value::LVec(x1, y1)) => x0 == x1 && y0 == y1,
        // record は名前とフィールドが同じなら等しい
        (Value::Record(x), Value::Record(y)) => {
            x.name == y.name && x.fields.len() == y.fields.len() && x.fields.iter().zip(&y.fields).all(|((f0, v0), (f1, v1))| f0 == f1 && equals(v0, v1))
        }
        (Value::Color(x), Value::Color(y)) => x == y,
        (Value::Range(x0, y0), Value::Range(x1, y1)) => x0 == x1 && y0 == y1,
        // 入れ物は中身が同じなら等しい。Dict は並び順を見ない
        (Value::Array(x), Value::Array(y)) => Rc::ptr_eq(x, y) || x.nums == y.nums,
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
        // オブジェクトは同一性 (同じ実体か)
        (Value::Object(x), Value::Object(y)) => Rc::ptr_eq(x, y),
        (Value::Func(x), Value::Func(y)) => Rc::ptr_eq(x, y),
        (Value::Builtin(x), Value::Builtin(y)) => x == y,
        (Value::Audio(x), Value::Audio(y)) => Rc::ptr_eq(x, y),
        (Value::Module(x), Value::Module(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// 整数が要る所。切り捨てずにエラーにする
pub(crate) fn whole(v: f64, what: &str) -> Result<i64> {
    if v.fract() != 0.0 || !v.is_finite() {
        return err(Kind::OutOfRange, format!("{what} must be a whole number, found {v}"));
    }
    Ok(v as i64)
}

/// TextArea の anchor は絶対値か割合のどちらか 1 つ
fn check_text_anchor(kind: &str, attrs: &HashMap<String, Value>) -> Result<()> {
    if kind == "TextArea" && attrs.contains_key("anchor") && attrs.contains_key("anchorRatio") {
        return err(Kind::ArgumentType, "TextArea takes either anchor or anchorRatio, not both");
    }
    Ok(())
}

/// deep_copy 用。中の実体も新しく作る
fn deep_copy(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            let src = o.borrow();
            Value::Object(Rc::new(RefCell::new(Object {
                id: next_object_id(),
                kind: src.kind.clone(),
                decl: src.decl.clone(),
                attrs: src.attrs.iter().map(|(k, v)| (k.clone(), deep_copy(v))).collect(),
                children: src.children.clone(),
                layers: src.layers.clone(),
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
/// いちばん外側のスコープ。builtin 関数も、ふつうの束縛として置く。
/// 名前で分岐しないので、同じ名前を書いたときに黙って builtin が勝つことがない
fn root_scope() -> Rc<RefCell<HashMap<String, Value>>> {
    let scope = new_scope();
    for b in crate::docs::BUILTINS {
        scope.borrow_mut().insert(b.name.to_string(), Value::Builtin(b.name));
    }
    scope
}

/// progress の表を、位置 1 つで引く
fn progress_at(table: &ProgressTable, values: &[Value]) -> Result<Value> {
    match values {
        [Value::Number(p, _)] => Ok(table.at(*p)),
        [v] => err(Kind::ArgumentType, format!("a progress table is called with a Number (0..1), found {}", v.type_name())),
        _ => err(Kind::ArityMismatch, format!("a progress table takes 1 argument, {} given", values.len())),
    }
}

/// builtin 関数。log と type_of は本体、ほかは math
fn call_builtin(name: &'static str, values: Vec<Value>) -> Result<Value> {
    match name {
        "log" => {
            eprintln!("{}", values.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "));
            Ok(Value::Nothing)
        }
        "type_of" => match values.as_slice() {
            [v] => Ok(Value::Str(v.type_name())),
            _ => err(Kind::ArityMismatch, format!("type_of takes 1 argument, {} given", values.len())),
        },
        "linear" | "ease_in" | "ease_out" | "ease" => crate::lang::curve::call(name, &values),
        name => call_native(name, &values),
    }
}

/// native モジュールの関数。前置きのある名前はそのモジュールへ回す
fn call_native(name: &str, values: &[Value]) -> Result<Value> {
    match name.split_once('.') {
        Some(("space3d", f)) => stdlib::space3d::call(f, values),
        Some(("bignum", f)) => stdlib::bignum::call(f, values),
        _ => stdlib::math::call(name, values),
    }
}

/// 属性が見つからないときの言い方。module だけ言い方を変える
fn no_attr(target: &Value, attr: &str) -> String {
    match target {
        Value::Module(m) => format!("module {} has no item \"{attr}\"", m.name),
        Value::Record(r) => format!("{} has no field \"{attr}\"", r.name),
        v => format!("{} has no attribute \"{attr}\"", v.type_name()),
    }
}

/// 属性の書き込みが、その場で済んだか (参照型) 作り直しになったか (値型)
fn same_place(before: &Value, after: &Value) -> bool {
    match (before, after) {
        (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

/// 属性の型に値が合うか。builtin の Union (Paint = Color | Shader) もここで見る
/// builtin 型の名前 (補完用)
/// font に書いた候補。String 1 つなら 1 つ、List なら中の String を順に。
/// 書体は「あれば使う」ものなので、String でないものは黙って飛ばす
pub fn font_names(v: &Value) -> Vec<String> {
    match v {
        Value::Str(name) => vec![name.clone()],
        Value::List(items) => items.borrow().iter().filter_map(|i| match i {
            Value::Str(name) => Some(name.clone()),
            _ => None,
        }).collect(),
        _ => Vec::new(),
    }
}

/// while を 1 回の評価で回せる上限。超えたら止まらない繰り返しとみなす
const LOOP_LIMIT: usize = 1_000_000;

/// Group(...) の引数が、Group 自身の属性か。そうでなければ中身
pub const KINDS: &[&str] =
    &["Circle", "Ellipse", "Rect", "Line", "Polygon", "Path", "TextArea", "Composition", "Narration", "SayVoiceEngine", "EspeakVoiceEngine", "Shader", "ZoomPath", "Camera", "Gradient", "Color", "PerspectiveCamera", "OrthographicCamera", "IsometricCamera", "Transform3", "Mesh", "Face", "Solid", "World", "Group"];

/// builtin 型の属性と型
/// 型の名前は大文字で始まり、builtin の型と union の名前は使えない
fn check_free_name(name: &str) -> Result<()> {
    if !name.starts_with(char::is_uppercase) {
        return err(Kind::Reserved, format!("a type name must start with an uppercase letter; \"{name}\" does not"));
    }
    if crate::docs::types().iter().any(|t| t.name == name) {
        return err(Kind::Reserved, format!("\"{name}\" is a builtin type and cannot be redeclared"));
    }
    Ok(())
}

/// 書かなくてもその値で描かれる属性。ここが既定の唯一の定義で、構築時に入れておく。
/// 既定が「何もしない」もの (fill / stroke / pivot / w / h / font) は入れない
/// 輪郭を長さの割合でたどった点。u は 0..1 に丸める
fn point_at(path: &vello::kurbo::BezPath, u: f64) -> Value {
    use vello::kurbo::{ParamCurve, ParamCurveArclen};
    let segs: Vec<_> = path.segments().collect();
    let lengths: Vec<f64> = segs.iter().map(|s| s.arclen(1e-6)).collect();
    let total: f64 = lengths.iter().sum();
    let Some(first) = segs.first() else { return Value::Vector(0.0, 0.0) };
    if !(total > 0.0) {
        let p = first.eval(0.0);
        return Value::Vector(p.x, p.y);
    }
    let mut want = u.clamp(0.0, 1.0) * total;
    for (seg, len) in segs.iter().zip(&lengths) {
        if want <= *len || *len <= 0.0 {
            let p = seg.eval(seg.inv_arclen(want.max(0.0), 1e-6));
            return Value::Vector(p.x, p.y);
        }
        want -= len;
    }
    let p = segs.last().expect("not empty").eval(1.0);
    Value::Vector(p.x, p.y)
}

pub fn defaults(kind: &str) -> Vec<(&'static str, Value)> {
    let num = |n: f64| Value::num(n);
    // 3D のカメラ。up は y が上、fov は度
    match kind {
        "PerspectiveCamera" => return vec![("up", Value::Vector3(0.0, 1.0, 0.0)), ("fov", num(std::f64::consts::FRAC_PI_4))],
        "OrthographicCamera" => return vec![("up", Value::Vector3(0.0, 1.0, 0.0)), ("height", num(8.0))],
        "IsometricCamera" => return vec![("unit", num(1.0))],
        "Transform3" => return vec![("m", stdlib::space3d::identity())],
        "Solid" => return vec![("fill", Value::Color([0.5, 0.5, 0.5, 1.0]))],
        "World" => return vec![("light", Value::Vector3(0.4, 0.8, 0.5)), ("ambient", num(0.25))],
        _ => {}
    }
    let sym = |s: &str| Value::Symbol(s.to_string());
    let mut out: Vec<(&'static str, Value)> = Vec::new();
    if schema(kind).is_some_and(|s| s.iter().any(|a| a.name == "opacity")) {
        out.push(("opacity", num(1.0)));
    }
    let has = |name: &str| schema(kind).is_some_and(|s| s.iter().any(|a| a.name == name));
    if has("rotation") {
        out.push(("rotation", num(0.0)));
    }
    if has("scale") {
        out.push(("scale", num(1.0)));
    }
    if has("position") {
        out.push(("position", Value::Vector(0.0, 0.0)));
    }
    if has("blend") {
        out.push(("blend", sym("normal")));
    }
    if has("strokeWidth") {
        out.push(("strokeWidth", num(0.01)));
    }
    if has("strokeCap") {
        out.push(("strokeCap", sym("butt")));
    }
    if has("strokeJoin") {
        out.push(("strokeJoin", sym("miter")));
    }
    if has("dashOffset") {
        out.push(("dashOffset", num(0.0)));
    }
    match kind {
        "Rect" => out.push(("radius", num(0.0))),
        "Path" => out.push(("closed", Value::Bool(false))),
        "TextArea" => out.push(("align", sym("left"))),
        "Shader" => out.push(("samples", num(1.0))),
        _ => {}
    }
    out
}

/// 型が持つ属性 1 つ。`required` は「書かないと描けない」もの
#[derive(Clone, Copy)]
pub struct Attr {
    pub name: &'static str,
    pub ty: &'static str,
    /// 書かないと描くときに `NameError.UndefinedAttribute` か `TypeError.AttributeType` になる
    pub required: bool,
}

pub const fn opt(name: &'static str, ty: &'static str) -> Attr {
    Attr { name, ty, required: false }
}

pub const fn req(name: &'static str, ty: &'static str) -> Attr {
    Attr { name, ty, required: true }
}

/// schema に書いた `List<Vector>` のような型名を注釈にしたもの。
/// 名前は 'static で数も限られるので、はじめに 1 度だけ作って引き回す
fn schema_anns() -> &'static HashMap<&'static str, TypeAnn> {
    static ALL: std::sync::OnceLock<HashMap<&'static str, TypeAnn>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| {
        let mut out = HashMap::new();
        for kind in KINDS {
            for a in schema(kind).unwrap_or(&[]).iter().filter(|a| a.ty.contains('<')) {
                let ann = crate::lang::parser::parse_type(a.ty).unwrap_or_else(|_| panic!("schema の型 \"{}\" が読めない", a.ty));
                out.insert(a.ty, ann);
            }
        }
        out
    })
}

pub fn schema(kind: &str) -> Option<&'static [Attr]> {
    // 図形が共通で持つ属性。ただし、その図形で効きようがないものは持たせない
    //   strokeJoin — 角のある図形だけ (Circle / Ellipse / Line には角が無い)
    //   fill        — 面のある図形だけ (Line には面が無い)
    //   strokeCap / dash / dashOffset — 字の輪郭は破線にできないので TextArea は持たない
    const PAINT: Attr = opt("fill", "Paint");
    const LINE: [Attr; 5] = [opt("stroke", "Color"), opt("strokeWidth", "Length"), opt("strokeCap", "StrokeCap"), opt("dash", "List<Number>"), opt("dashOffset", "Number")];
    const JOIN: Attr = opt("strokeJoin", "StrokeJoin");
    // すべての Node が持つ transform 一式と blend。anchor は Node 自身の座標の基準点、position はそれを置く場所
    const COMMON: [Attr; 6] = [opt("opacity", "Number"), opt("rotation", "Number"), opt("scale", "Number"), opt("anchor", "Vector"), opt("position", "Vector"), opt("blend", "Blend")];
    macro_rules! shape {
        (fill: $fill:literal, join: $join:literal, $($extra:expr),*) => {{
            const ATTRS: &[Attr] = &[
                $($extra,)*
                PAINT, LINE[0], LINE[1], LINE[2], LINE[3], LINE[4], JOIN,
                COMMON[0], COMMON[1], COMMON[2], COMMON[3], COMMON[4], COMMON[5],
            ];
            const NO_FILL: &[Attr] = &[
                $($extra,)*
                LINE[0], LINE[1], LINE[2], LINE[3], LINE[4],
                COMMON[0], COMMON[1], COMMON[2], COMMON[3], COMMON[4], COMMON[5],
            ];
            const NO_JOIN: &[Attr] = &[
                $($extra,)*
                PAINT, LINE[0], LINE[1], LINE[2], LINE[3], LINE[4],
                COMMON[0], COMMON[1], COMMON[2], COMMON[3], COMMON[4], COMMON[5],
            ];
            match ($fill, $join) {
                (true, true) => ATTRS,
                (true, false) => NO_JOIN,
                _ => NO_FILL,
            }
        }};
    }
    // 関数呼び出しは 'static に上げてもらえないので、それぞれ const にする
    const TEXT_AREA: &[Attr] = &[
        req("text", "String"),
        req("fontSize", "Length"),
        opt("w", "Length"),
        opt("font", "Font"),
        opt("align", "Align"),
        opt("anchorRatio", "Vector"),
        PAINT,
        LINE[0],
        LINE[1],
        JOIN,
        COMMON[0],
        COMMON[1],
        COMMON[2],
        COMMON[3],
        COMMON[4],
        COMMON[5],
    ];
    const COMPOSITION: &[Attr] = &[
        req("box", "Vector"),
        req("duration", "Duration"),
        COMMON[0],
        COMMON[1],
        COMMON[2],
        COMMON[3],
        COMMON[4],
        COMMON[5],
    ];
    // 読み上げ。voice は engine に渡す声の名前、volume は混ぜるときの音量
    const NARRATION: &[Attr] = &[req("text", "String"), opt("voice", "Voice"), opt("volume", "Number")];
    const SHADER: &[Attr] = &[req("color", "Func"), opt("args", "Numbers"), opt("samples", "Number"), opt("zoom", "ZoomPath"), opt("camera", "Camera")];
    // 倍率の表は zoom と duration と unit から作る (作るのは construct)。scale は作った結果
    const ZOOM_MAP: &[Attr] =
        &[req("center", "Vector"), req("zoom", "Func"), req("duration", "Duration"), opt("unit", "Number"), opt("scale", "List")];
    // 箱の中身の寄り引き。from を to に、scale 倍で置く
    const CAMERA: &[Attr] = &[req("from", "Vector"), opt("to", "Vector"), opt("scale", "Number")];
    // 3D のカメラ。box は投影先の箱の大きさ (View の box と同じ)
    const PERSPECTIVE: &[Attr] =
        &[req("from", "Vector3"), req("to", "Vector3"), opt("up", "Vector3"), opt("fov", "Number"), req("box", "Vector")];
    const ORTHOGRAPHIC: &[Attr] =
        &[req("from", "Vector3"), req("to", "Vector3"), opt("up", "Vector3"), opt("height", "Number"), req("box", "Vector")];
    const ISOMETRIC: &[Attr] = &[opt("unit", "Number"), req("box", "Vector")];
    const TRANSFORM3: &[Attr] = &[opt("m", "List<Number>")];
    const MESH: &[Attr] = &[req("points", "List<Vector3>"), opt("edges", "List<Tuple>"), opt("faces", "List<List<Number>>")];
    const GROUP: &[Attr] = &[COMMON[0], COMMON[1], COMMON[2], COMMON[3], COMMON[4], COMMON[5], opt("camera", "Camera"), opt("clip", "Shape")];
    const SOLID: &[Attr] = &[req("mesh", "Mesh"), opt("fill", "Color"), opt("transform", "Transform3")];
    const WORLD: &[Attr] =
        &[req("camera", "Projection"), req("parts", "List<Solid>"), opt("light", "Vector3"), opt("ambient", "Number"), opt("background", "Color")];
    const FACE: &[Attr] = &[req("points", "List<Vector>"), req("normal", "Vector3"), req("depth", "Number")];
    // to は :linear、radius は :radial のときだけ要るので、必須にはしない
    const GRADIENT: &[Attr] = &[
        req("stops", "List<Color>"),
        req("from", "Vector"),
        opt("kind", "GradientKind"),
        opt("to", "Vector"),
        opt("radius", "Number"),
    ];
    Some(match kind {
        "Circle" => shape!(fill: true, join: false, req("radius", "Length")),
        "Ellipse" => shape!(fill: true, join: false, req("rx", "Length"), req("ry", "Length")),
        "Rect" => shape!(fill: true, join: true, req("w", "Length"), req("h", "Length"), opt("radius", "Length")),
        "Line" => shape!(fill: false, join: false, req("from", "Vector"), req("to", "Vector")),
        "Polygon" => shape!(fill: true, join: true, req("points", "List<Vector>")),
        "Path" => shape!(fill: true, join: true, req("from", "Vector"), req("segments", "List<Tuple>"), opt("closed", "Bool"), opt("upto", "Number")),
        "TextArea" => TEXT_AREA,
        "Composition" => COMPOSITION,
        "Narration" => NARRATION,
        // engine の型。属性はその engine が持っている
        name if crate::render::voice::by_name(name).is_some() => crate::render::voice::by_name(name).expect("just checked").attrs(),
        "Shader" => SHADER,
        "ZoomPath" => ZOOM_MAP,
        "Camera" => CAMERA,
        "PerspectiveCamera" => PERSPECTIVE,
        "OrthographicCamera" => ORTHOGRAPHIC,
        "IsometricCamera" => ISOMETRIC,
        "Transform3" => TRANSFORM3,
        "Mesh" => MESH,
        "Face" => FACE,
        "Solid" => SOLID,
        "Group" => GROUP,
        "World" => WORLD,
        "Gradient" => GRADIENT,
        _ => return None,
    })
}

/// 2 つの値の間。k は 0..1
/// 途中の数は分数で持たない。書いた値の間を取ったものなので分数で表せることはまずなく、
/// 毎フレーム何千回も作るところなので、分数を求める手間を掛けない
pub(crate) fn interpolate(a: &Value, b: &Value, k: f64) -> Value {
    match (a, b) {
        (Value::Number(x, _), Value::Number(y, _)) => Value::Number(x + (y - x) * k, None),
        (Value::Duration(x), Value::Duration(y)) => Value::Duration(x + (y - x) * k),
        // 単位つきの長さは、単位ごとの係数を補間する
        (Value::Length(_), Value::Length(_) | Value::Number(..)) | (Value::Number(..), Value::Length(_)) => {
            let (x, y) = (a.as_length().expect("length"), b.as_length().expect("length"));
            Value::Length(x.plus(y.minus(x).scaled(k)))
        }
        (Value::LVec(..), Value::LVec(..) | Value::Vector(..)) | (Value::Vector(..), Value::LVec(..)) => {
            let ((x0, y0), (x1, y1)) = (a.as_lvec().expect("vector"), b.as_lvec().expect("vector"));
            Value::LVec(x0.plus(x1.minus(x0).scaled(k)), y0.plus(y1.minus(y0).scaled(k)))
        }
        (Value::Vector(x0, y0), Value::Vector(x1, y1)) => Value::Vector(x0 + (x1 - x0) * k, y0 + (y1 - y0) * k),
        (Value::Color(c0), Value::Color(c1)) => {
            let k = k as f32;
            Value::Color(std::array::from_fn(|i| c0[i] + (c1[i] - c0[i]) * k))
        }
        // 並びは要素ごとに。Path の segments や Polygon の points の形が変わる。
        // 長さが違えば形が対応しないので、補間せず元のままにする
        (Value::Tuple(x), Value::Tuple(y)) if x.len() == y.len() => Value::Tuple(x.iter().zip(y).map(|(a, b)| interpolate(a, b, k)).collect()),
        (Value::List(x), Value::List(y)) if x.borrow().len() == y.borrow().len() => {
            let out = x.borrow().iter().zip(y.borrow().iter()).map(|(a, b)| interpolate(a, b, k)).collect();
            Value::List(Rc::new(RefCell::new(out)))
        }
        _ => a.clone(),
    }
}


/// 単位つきの長さが絡む四則。長さは係数の和なので、箱が決まる前に計算できる
fn length_arith(op: BinOp, l: &Value, r: &Value) -> Result<Option<Value>> {
    let involved = |v: &Value| matches!(v, Value::Length(_) | Value::LVec(..));
    if !(involved(l) || involved(r)) || !matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) {
        return Ok(None);
    }
    let bad = || MophError::new(Kind::OperandType, format!("cannot {} {} and {}", verb(op), l.type_name(), r.type_name()));
    let number = |v: &Value| match v {
        Value::Number(k, _) => Some(*k),
        _ => None,
    };
    // 数との掛け割り。(掛けられる側, 倍率) に直す
    let scale = match (op, number(l), number(r)) {
        (BinOp::Mul, Some(k), None) => Some((r, k)),
        (BinOp::Mul, None, Some(k)) => Some((l, k)),
        (BinOp::Div, _, Some(k)) if k == 0.0 => return err(Kind::DivisionByZero, "division by zero"),
        (BinOp::Div, None, Some(k)) => Some((l, 1.0 / k)),
        _ => None,
    };
    if let Some((target, k)) = scale {
        return match (target.as_length(), target.as_lvec()) {
            (Some(len), _) => Ok(Some(Value::Length(len.scaled(k)))),
            (_, Some((x, y))) => Ok(Some(Value::vector_of(x.scaled(k), y.scaled(k)))),
            _ => Err(bad()),
        };
    }
    // 足し引き。Number は単位なしの長さ
    match (op, l.as_length(), r.as_length(), l.as_lvec(), r.as_lvec()) {
        (BinOp::Add, Some(a), Some(b), ..) => Ok(Some(Value::Length(a.plus(b)))),
        (BinOp::Sub, Some(a), Some(b), ..) => Ok(Some(Value::Length(a.minus(b)))),
        (BinOp::Add, _, _, Some((ax, ay)), Some((bx, by))) => Ok(Some(Value::vector_of(ax.plus(bx), ay.plus(by)))),
        (BinOp::Sub, _, _, Some((ax, ay)), Some((bx, by))) => Ok(Some(Value::vector_of(ax.minus(bx), ay.minus(by)))),
        _ => Err(bad()),
    }
}

fn binary(op: BinOp, l: Value, r: Value) -> Result<Value> {
    use Value::{Bool, Duration, Number};
    if let Some(v) = length_arith(op, &l, &r)? {
        return Ok(v);
    }
    // Vector は実数 2 つの Tuple と同じに計算し、結果を Vector に戻す (Vector ± Vector / (x, y)、Vector × ÷ Number)
    let is_vector = |v: &Value| matches!(v, Value::Vector(..) | Value::Vector3(..));
    if (is_vector(&l) || is_vector(&r)) && matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) {
        let as_tuple = |v: &Value| match v {
            Value::Vector(x, y) => Value::Tuple(vec![Value::num(*x), Value::num(*y)]),
            Value::Vector3(x, y, z) => Value::Tuple(vec![Value::num(*x), Value::num(*y), Value::num(*z)]),
            other => other.clone(),
        };
        let result = binary(op, as_tuple(&l), as_tuple(&r)).map_err(|e| match e.kind {
            Kind::OperandType => MophError::new(Kind::OperandType, format!("cannot {} {} and {}", verb(op), l.type_name(), r.type_name())),
            _ => e,
        })?;
        return Ok(match result {
            Value::Tuple(items) => match items.as_slice() {
                [Number(x, _), Number(y, _)] => Value::Vector(*x, *y),
                [Number(x, _), Number(y, _), Number(z, _)] if is_vector(&l) || is_vector(&r) => Value::Vector3(*x, *y, *z),
                _ => Value::Tuple(items),
            },
            other => other,
        });
    }
    // 両辺が分数のまま持てていれば、分数で計算する。表せなくなったら実数に落ちる
    let num = |f: fn(f64, f64) -> f64| -> Option<Value> {
        match (&l, &r) {
            (Number(a, ra), Number(b, rb)) => Some(match (ra, rb) {
                (Some(x), Some(y)) => exact(op, *x, *y).unwrap_or_else(|| Value::num(f(*a, *b))),
                _ => Value::num(f(*a, *b)),
            }),
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
            (Duration(a), Number(b, _)) | (Number(b, _), Duration(a)) => Some(Duration(a * b)),
            (Value::Tuple(items), Number(k, _)) | (Number(k, _), Value::Tuple(items)) => scale_tuple(items, *k, BinOp::Mul)?,
            (Value::Color(c), Number(k, _)) | (Number(k, _), Value::Color(c)) => {
                let k = *k as f32;
                check_color(Some(Value::Color(std::array::from_fn(|i| c[i] * k))))?
            }
            _ => num(|a, b| a * b),
        },
        BinOp::Div => match (&l, &r) {
            (_, Number(b, _)) | (_, Duration(b)) if *b == 0.0 => return err(Kind::DivisionByZero, "division by zero"),
            (Duration(a), Number(b, _)) => Some(Duration(a / b)),
            (Duration(a), Duration(b)) => Some(Value::num(a / b)),
            (Value::Tuple(items), Number(k, _)) => scale_tuple(items, *k, BinOp::Div)?,
            _ => num(|a, b| a / b),
        },
        BinOp::Rem => match (&l, &r) {
            (_, Number(b, _)) if *b == 0.0 => return err(Kind::DivisionByZero, "division by zero"),
            _ => num(|a, b| a % b),
        },
        BinOp::Pow => num(f64::powf),
        BinOp::Range | BinOp::RangeInclusive => match (&l, &r) {
            (Number(a, _), Number(b, _)) => {
                let (a, b) = (whole(*a, "Range.start")?, whole(*b, "Range.end")?);
                Some(Value::Range(a, b + i64::from(op == BinOp::RangeInclusive)))
            }
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
    result.ok_or_else(|| MophError::new(Kind::OperandType, format!("cannot {} {} and {}", verb(op), l.type_name(), r.type_name())))
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
/// 分数どうしの計算。桁が溢れたら諦めて実数に任せる
fn exact(op: BinOp, a: Ratio, b: Ratio) -> Option<Value> {
    let (num, den) = match op {
        BinOp::Add => (a.num.checked_mul(b.den)?.checked_add(b.num.checked_mul(a.den)?)?, a.den.checked_mul(b.den)?),
        BinOp::Sub => (a.num.checked_mul(b.den)?.checked_sub(b.num.checked_mul(a.den)?)?, a.den.checked_mul(b.den)?),
        BinOp::Mul => (a.num.checked_mul(b.num)?, a.den.checked_mul(b.den)?),
        BinOp::Div => (a.num.checked_mul(b.den)?, a.den.checked_mul(b.num)?),
        // 指数が整数のときだけ
        BinOp::Pow if b.den == 1 && (0..=32).contains(&b.num) => {
            let n = b.num as u32;
            (a.num.checked_pow(n)?, a.den.checked_pow(n)?)
        }
        _ => return None,
    };
    Value::ratio(num, den)
}

fn zip_tuples(a: &[Value], b: &[Value], op: BinOp) -> Result<Option<Value>> {
    if a.len() != b.len() {
        return err(Kind::ArityMismatch, format!("tuples have different lengths: {} and {}", a.len(), b.len()));
    }
    let items = a.iter().zip(b).map(|(x, y)| binary(op, x.clone(), y.clone())).collect::<Result<Vec<_>>>()?;
    Ok(Some(Value::Tuple(items)))
}

fn scale_tuple(items: &[Value], k: f64, op: BinOp) -> Result<Option<Value>> {
    let items = items.iter().map(|x| binary(op, x.clone(), Value::num(k))).collect::<Result<Vec<_>>>()?;
    Ok(Some(Value::Tuple(items)))
}

/// Color の各チャンネルが 0..1 に収まっているか
fn check_color(v: Option<Value>) -> Result<Option<Value>> {
    if let Some(Value::Color(c)) = &v {
        if c.iter().any(|x| !(0.0..=1.0).contains(x)) {
            return err(Kind::OutOfRange, "color channel exceeds 1.0");
        }
    }
    Ok(v)
}

/// 大小を比べる。比べようがない組み合わせは None (`<` はエラー、sort もエラー)
pub(crate) fn cmp(l: &Value, r: &Value) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    match (l, r) {
        (Value::Number(a, _), Value::Number(b, _)) | (Value::Duration(a), Value::Duration(b)) => a.partial_cmp(b),
        // 文字は Unicode の順。日本語の五十音順にはならない
        (Value::Str(a), Value::Str(b)) | (Value::Symbol(a), Value::Symbol(b)) => Some(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        // 組は前から順に。先に差が付いた所で決まる (長さが違えば短い方が先)
        (Value::Tuple(a), Value::Tuple(b)) => seq_cmp(a, b),
        (Value::List(a), Value::List(b)) => seq_cmp(&a.borrow(), &b.borrow()),
        // record はフィールドを宣言順に。型が違えば比べない
        (Value::Record(a), Value::Record(b)) if a.name == b.name => {
            for ((_, x), (_, y)) in a.fields.iter().zip(&b.fields) {
                match cmp(x, y)? {
                    Ordering::Equal => {}
                    other => return Some(other),
                }
            }
            Some(Ordering::Equal)
        }
        _ => None,
    }
}

/// 並びを前から比べる
fn seq_cmp(a: &[Value], b: &[Value]) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    for (x, y) in a.iter().zip(b) {
        match cmp(x, y)? {
            Ordering::Equal => {}
            other => return Some(other),
        }
    }
    Some(a.len().cmp(&b.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// xs[i] が並び全体を複製していて、n 回引くと O(n²) になっていた。
    /// 直す前は 5 万要素で 15 秒ほど掛かっていたので、余裕を見て 2 秒で切る
    #[test]
    fn indexing_a_list_does_not_copy_it() {
        let n = 50_000;
        let list = Value::List(Rc::new(RefCell::new((0..n).map(|i| Value::num(f64::from(i))).collect())));
        let started = std::time::Instant::now();
        let mut sum = 0.0;
        for i in 0..n {
            let Ok(Value::Number(v, _)) = index_value(&list, &Value::num(f64::from(i))) else {
                panic!("index {i} failed");
            };
            sum += v;
        }
        assert_eq!(sum, f64::from(n) * f64::from(n - 1) / 2.0);
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "indexing took {:?}", started.elapsed());
    }

    /// while は止まらないことがあるので、回数に上限を置いて知らせる
    #[test]
    fn an_endless_while_stops_with_an_error() {
        let src = "let n = 0\nwhile true { n = n + 1 }\n";
        let stmts = crate::lang::parser::parse(src).expect("parses");
        let err = Interp::new().run(&stmts).expect_err("the loop never ends");
        assert_eq!(err.kind, Kind::EndlessLoop, "got {err}");
    }

    /// TextArea の anchor は絶対値か割合のどちらかで、両方は書けない
    #[test]
    fn a_text_area_takes_anchor_or_anchor_ratio_not_both() {
        let run = |src: &str| Interp::new().run(&crate::lang::parser::parse(src).expect("parses"));
        assert!(run("TextArea(text = \"a\", fontSize = 1, anchorRatio = Vector(1, 0))").is_ok());
        let err = run("TextArea(text = \"a\", fontSize = 1, anchor = Vector(0, 0), anchorRatio = Vector(1, 0))").expect_err("both");
        assert_eq!(err.kind, Kind::ArgumentType, "got {err}");
        let err = run("TextArea(text = \"a\", fontSize = 1, anchor = Vector(0, 0)).copy(anchorRatio = Vector(1, 0))").expect_err("both through copy");
        assert_eq!(err.kind, Kind::ArgumentType, "got {err}");
    }

    /// スクリプトを実行し、output した Composition と一緒に返す
    fn composition(src: &str) -> (Interp, ObjRef) {
        let stmts = crate::lang::parser::parse(src).expect("parses");
        let mut interp = Interp::new();
        interp.run(&stmts).expect("runs");
        let comp = interp.output.clone().expect("output");
        (interp, comp)
    }

    fn attr_of(node: &ObjRef, name: &str) -> Value {
        node.borrow().attrs.get(name).cloned().unwrap_or_else(|| panic!("no attribute {name}"))
    }

    /// Layer は in <= t < out の間だけ存在する
    #[test]
    fn a_layer_exists_only_between_in_and_out() {
        let (mut interp, comp) = composition(
            "let comp = Composition(box = Vector(16, 9), duration = 6s)\n\
             comp.place(\"a\", Circle(radius = 1), in = 1s, out = 3s)\n\
             comp.place(\"b\", Circle(radius = 2), in = 2s, out = 5s)\n\
             output comp\n",
        );
        let count = |interp: &mut Interp, t: f64| interp.frame(&comp, t).expect("frame").borrow().children.len();
        assert_eq!([0.5, 1.0, 2.5, 3.0, 4.9, 5.0].map(|t| count(&mut interp, t)), [0, 1, 2, 1, 1, 0]);
    }

    /// 関数の Layer は、自分の in を 0 とした時刻で呼ばれる
    #[test]
    fn a_function_layer_gets_the_time_since_its_in() {
        let (mut interp, comp) = composition(
            "func grow(t) { Circle(radius = t / 1s) }\n\
             let comp = Composition(box = Vector(16, 9), duration = 6s)\n\
             comp.place(\"grow\", grow, in = 2s, out = 5s)\n\
             output comp\n",
        );
        let frame = interp.frame(&comp, 3.5).expect("frame");
        let circle = frame.borrow().children[0].clone();
        assert!(matches!(attr_of(&circle, "radius"), Value::Number(r, _) if r == 1.5));
    }

    #[test]
    fn placing_a_name_twice_is_an_error() {
        let stmts = crate::lang::parser::parse(
            "let comp = Composition(box = Vector(16, 9), duration = 6s)\n\
             comp.place(\"a\", Circle(radius = 1), in = 0s, out = 1s)\n\
             comp.place(\"a\", Circle(radius = 1), in = 0s, out = 1s)\n",
        )
        .expect("parses");
        let err = Interp::new().run(&stmts).expect_err("a name is placed once");
        assert_eq!(err.kind, Kind::AlreadyPlaced, "got {err}");
    }

    #[test]
    fn a_layer_needs_in_and_out_and_a_span() {
        for tail in ["", ", in = 1s", ", in = 2s, out = 1s", ", in = 1s, out = 1s"] {
            let src = format!("let comp = Composition(box = Vector(16, 9), duration = 6s)\ncomp.place(\"a\", Circle(radius = 1){tail})\n");
            let stmts = crate::lang::parser::parse(&src).expect("parses");
            assert!(Interp::new().run(&stmts).is_err(), "accepted: {tail}");
        }
    }

    /// 図形は不変。変えたものは copy で作り、元は変わらない
    #[test]
    fn shapes_are_immutable_and_copy_makes_a_changed_one() {
        let stmts = crate::lang::parser::parse("let c = Circle(radius = 1)\nc.radius = 2\n").expect("parses");
        let err = Interp::new().run(&stmts).expect_err("a shape cannot be assigned");
        assert_eq!(err.kind, Kind::AttributeType, "got {err}");

        let stmts = crate::lang::parser::parse("let c = Circle(radius = 1, opacity = 0.5)\nlet d = c.copy(radius = 3)\n").expect("parses");
        let mut interp = Interp::new();
        interp.run(&stmts).expect("runs");
        let (Value::Object(c), Value::Object(d)) = (interp.lookup("c").expect("c"), interp.lookup("d").expect("d")) else { panic!("objects") };
        assert!(matches!(attr_of(&c, "radius"), Value::Number(r, _) if r == 1.0));
        assert!(matches!(attr_of(&d, "radius"), Value::Number(r, _) if r == 3.0));
        assert!(matches!(attr_of(&d, "opacity"), Value::Number(o, _) if o == 0.5));
    }

    /// Group は名前の無い引数が中身
    #[test]
    fn a_group_holds_its_unnamed_arguments() {
        let stmts = crate::lang::parser::parse("let g = Group(Circle(radius = 1), Rect(w = 1, h = 1), position = Vector(2, 3))\n").expect("parses");
        let mut interp = Interp::new();
        interp.run(&stmts).expect("runs");
        let Value::Object(g) = interp.lookup("g").expect("g") else { panic!("object") };
        assert_eq!(g.borrow().children.len(), 2);
        assert!(matches!(attr_of(&g, "position"), Value::Vector(x, y) if x == 2.0 && y == 3.0));
    }

    /// 入れ子の Composition は、自分の Layer を同じ規則で並べる
    #[test]
    fn a_nested_composition_keeps_its_own_time() {
        let (mut interp, comp) = composition(
            "let inner = Composition(box = Vector(4, 4), duration = 2s)\n\
             inner.place(\"dot\", Circle(radius = 1), in = 1s, out = 2s)\n\
             let comp = Composition(box = Vector(16, 9), duration = 8s)\n\
             comp.place(\"inner\", inner, in = 3s, out = 5s)\n\
             output comp\n",
        );
        let count = |interp: &mut Interp, t: f64| {
            let frame = interp.frame(&comp, t).expect("frame");
            let group = frame.borrow().children.first().cloned();
            group.map(|g| g.borrow().children.len())
        };
        assert_eq!([2.0, 3.5, 4.0, 5.5].map(|t| count(&mut interp, t)), [None, Some(0), Some(1), None]);
    }

    /// 入れ子の Composition は、自分の duration を過ぎると消える
    #[test]
    fn a_nested_composition_vanishes_after_its_duration() {
        let (mut interp, comp) = composition(
            "let inner = Composition(box = Vector(4, 4), duration = 2s)\n\
             inner.place(\"dot\", Circle(radius = 1), in = 0s, out = 2s)\n\
             let comp = Composition(box = Vector(16, 9), duration = 8s)\n\
             comp.place(\"inner\", inner, in = 0s, out = 8s)\n\
             output comp\n",
        );
        let count = |interp: &mut Interp, t: f64| interp.frame(&comp, t).expect("frame").borrow().children.len();
        assert_eq!([1.0, 1.9, 2.0, 5.0].map(|t| count(&mut interp, t)), [1, 1, 0, 0]);
    }

    /// 1 列の分解は `let (x) = ...` と書く
    #[test]
    fn a_one_column_destructure_is_written_with_parentheses() {
        let stmts = crate::lang::parser::parse("let (x) = (3,)\n").expect("parses");
        let mut interp = Interp::new();
        interp.run(&stmts).expect("runs");
        assert!(matches!(interp.lookup("x"), Ok(Value::Number(n, _)) if n == 3.0));
    }

    /// 標準ライブラリは実行ファイルに埋め込んであり、ディスクには無い。
    /// 何ファイルかに分けたモジュールは、その中で相対 import を解けないと読めない。
    /// 素材は 3 つとも、名前を出すだけの index.moph が束ねている
    #[test]
    fn a_stdlib_module_can_be_split_across_files() {
        let src = "import palette\nimport pattern\nimport icon\n\
            log(palette.NIGHT.ink, palette.HOUSE.accent)\n\
            log(pattern.STRIPES, pattern.dots(palette.NIGHT.back, palette.NIGHT.accent))\n\
            log(icon.mark(icon.SEARCH, Vector(8, 4.5), 1, palette.HOUSE.ink))\n";
        let stmts = crate::lang::parser::parse(src).expect("parses");
        let mut interp = Interp::new();
        // 呼ぶ側の置き場は、標準ライブラリの中の import と関係が無い
        interp.base_dir = PathBuf::from("/nowhere");
        interp.run(&stmts).expect("import palette");
    }
}
