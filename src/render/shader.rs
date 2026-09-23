//! Shader: 図形の塗りを「位置と時刻から色を返す関数」で決める。
//! 関数 (mophila の数値の部分) を WGSL に変換し、compute shader で図形の範囲のピクセルを計算して、その画像で塗る。
//! 変換できるのは Number / Bool / Color / Vector と、四則・比較・論理、if、範囲の for、return、math、
//! 外側の変数 (Number / Color / Bool / Vector と、それらの List、関数)。図形や文字列は扱えない

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};
use vello::wgpu;

use crate::lang::ast::{BinOp, Expr, Pattern, Stmt, StmtKind};
use crate::lang::error::{Kind, MophError, Result, err};
use crate::lang::value::{Closure, Value};

const KIND: Kind = Kind::ShaderCompile;

// ---------- WGSL への変換 ----------

/// WGSL 側の型
#[derive(Clone, Copy, PartialEq, Debug)]
enum Ty {
    Num,
    Bool,
    Color,
    Vec,
    /// Number の配列。None は長さが実行時に決まる args
    Nums(Option<usize>),
    Colors(usize),
    Vecs(usize),
    Nothing,
}

impl Ty {
    fn wgsl(self) -> Result<String> {
        Ok(match self {
            Ty::Num => "f32".into(),
            Ty::Bool => "bool".into(),
            Ty::Color => "vec4<f32>".into(),
            Ty::Vec => "vec2<f32>".into(),
            Ty::Nums(Some(n)) => format!("array<f32, {n}>"),
            Ty::Colors(n) => format!("array<vec4<f32>, {n}>"),
            Ty::Vecs(n) => format!("array<vec2<f32>, {n}>"),
            Ty::Nums(None) => return err(KIND, "args cannot be copied into a variable; index it directly"),
            Ty::Nothing => return err(KIND, "this expression has no value"),
        })
    }

    fn name(self) -> String {
        match self {
            Ty::Num => "Number".into(),
            Ty::Bool => "Bool".into(),
            Ty::Color => "Color".into(),
            Ty::Vec => "Vector".into(),
            Ty::Nums(_) | Ty::Colors(_) | Ty::Vecs(_) => "List".into(),
            Ty::Nothing => "Nothing".into(),
        }
    }
}

/// 出力中の関数本体
struct Body {
    lines: Vec<String>,
    indent: usize,
}

impl Body {
    fn new(indent: usize) -> Self {
        Self { lines: Vec::new(), indent }
    }

    fn line(&mut self, s: impl Into<String>) {
        self.lines.push(format!("{}{}", "    ".repeat(self.indent), s.into()));
    }

    fn append(&mut self, other: Body) {
        self.lines.extend(other.lines);
    }
}

/// mophila の名前 → (WGSL の名前, 型)
type Env = Vec<HashMap<String, (String, Ty)>>;

fn lookup(env: &Env, name: &str) -> Option<(String, Ty)> {
    env.iter().rev().find_map(|s| s.get(name).cloned())
}

struct Gen {
    /// 生成した WGSL の関数と定数
    decls: Vec<String>,
    /// (関数の実体, 引数の型) → (WGSL の名前, 戻り型)
    funcs: HashMap<(usize, Vec<String>), (String, Ty)>,
    /// 外側の List → WGSL の定数名
    consts: HashMap<usize, (String, Ty)>,
    counter: usize,
    /// 変換中の関数 (再帰の検出)
    stack: Vec<usize>,
}

impl Gen {
    fn fresh(&mut self, base: &str) -> String {
        self.counter += 1;
        let clean: String = base.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        format!("{clean}_{}", self.counter)
    }

    /// 関数を WGSL にする。引数の型ごとに 1 つ作る。戻り値は (名前, 戻り型)
    fn func(&mut self, closure: &Closure, arg_tys: &[Ty]) -> Result<(String, Ty)> {
        let key = (Rc::as_ptr(&closure.def) as usize, arg_tys.iter().map(|t| format!("{t:?}")).collect());
        if let Some(found) = self.funcs.get(&key) {
            return Ok(found.clone());
        }
        if self.stack.contains(&key.0) {
            return err(KIND, "a shader function cannot call itself (no recursion on the GPU)");
        }
        let params = &closure.def.params;
        if params.len() != arg_tys.len() {
            return err(Kind::ArityMismatch, format!("shader function takes {} arguments, {} given", params.len(), arg_tys.len()));
        }
        let name = self.fresh("fn");
        self.stack.push(key.0);
        let mut env: Env = vec![HashMap::new()];
        let mut sig = Vec::new();
        for (p, ty) in params.iter().zip(arg_tys) {
            let Pattern::Name(n) = &p.pattern else { return err(KIND, "shader function parameters must be plain names") };
            let wgsl = self.fresh(n);
            sig.push(format!("{wgsl}: {}", ty.wgsl()?));
            env.last_mut().expect("scope").insert(n.clone(), (wgsl, *ty));
        }
        let mut body = Body::new(1);
        let mut ret: Option<Ty> = None;
        let last = self.block(&closure.def.body, &mut body, &mut env, closure, &mut ret, true)?;
        if let Some((v, ty)) = last {
            if ty != Ty::Nothing {
                unify(&mut ret, ty)?;
                body.line(format!("return {v};"));
            }
        }
        let ret = ret.filter(|t| *t != Ty::Nothing).ok_or_else(|| MophError::new(KIND, "shader function must return a value"))?;
        self.stack.pop();
        let mut text = format!("fn {name}({}) -> {} {{\n", sig.join(", "), ret.wgsl()?);
        text.push_str(&body.lines.join("\n"));
        text.push_str("\n}\n");
        self.decls.push(text);
        self.funcs.insert(key, (name.clone(), ret));
        Ok((name, ret))
    }

    /// 文の列。最後の文が式ならその値を返す。want_value が false なら値は捨てる
    fn block(&mut self, stmts: &[Stmt], out: &mut Body, env: &mut Env, closure: &Closure, ret: &mut Option<Ty>, want_value: bool) -> Result<Option<(String, Ty)>> {
        env.push(HashMap::new());
        let mut last = None;
        for (i, stmt) in stmts.iter().enumerate() {
            let is_last = i + 1 == stmts.len();
            last = self.stmt(stmt, out, env, closure, ret, is_last && want_value).map_err(|e| with_line(e, stmt.line))?;
        }
        env.pop();
        Ok(last)
    }

    fn stmt(&mut self, stmt: &Stmt, out: &mut Body, env: &mut Env, closure: &Closure, ret: &mut Option<Ty>, want_value: bool) -> Result<Option<(String, Ty)>> {
        match &stmt.kind {
            StmtKind::Let(Pattern::Name(name), _, e) => {
                let (v, ty) = self.expr(e, out, env, closure, ret)?;
                let wgsl = self.fresh(name);
                out.line(format!("var {wgsl}: {} = {v};", ty.wgsl()?));
                env.last_mut().expect("scope").insert(name.clone(), (wgsl, ty));
                Ok(None)
            }
            StmtKind::Let(..) => err(KIND, "destructuring let is not allowed in a shader"),
            StmtKind::AssignVar(name, e) => {
                let (v, ty) = self.expr(e, out, env, closure, ret)?;
                let Some((wgsl, expected)) = lookup(env, name) else {
                    return err(KIND, format!("\"{name}\" is not a variable of this shader function (outer variables are constants)"));
                };
                if ty != expected {
                    return err(Kind::OperandType, format!("cannot assign {} to {name} ({})", ty.name(), expected.name()));
                }
                out.line(format!("{wgsl} = {v};"));
                Ok(None)
            }
            StmtKind::AssignMulti(targets, values) => {
                let mut temps = Vec::new();
                for e in values {
                    let (v, ty) = self.expr(e, out, env, closure, ret)?;
                    let tmp = self.fresh("tmp");
                    out.line(format!("let {tmp}: {} = {v};", ty.wgsl()?));
                    temps.push((tmp, ty));
                }
                for (target, (tmp, ty)) in targets.iter().zip(temps) {
                    let Expr::Ident(name) = target else { return err(KIND, "only variables can be assigned in a shader") };
                    let Some((wgsl, expected)) = lookup(env, name) else {
                        return err(KIND, format!("\"{name}\" is not a variable of this shader function"));
                    };
                    if ty != expected {
                        return err(Kind::OperandType, format!("cannot assign {} to {name} ({})", ty.name(), expected.name()));
                    }
                    out.line(format!("{wgsl} = {tmp};"));
                }
                Ok(None)
            }
            StmtKind::For(Pattern::Name(name), iter, body) => {
                let Expr::Binary(op @ (BinOp::Range | BinOp::RangeInclusive), a, b) = iter else {
                    return err(KIND, "for in a shader must iterate a range (a..b)");
                };
                let (av, at) = self.expr(a, out, env, closure, ret)?;
                let (bv, bt) = self.expr(b, out, env, closure, ret)?;
                if at != Ty::Num || bt != Ty::Num {
                    return err(Kind::OperandType, "range bounds must be Number");
                }
                let i = self.fresh("i");
                let end = if *op == BinOp::RangeInclusive { format!("i32({bv}) + 1") } else { format!("i32({bv})") };
                out.line(format!("for (var {i}: i32 = i32({av}); {i} < {end}; {i}++) {{"));
                let wgsl = self.fresh(name);
                let mut inner = Body::new(out.indent + 1);
                inner.line(format!("var {wgsl}: f32 = f32({i});"));
                env.push(HashMap::from([(name.clone(), (wgsl, Ty::Num))]));
                self.block(body, &mut inner, env, closure, ret, false)?;
                env.pop();
                out.append(inner);
                out.line("}");
                Ok(None)
            }
            StmtKind::For(..) => err(KIND, "for in a shader must bind one name: for i in 0..n"),
            // 回数の決まらない繰り返しは GPU を止めてしまう (画面ごと固まる) ので受け取らない
            StmtKind::While(..) => err(KIND, "while is not available in a shader; use a for with a range and break out of it"),
            StmtKind::Break => {
                out.line("break;");
                Ok(None)
            }
            StmtKind::Return(e) => {
                let (v, ty) = self.expr(e, out, env, closure, ret)?;
                unify(ret, ty)?;
                out.line(format!("return {v};"));
                Ok(None)
            }
            StmtKind::Expr(Expr::If(cond, then, otherwise)) => self.if_(cond, then, otherwise.as_deref(), out, env, closure, ret, want_value),
            StmtKind::Expr(e) => {
                let (v, ty) = self.expr(e, out, env, closure, ret)?;
                if want_value {
                    Ok(Some((v, ty)))
                } else {
                    if ty != Ty::Nothing {
                        out.line(format!("_ = {v};"));
                    }
                    Ok(None)
                }
            }
            _ => err(KIND, "this statement is not allowed in a shader (only let, assignment, for over a range, break, return, and expressions)"),
        }
    }

    /// if。両方の枝が値を持ち、値が要るなら一時変数に入れて式の値にする
    #[allow(clippy::too_many_arguments)]
    fn if_(&mut self, cond: &Expr, then: &[Stmt], otherwise: Option<&[Stmt]>, out: &mut Body, env: &mut Env, closure: &Closure, ret: &mut Option<Ty>, want_value: bool) -> Result<Option<(String, Ty)>> {
        let (c, ct) = self.expr(cond, out, env, closure, ret)?;
        if ct != Ty::Bool {
            return err(Kind::OperandType, format!("if condition must be Bool, found {}", ct.name()));
        }
        let mut then_body = Body::new(out.indent + 1);
        let then_val = self.block(then, &mut then_body, env, closure, ret, want_value)?;
        let mut else_body = Body::new(out.indent + 1);
        let else_val = match otherwise {
            Some(stmts) => self.block(stmts, &mut else_body, env, closure, ret, want_value)?,
            None => None,
        };
        let valued = want_value && matches!((&then_val, &else_val), (Some((_, a)), Some((_, b))) if a == b && *a != Ty::Nothing);
        let mut result = None;
        if valued {
            let (_, ty) = then_val.clone().expect("checked");
            let tmp = self.fresh("if");
            out.line(format!("var {tmp}: {};", ty.wgsl()?));
            result = Some((tmp, ty));
        }
        out.line(format!("if ({c}) {{"));
        Self::finish_branch(out, then_body, then_val, result.as_ref());
        if otherwise.is_some() {
            out.line("} else {");
            Self::finish_branch(out, else_body, else_val, result.as_ref());
        }
        out.line("}");
        Ok(result)
    }

    fn finish_branch(out: &mut Body, mut body: Body, value: Option<(String, Ty)>, result: Option<&(String, Ty)>) {
        match (value, result) {
            (Some((v, _)), Some((tmp, _))) => body.line(format!("{tmp} = {v};")),
            (Some((v, ty)), None) if ty != Ty::Nothing => body.line(format!("_ = {v};")),
            _ => {}
        }
        out.append(body);
    }

    fn expr(&mut self, e: &Expr, out: &mut Body, env: &mut Env, closure: &Closure, ret: &mut Option<Ty>) -> Result<(String, Ty)> {
        Ok(match e {
            Expr::Number(n) => (lit(*n), Ty::Num),
            Expr::Bool(b) => (b.to_string(), Ty::Bool),
            Expr::Color(c) => (color_lit(c), Ty::Color),
            Expr::Ident(name) => match lookup(env, name) {
                Some(found) => found,
                None => self.captured(name, closure)?,
            },
            Expr::Neg(inner) => {
                let (v, ty) = self.expr(inner, out, env, closure, ret)?;
                if ty != Ty::Num && ty != Ty::Color && ty != Ty::Vec {
                    return err(Kind::OperandType, format!("cannot negate {}", ty.name()));
                }
                (format!("(-{v})"), ty)
            }
            Expr::Not(inner) => {
                let (v, ty) = self.expr(inner, out, env, closure, ret)?;
                if ty != Ty::Bool {
                    return err(Kind::OperandType, format!("cannot apply not to {}", ty.name()));
                }
                (format!("(!{v})"), Ty::Bool)
            }
            Expr::Binary(op, l, r) => {
                let (lv, lt) = self.expr(l, out, env, closure, ret)?;
                let (rv, rt) = self.expr(r, out, env, closure, ret)?;
                binary(*op, &lv, lt, &rv, rt)?
            }
            // a < b <= c。シェーダの式は副作用が無いので、真ん中を 2 度書いて && でつなぐ
            Expr::Compare(first, rest) => {
                let mut left = self.expr(first, out, env, closure, ret)?;
                let mut parts = Vec::new();
                for (op, e) in rest {
                    let right = self.expr(e, out, env, closure, ret)?;
                    let (v, t) = binary(*op, &left.0, left.1, &right.0, right.1)?;
                    if t != Ty::Bool {
                        return err(Kind::OperandType, "a comparison must produce a Bool");
                    }
                    parts.push(v);
                    left = right;
                }
                (format!("({})", parts.join(" && ")), Ty::Bool)
            }
            Expr::Attr(target, attr) => match target.as_ref() {
                Expr::Ident(m) if m == "math" && matches!(attr.as_str(), "PI" | "TAU" | "E") => {
                    let v = match attr.as_str() {
                        "PI" => std::f64::consts::PI,
                        "TAU" => std::f64::consts::TAU,
                        _ => std::f64::consts::E,
                    };
                    (lit(v), Ty::Num)
                }
                _ if attr == "x" || attr == "y" => {
                    let (v, ty) = self.expr(target, out, env, closure, ret)?;
                    if ty != Ty::Vec {
                        return err(Kind::UndefinedAttribute, format!("{} has no attribute \"{attr}\" in a shader", ty.name()));
                    }
                    (format!("{v}.{attr}"), Ty::Num)
                }
                _ => return err(KIND, format!("attribute .{attr} is not available in a shader")),
            },
            Expr::Call(callee, args) => {
                let mut values = Vec::new();
                for a in args {
                    if a.name.is_some() {
                        return err(KIND, "named arguments are not allowed in a shader");
                    }
                    values.push(self.expr(&a.value, out, env, closure, ret)?);
                }
                match callee.as_ref() {
                    Expr::Attr(m, fname) if matches!(m.as_ref(), Expr::Ident(n) if n == "math") => math_call(fname, &values)?,
                    Expr::Ident(name) if name == "Vector" || name == "Color" => build(name, values)?,
                    Expr::Ident(fname) => {
                        if lookup(env, fname).is_some() {
                            return err(KIND, format!("\"{fname}\" is a variable, not a function"));
                        }
                        let Some(Value::Func(f)) = find_captured(fname, closure) else {
                            return err(Kind::UndefinedVariable, format!("\"{fname}\" is not a function known to the shader"));
                        };
                        let tys: Vec<Ty> = values.iter().map(|(_, t)| *t).collect();
                        let (name, ret_ty) = self.func(&f, &tys)?;
                        let list: Vec<String> = values.into_iter().map(|(v, _)| v).collect();
                        (format!("{name}({})", list.join(", ")), ret_ty)
                    }
                    _ => return err(KIND, "only named functions and math.* can be called in a shader"),
                }
            }
            Expr::Index(target, index) => {
                let (tv, tt) = self.expr(target, out, env, closure, ret)?;
                let (iv, it) = self.expr(index, out, env, closure, ret)?;
                if it != Ty::Num {
                    return err(Kind::OperandType, "index must be Number");
                }
                let elem = match tt {
                    Ty::Nums(_) => Ty::Num,
                    Ty::Colors(_) => Ty::Color,
                    Ty::Vecs(_) => Ty::Vec,
                    other => return err(Kind::OperandType, format!("cannot index {}", other.name())),
                };
                (format!("{tv}[u32({iv})]"), elem)
            }
            Expr::Tuple(_) => return err(KIND, "a plain tuple is not available in a shader; write Vector(x, y)"),
            Expr::If(cond, then, otherwise) => {
                let value = self.if_(cond, then, otherwise.as_deref(), out, env, closure, ret, true)?;
                value.ok_or_else(|| MophError::new(KIND, "an if used as a value needs both branches to end with a value of the same type"))?
            }
            Expr::Str(_) | Expr::Symbol(_) | Expr::Duration(_) => return err(KIND, "strings, symbols and Durations are not available in a shader (use Number seconds)"),
            _ => return err(KIND, "this expression is not allowed in a shader"),
        })
    }

    /// 関数の外側の変数。値をそのまま埋め込む
    fn captured(&mut self, name: &str, closure: &Closure) -> Result<(String, Ty)> {
        let Some(v) = find_captured(name, closure) else {
            return err(Kind::UndefinedVariable, format!("\"{name}\" is not defined"));
        };
        Ok(match v {
            Value::Number(n, _) => (lit(n), Ty::Num),
            Value::Bool(b) => (b.to_string(), Ty::Bool),
            Value::Color(c) => (color_lit(&c), Ty::Color),
            Value::Vector(x, y) => (vec_lit(x, y), Ty::Vec),
            Value::List(items) => {
                let key = Rc::as_ptr(&items) as usize;
                if let Some(found) = self.consts.get(&key) {
                    return Ok(found.clone());
                }
                let items = items.borrow();
                let (ty, elems): (Ty, Vec<String>) = if items.iter().all(|v| matches!(v, Value::Number(_, _))) {
                    (Ty::Nums(Some(items.len())), items.iter().map(|v| match v { Value::Number(n, _) => lit(*n), _ => unreachable!() }).collect())
                } else if items.iter().all(|v| matches!(v, Value::Color(_))) {
                    (Ty::Colors(items.len()), items.iter().map(|v| match v { Value::Color(c) => color_lit(c), _ => unreachable!() }).collect())
                } else if items.iter().all(|v| matches!(v, Value::Vector(..))) {
                    (Ty::Vecs(items.len()), items.iter().map(|v| match v { Value::Vector(x, y) => vec_lit(*x, *y), _ => unreachable!() }).collect())
                } else {
                    return err(KIND, format!("List \"{name}\" must hold only Numbers, only Colors or only Vectors to be used in a shader"));
                };
                if elems.is_empty() {
                    return err(KIND, format!("List \"{name}\" is empty"));
                }
                let wgsl = self.fresh(name);
                self.decls.push(format!("const {wgsl}: {} = {}({});\n", ty.wgsl()?, ty.wgsl()?, elems.join(", ")));
                self.consts.insert(key, (wgsl.clone(), ty));
                (wgsl, ty)
            }
            Value::Func(_) => return err(KIND, format!("\"{name}\" is a function; call it")),
            other => return err(KIND, format!("\"{name}\" ({}) cannot be used in a shader", other.type_name())),
        })
    }
}

fn find_captured(name: &str, closure: &Closure) -> Option<Value> {
    closure.scopes.iter().rev().find_map(|s| s.borrow().get(name).cloned())
}

fn unify(ret: &mut Option<Ty>, ty: Ty) -> Result<()> {
    match ret {
        Some(existing) if *existing != ty => err(Kind::OperandType, format!("shader function returns both {} and {}", existing.name(), ty.name())),
        _ => {
            *ret = Some(ty);
            Ok(())
        }
    }
}

fn with_line(e: MophError, line: usize) -> MophError {
    if e.message.starts_with("line ") { e } else { MophError::new(e.kind, format!("line {line}: {}", e.message)) }
}

fn lit(n: f64) -> String {
    if !n.is_finite() {
        return "0.0".into();
    }
    let s = format!("{n:?}");
    let s = if s.contains('.') || s.contains('e') { s } else { format!("{s}.0") };
    if n < 0.0 { format!("({s})") } else { s }
}

fn vec_lit(x: f64, y: f64) -> String {
    format!("vec2<f32>({}, {})", lit(x), lit(y))
}

fn color_lit(c: &[f32; 4]) -> String {
    format!("vec4<f32>({}, {}, {}, {})", lit(f64::from(c[0])), lit(f64::from(c[1])), lit(f64::from(c[2])), lit(f64::from(c[3])))
}

fn binary(op: BinOp, lv: &str, lt: Ty, rv: &str, rt: Ty) -> Result<(String, Ty)> {
    use Ty::{Bool, Color, Num, Vec};
    let bad = || MophError::new(Kind::OperandType, format!("cannot {} {} and {} in a shader", crate::lang::eval::verb(op), lt.name(), rt.name()));
    Ok(match op {
        BinOp::Add | BinOp::Sub => match (lt, rt) {
            (Num, Num) => (format!("({lv} {} {rv})", sym(op)), Num),
            (Color, Color) => (format!("({lv} {} {rv})", sym(op)), Color),
            (Vec, Vec) => (format!("({lv} {} {rv})", sym(op)), Vec),
            _ => return Err(bad()),
        },
        BinOp::Mul => match (lt, rt) {
            (Num, Num) => (format!("({lv} * {rv})"), Num),
            (Color, Num) | (Num, Color) => (format!("({lv} * {rv})"), Color),
            (Vec, Num) | (Num, Vec) => (format!("({lv} * {rv})"), Vec),
            _ => return Err(bad()),
        },
        BinOp::Div => match (lt, rt) {
            (Num, Num) => (format!("({lv} / {rv})"), Num),
            (Color, Num) => (format!("({lv} / {rv})"), Color),
            (Vec, Num) => (format!("({lv} / {rv})"), Vec),
            _ => return Err(bad()),
        },
        BinOp::Rem => match (lt, rt) {
            (Num, Num) => (format!("({lv} % {rv})"), Num),
            _ => return Err(bad()),
        },
        BinOp::Pow => match (lt, rt) {
            (Num, Num) => (format!("pow({lv}, {rv})"), Num),
            _ => return Err(bad()),
        },
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => match (lt, rt) {
            (Num, Num) => (format!("({lv} {} {rv})", sym(op)), Bool),
            _ => return Err(bad()),
        },
        BinOp::Eq | BinOp::Ne => match (lt, rt) {
            (Num, Num) | (Bool, Bool) => (format!("({lv} {} {rv})", sym(op)), Bool),
            (Color, Color) | (Vec, Vec) => (format!("({}all({lv} == {rv}))", if op == BinOp::Ne { "!" } else { "" }), Bool),
            _ => return Err(bad()),
        },
        BinOp::And | BinOp::Or => match (lt, rt) {
            (Bool, Bool) => (format!("({lv} {} {rv})", if op == BinOp::And { "&&" } else { "||" }), Bool),
            _ => return Err(bad()),
        },
        BinOp::Range | BinOp::RangeInclusive => return err(KIND, "a range is only allowed in for"),
    })
}

fn sym(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        _ => "?",
    }
}

/// シェーダの中で Vector(x, y) と Color(r, g, b [, a]) を作る
fn build(name: &str, values: Vec<(String, Ty)>) -> Result<(String, Ty)> {
    let mut parts = Vec::new();
    for (v, ty) in values {
        if ty != Ty::Num {
            return err(Kind::ArgumentType, format!("{name} expects Number, found {}", ty.name()));
        }
        parts.push(v);
    }
    match (name, parts.as_slice()) {
        ("Vector", [x, y]) => Ok((format!("vec2<f32>({x}, {y})"), Ty::Vec)),
        ("Color", [r, g, b]) => Ok((format!("vec4<f32>({r} / 255.0, {g} / 255.0, {b} / 255.0, 1.0)"), Ty::Color)),
        ("Color", [r, g, b, a]) => Ok((format!("vec4<f32>({r} / 255.0, {g} / 255.0, {b} / 255.0, {a})"), Ty::Color)),
        ("Vector", _) => err(Kind::ArityMismatch, "Vector takes 2 arguments"),
        _ => err(Kind::ArityMismatch, "Color takes 3 or 4 arguments"),
    }
}

fn math_call(name: &str, args: &[(String, Ty)]) -> Result<(String, Ty)> {
    if args.iter().any(|(_, t)| *t != Ty::Num) {
        return err(Kind::ArgumentType, format!("math.{name} takes Numbers"));
    }
    let list: Vec<&str> = args.iter().map(|(v, _)| v.as_str()).collect();
    let unary = |f: &str| -> Result<(String, Ty)> {
        match list.as_slice() {
            [x] => Ok((format!("{f}({x})"), Ty::Num)),
            _ => err(Kind::ArityMismatch, format!("math.{name} takes 1 argument, {} given", list.len())),
        }
    };
    match name {
        "sin" | "cos" | "tan" | "floor" | "ceil" | "round" | "abs" | "sign" | "sqrt" | "exp" | "exp2" | "log2" => unary(name),
        "ln" => unary("log"),
        "log10" => match list.as_slice() {
            [x] => Ok((format!("(log2({x}) * 0.30102999566)"), Ty::Num)),
            _ => err(Kind::ArityMismatch, format!("math.log10 takes 1 argument, {} given", list.len())),
        },
        "pow" => match list.as_slice() {
            [x, y] => Ok((format!("pow({x}, {y})"), Ty::Num)),
            _ => err(Kind::ArityMismatch, "math.pow takes 2 arguments (x, y)"),
        },
        "clamp" => match list.as_slice() {
            [x, lo, hi] => Ok((format!("clamp({x}, {lo}, {hi})"), Ty::Num)),
            _ => err(Kind::ArityMismatch, "math.clamp takes 3 arguments (x, lo, hi)"),
        },
        "atan2" => match list.as_slice() {
            [y, x] => Ok((format!("atan2({y}, {x})"), Ty::Num)),
            _ => err(Kind::ArityMismatch, "math.atan2 takes 2 arguments (y, x)"),
        },
        "max" | "min" => match list.as_slice() {
            [] => err(Kind::ArityMismatch, format!("math.{name} needs at least 1 argument")),
            [x] => Ok((x.to_string(), Ty::Num)),
            _ => Ok((list[1..].iter().fold(list[0].to_string(), |acc, x| format!("{name}({acc}, {x})")), Ty::Num)),
        },
        _ => err(Kind::UndefinedAttribute, format!("math has no \"{name}\" usable in a shader")),
    }
}

/// color: func (x, y, t [, scale] [, args]) を、ピクセルごとに走る compute shader の WGSL にする。
/// camera を入れた Shader は、x y が camera.from からの差になり、4 つ目で倍率を受け取る
pub fn compile(closure: &Closure, camera: bool) -> Result<String> {
    let params = &closure.def.params;
    let fixed = if camera { 4 } else { 3 };
    if params.len() != fixed && params.len() != fixed + 1 {
        let shape = if camera { "func (x, y, t, scale) or func (x, y, t, scale, args)" } else { "func (x, y, t) or func (x, y, t, args)" };
        return err(Kind::ArityMismatch, format!("Shader.color must be {shape}, found {} parameters", params.len()));
    }
    let mut g = Gen { decls: Vec::new(), funcs: HashMap::new(), consts: HashMap::new(), counter: 0, stack: Vec::new() };
    let mut env: Env = vec![HashMap::new()];
    let names = ["x", "y", "t", "cam"];
    for (i, p) in params.iter().enumerate() {
        let Pattern::Name(n) = &p.pattern else { return err(KIND, "Shader.color parameters must be plain names") };
        let entry = if i < fixed { (names[i].to_string(), Ty::Num) } else { ("args".to_string(), Ty::Nums(None)) };
        env[0].insert(n.clone(), entry);
    }
    let mut body = Body::new(1);
    let mut ret = None;
    let last = g.block(&closure.def.body, &mut body, &mut env, closure, &mut ret, true)?;
    if let Some((v, ty)) = last {
        if ty != Ty::Nothing {
            unify(&mut ret, ty)?;
            body.line(format!("return {v};"));
        }
    }
    if ret != Some(Ty::Color) {
        return err(Kind::ArgumentType, format!("Shader.color must return Color, found {}", ret.map_or("nothing".to_string(), Ty::name)));
    }
    let mut wgsl = String::from(UNIFORM);
    wgsl.push_str(RING);
    wgsl.push_str(
        "@group(0) @binding(1) var<storage, read> args: array<f32>;\n\
         @group(0) @binding(2) var out: texture_storage_2d_array<rgba8unorm, write>;\n\
         \n",
    );
    for d in &g.decls {
        wgsl.push_str(d);
        wgsl.push('\n');
    }
    let sig = if camera { "x: f32, y: f32, t: f32, cam: f32" } else { "x: f32, y: f32, t: f32" };
    wgsl.push_str(&format!("fn color({sig}) -> vec4<f32> {{\n"));
    wgsl.push_str(&body.lines.join("\n"));
    wgsl.push_str("\n}\n\n");
    // 1 ピクセルを s x s の格子で評価して平均する (アンチエイリアス)
    wgsl.push_str(
        "@compute @workgroup_size(8, 8)\n\
         fn main(@builtin(global_invocation_id) id: vec3<u32>) {\n\
         \x20   if (id.x >= u.w || id.y >= u.h) { return; }\n\
         \x20   let g = max(u.s, 1u);\n\
         \x20   var acc = vec4<f32>(0.0);\n\
         \x20   for (var j = 0u; j < g; j++) {\n\
         \x20       for (var i = 0u; i < g; i++) {\n\
         \x20           let x = u.ox + (f32(id.x) + (f32(i) + 0.5) / f32(g)) * u.sx;\n\
         \x20           let y = u.oy + (f32(id.y) + (f32(j) + 0.5) / f32(g)) * u.sy;\n\
         \x20           acc += clamp(color(x, y, u.t{CAM}), vec4<f32>(0.0), vec4<f32>(1.0));\n\
         \x20       }\n\
         \x20   }\n\
         \x20   textureStore(out, vec2<i32>(i32(id.x), i32(id.y)), 0, acc / f32(g * g));\n\
         }\n",
    );
    wgsl.push_str(STRIP);
    Ok(wgsl.replace("{CAM}", if camera { ", u.cam" } else { "" }))
}

/// 3 つの compute shader で共通の入れ物。zoom で始まる分はズーム動画のときだけ使う
const UNIFORM: &str = "struct U {\n\
    \x20   t: f32, ox: f32, oy: f32, sx: f32, sy: f32,\n\
    \x20   w: u32, h: u32, n: u32, s: u32,\n\
    \x20   cx: f32, cy: f32, ustart: f32, lnk: f32, cpe: f32,\n\
    \x20   c0: i32, ring: u32, rows: u32, cam: f32,\n\
    \x20   ea: f32, eb: f32, wide: u32,\n\
    }\n\
    @group(0) @binding(0) var<uniform> u: U;\n";

/// 帯を 1 回の dispatch でいくつのテクセルまで作るか。長すぎるとドライバに切り上げられるので区切る
const STRIP_TEXELS: u32 = 1 << 20;

/// 帯の角度軸の刻み。等間隔ではなく、その方向で画面の縁までの距離 R(th) に比例した密度で刻む。
/// 帯の列は「画面の隅に届くまで」使い回すので、必要な細かさは方向ごとに R(th) までで、
/// 上下のように早く画面から出る方向に隅と同じ細かさを持たせるのは誰も見ない計算になる (16:9 で 3 割)。
/// 目盛りは F(th) = ∫R dth。R は矩形なので区間ごとに a/cos で、F と逆向きは asinh と sinh で書ける。
/// ea eb は中心から縁までの左右・上下の距離 (中心が真ん中でなければ広いほうに合わせる)
const RING: &str = "const MZ_QP: f32 = 1.5707963267948966;\n\
    fn mz_q1() -> f32 { return u.ea * asinh(u.eb / u.ea); }\n\
    fn mz_q2() -> f32 { return u.eb * asinh(u.ea / u.eb); }\n\
    fn mz_total() -> f32 { return 4.0 * (mz_q1() + mz_q2()); }\n\
    fn mz_reach(th: f32) -> f32 {\n\
    \x20   let c = abs(cos(th));\n\
    \x20   let s = abs(sin(th));\n\
    \x20   return min(select(1.0e30, u.ea / c, c > 1.0e-6), select(1.0e30, u.eb / s, s > 1.0e-6));\n\
    }\n\
    // 四分円ごとに、先に来る辺と後に来る辺が入れ替わる。1 周ぶんの目盛りはどの四分円でも同じ\n\
    fn mz_sides(k: f32) -> vec2<f32> {\n\
    \x20   let odd = (k - 2.0 * floor(k * 0.5)) > 0.5;\n\
    \x20   return vec2<f32>(select(u.ea, u.eb, odd), select(u.eb, u.ea, odd));\n\
    }\n\
    fn mz_mark(th: f32) -> f32 {\n\
    \x20   let k = floor(th / MZ_QP);\n\
    \x20   let r = th - k * MZ_QP;\n\
    \x20   let e = mz_sides(k);\n\
    \x20   let first = e.x * asinh(e.y / e.x);\n\
    \x20   var m = first + e.y * (asinh(tan(r - MZ_QP)) + asinh(e.x / e.y));\n\
    \x20   if (r < atan2(e.y, e.x)) { m = e.x * asinh(tan(r)); }\n\
    \x20   return k * (mz_q1() + mz_q2()) + m;\n\
    }\n\
    fn mz_angle(f: f32) -> f32 {\n\
    \x20   let per = mz_q1() + mz_q2();\n\
    \x20   let k = floor(f / per);\n\
    \x20   let g = f - k * per;\n\
    \x20   let e = mz_sides(k);\n\
    \x20   let first = e.x * asinh(e.y / e.x);\n\
    \x20   var r = MZ_QP + atan(sinh((g - first) / e.y - asinh(e.x / e.y)));\n\
    \x20   if (g < first) { r = atan(sinh(g / e.x)); }\n\
    \x20   return k * MZ_QP + r;\n\
    }\n";

/// 帯を作る。列 c は中心からの距離の対数 u = ustart - c / cpe、行は角度。
/// 書き込み先は環状バッファなので c を ring で折り返す
const STRIP: &str = "\n@compute @workgroup_size(8, 8)\n\
    fn strip(@builtin(global_invocation_id) id: vec3<u32>) {\n\
    \x20   if (id.x >= u.w || id.y >= u.h) { return; }\n\
    \x20   let c = u.c0 + i32(id.x);\n\
    \x20   let g = max(u.s, 1u);\n\
    \x20   let per = mz_total() / f32(u.rows);\n\
    \x20   var acc = vec4<f32>(0.0);\n\
    \x20   for (var j = 0u; j < g; j++) {\n\
    \x20       for (var i = 0u; i < g; i++) {\n\
    \x20           let cf = f32(c) + (f32(i) + 0.5) / f32(g) - 0.5;\n\
    \x20           let x = u.ustart - cf * u.sx;\n\
    \x20           let y = mz_angle((f32(id.y) + (f32(j) + 0.5) / f32(g)) * per);\n\
    \x20           acc += clamp(color(x, y, u.t{CAM}), vec4<f32>(0.0), vec4<f32>(1.0));\n\
    \x20       }\n\
    \x20   }\n\
    \x20   let col = ((c % i32(u.ring)) + i32(u.ring)) % i32(u.ring);\n\
    \x20   let v = acc / f32(g * g);\n\
    \x20   let wide = i32(u.wide);\n\
    \x20   textureStore(out, vec2<i32>(col % wide, i32(id.y)), col / wide, v);\n\
    \x20   // 層の先頭は、1 つ前の層の右端にも置く (最初の層の先頭は最後の層の右端へ回り込む)\n\
    \x20   if (col % wide == 0) {\n\
    \x20       let back = (col / wide + i32(u.ring) / wide - 1) % (i32(u.ring) / wide);\n\
    \x20       textureStore(out, vec2<i32>(wide, i32(id.y)), back, v);\n\
    \x20   }\n\
    }\n";

/// 帯から 1 フレームを組む。どの Shader でも同じなので、これだけ別のモジュールにする
const FRAME: &str = "@group(0) @binding(1) var strip: texture_2d_array<f32>;\n\
    @group(0) @binding(2) var samp: sampler;\n\
    @group(0) @binding(3) var out: texture_storage_2d_array<rgba8unorm, write>;\n\
    \n\
    // 帯は幅がテクスチャの上限を超えることがあるので (4K で 18500 列) 層に分けてある。\n\
    // 各層は右端に次の層の先頭を 1 列だけ写して持っているので、層の中で補間すれば継ぎ目は出ない\n\
    fn mz_column(cw: f32) -> vec2<f32> {\n\
    \x20   let ring = f32(u.ring);\n\
    \x20   let cm = cw - floor(cw / ring) * ring;\n\
    \x20   let base = i32(floor(cm));\n\
    \x20   let wide = i32(u.wide);\n\
    \x20   return vec2<f32>(f32(base % wide) + cm - floor(cm), f32(base / wide));\n\
    }\n\
    \n\
    @compute @workgroup_size(8, 8)\n\
    fn main(@builtin(global_invocation_id) id: vec3<u32>) {\n\
    \x20   if (id.x >= u.w || id.y >= u.h) { return; }\n\
    \x20   let dx = f32(id.x) + 0.5 - u.cx;\n\
    \x20   let dy = f32(id.y) + 0.5 - u.cy;\n\
    \x20   // 中心の 1 画素は対数が発散するので、半画素で止める\n\
    \x20   let rho = max(sqrt(dx * dx + dy * dy), 0.5);\n\
    \x20   var th = atan2(dy, dx);\n\
    \x20   if (th < 0.0) { th = th + 6.283185307179586; }\n\
    \x20   let c = (u.ustart - (log(rho) + u.lnk)) * u.cpe;\n\
    \x20   // 1 画素が覆う帯の広さ。中心に近いほど帯は細かいので、そのぶん平均する\n\
    \x20   let per = mz_total() / f32(u.rows);\n\
    \x20   let fc = u.cpe / rho;\n\
    \x20   let fr = mz_reach(th) / (per * rho);\n\
    \x20   let n = i32(clamp(ceil(max(fc, fr)), 1.0, 4.0));\n\
    \x20   var acc = vec4<f32>(0.0);\n\
    \x20   for (var j = 0; j < n; j++) {\n\
    \x20       for (var i = 0; i < n; i++) {\n\
    \x20           let ox = ((f32(i) + 0.5) / f32(n) - 0.5) * fc;\n\
    \x20           let oy = ((f32(j) + 0.5) / f32(n) - 0.5) * fr;\n\
    \x20           let at = mz_column(c + ox);\n\
    \x20           let uv = vec2<f32>((at.x + 0.5) / f32(u.wide + 1u), (mz_mark(th) / per + oy + 0.5) / f32(u.rows));\n\
    \x20           acc += textureSampleLevel(strip, samp, uv, i32(at.y), 0.0);\n\
    \x20       }\n\
    \x20   }\n\
    \x20   textureStore(out, vec2<i32>(i32(id.x), i32(id.y)), 0, acc / f32(n * n));\n\
    }\n";

// ---------- GPU で走らせる ----------

/// 3D の塗りの依頼。頂点は面ごとに分けて並べてある (平らに塗るため)
pub struct World<'a> {
    /// 塗る図形の通し番号 (テクスチャの使い回しの鍵)
    pub shape: u64,
    /// いま組んでいるフレームの番号
    pub frame: u64,
    pub width: u32,
    pub height: u32,
    pub view: crate::render::mesh::View3,
    /// 何も無いところの色
    pub background: [f32; 4],
    pub vertices: &'a [crate::render::mesh::Vertex],
}

/// 1 つの図形の塗りの依頼
/// Shader.args。Array は同じ実体である間 GPU に送り直さない。List は毎フレーム写して送る
pub enum Args<'a> {
    Plain(&'a [f32]),
    Shared(&'a Rc<crate::lang::value::Array>),
}

impl Args<'_> {
    fn f32s(&self) -> &[f32] {
        match self {
            Args::Plain(xs) => xs,
            Args::Shared(a) => a.f32s(),
        }
    }
}

pub struct Request<'a> {
    /// 塗る図形の通し番号 (テクスチャの使い回しの鍵)
    pub shape: u64,
    /// いま組んでいるフレームの番号。使わなくなったものを手放すのに使う
    pub frame: u64,
    pub closure: &'a Rc<Closure>,
    pub args: Args<'a>,
    pub t: f64,
    pub width: u32,
    pub height: u32,
    /// ピクセル (0, 0) の中心の箱の座標と、1 ピクセルあたりの箱の座標の増分
    pub origin: (f64, f64),
    pub step: (f64, f64),
    /// 1 ピクセルあたりのサンプル数。平方数に切り上げる (4 → 2x2)
    pub samples: u32,
    /// ズーム動画のとき。1 枚ずつ描かず、対数極座標の帯を伸ばしながら使い回す
    pub zoom: Option<ZoomPath<'a>>,
    /// カメラの倍率。camera を入れた Shader のとき。x y は既に camera.from からの差になっている
    pub camera: Option<f64>,
}

/// 中心へ寄っていくだけのズームは、(中心からの距離の対数, 角度) で見ると
/// どのフレームも同じ絵の平行移動になる。だから全フレーム分を 1 本の帯として持ち、
/// 各フレームはその窓を読むだけで済む。帯は 1 フレームあたり数列しか伸びない
pub struct ZoomPath<'a> {
    /// 箱の座標での、寄っていく先
    pub center: (f64, f64),
    /// ln(箱の座標 1 あたりの複素平面の長さ) を、0..duration に等間隔で並べたもの
    pub scale: &'a [f64],
    /// scale が覆う時間 (秒)
    pub duration: f64,
}

impl ZoomPath<'_> {
    /// その時刻の ln(スケール)。表の間は直線で結ぶ
    fn ln_scale(&self, t: f64) -> f64 {
        let last = self.scale.len() - 1;
        let at = (t / self.duration * last as f64).clamp(0.0, last as f64);
        let i = (at.floor() as usize).min(last);
        let j = (i + 1).min(last);
        self.scale[i] + (self.scale[j] - self.scale[i]) * (at - i as f64)
    }
}

/// uniform の中身。WGSL の struct U と並びを合わせること
const UNIFORM_BYTES: usize = 96;

#[derive(Default)]
struct Uniforms {
    t: f64,
    origin: (f64, f64),
    step: (f64, f64),
    w: u32,
    h: u32,
    n: u32,
    s: u32,
    center: (f64, f64),
    ustart: f64,
    lnk: f64,
    cpe: f64,
    c0: i32,
    ring: u32,
    rows: u32,
    /// camera.scale。camera を入れた Shader だけ使う
    cam: f64,
    /// 中心から画面の縁までの左右・上下の距離 (画素)。帯の角度軸の刻みに使う
    edge: (f64, f64),
    /// 帯 1 層あたりの列数。テクスチャの上限を超える幅は層に分ける
    wide: u32,
}

impl Uniforms {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(UNIFORM_BYTES);
        for f in [self.t, self.origin.0, self.origin.1, self.step.0, self.step.1] {
            out.extend_from_slice(&(f as f32).to_le_bytes());
        }
        for n in [self.w, self.h, self.n, self.s] {
            out.extend_from_slice(&n.to_le_bytes());
        }
        for f in [self.center.0, self.center.1, self.ustart, self.lnk, self.cpe] {
            out.extend_from_slice(&(f as f32).to_le_bytes());
        }
        out.extend_from_slice(&self.c0.to_le_bytes());
        for n in [self.ring, self.rows] {
            out.extend_from_slice(&n.to_le_bytes());
        }
        for f in [self.cam, self.edge.0, self.edge.1] {
            out.extend_from_slice(&(f as f32).to_le_bytes());
        }
        out.extend_from_slice(&self.wide.to_le_bytes());
        out.resize(UNIFORM_BYTES, 0);
        out
    }
}

struct Pipeline {
    pipeline: wgpu::ComputePipeline,
    /// 帯を伸ばすほう。ズーム動画のときだけ使う
    strip: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

/// 1 つの図形が持つ帯。列は環状に使い回す
struct Strip {
    /// 最後に使ったフレームの番号
    used: u64,
    view: wgpu::TextureView,
    /// 列数 (環状バッファの幅)。テクスチャの上限を超えるときは層に分ける
    ring: u32,
    /// 1 層あたりの列数
    wide: u32,
    /// 行数 (角度の刻み)
    rows: u32,
    /// 列 0 の u (中心からの距離の対数)
    ustart: f64,
    /// 計算済みの列 [a, b]
    have: Option<(i64, i64)>,
    /// 作ったときの図形の大きさ。変わったら作り直す
    size: (u32, u32),
}

struct Target {
    /// 最後に使ったフレームの番号
    used: u64,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// 描き先として渡すときの view (層を持たない形)。3D のときだけ使う
    attach: wgpu::TextureView,
    /// 3D の前後を決める深度テクスチャと、頂点を置く場所。要るときに作る
    depth: Option<wgpu::TextureView>,
    points: Option<wgpu::Buffer>,
    width: u32,
    height: u32,
    /// Vello に渡す画像。中身は使わず、テクスチャで差し替える
    image: ImageData,
    uniforms: wgpu::Buffer,
    args: wgpu::Buffer,
    /// args に送ってある Array。同じものが来たら送らない (持っておくので、その番地が別の Array に回ることも無い)
    shared: Option<Rc<crate::lang::value::Array>>,
}

pub struct ShaderRunner {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// 関数の実体 → pipeline
    pipelines: HashMap<usize, Pipeline>,
    /// 図形の通し番号 → テクスチャ
    targets: HashMap<u64, Target>,
    /// 図形の通し番号 → 帯 (ズーム動画のときだけ)
    strips: HashMap<u64, Strip>,
    /// 帯から 1 フレームを組む pipeline。どの Shader でも同じなので 1 つだけ
    frame: Option<(wgpu::ComputePipeline, wgpu::BindGroupLayout, wgpu::Sampler)>,
    /// Mesh を描く pipeline。どの World でも同じなので 1 つだけ
    meshes: Option<crate::render::mesh::Meshes>,
    /// 描画の前に Vello へ登録する (画像, テクスチャ)
    pub overrides: Vec<(ImageData, wgpu::Texture)>,
    /// 手放したテクスチャ。Vello の登録も外さないと、あちらが持っているぶんが解放されない
    pub released: Vec<ImageData>,
}

impl ShaderRunner {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self {
            device,
            queue,
            pipelines: HashMap::new(),
            targets: HashMap::new(),
            strips: HashMap::new(),
            frame: None,
            meshes: None,
            overrides: Vec::new(),
            released: Vec::new(),
        }
    }

    /// いま持っている GPU のメモリ (バイト)。テクスチャと帯の合計
    pub fn bytes(&self) -> u64 {
        let targets: u64 = self.targets.values().map(|t| u64::from(t.width) * u64::from(t.height) * 4).sum();
        let strips: u64 = self.strips.values().map(|s| u64::from(s.ring) * u64::from(s.rows) * 4).sum();
        targets + strips
    }

    /// しばらく使っていないテクスチャと帯を手放す。wgpu は参照が落ちた時点で解放する
    pub fn drop_unused(&mut self, now: u64, keep: u64) {
        let released = &mut self.released;
        self.targets.retain(|_, t| {
            let alive = now.saturating_sub(t.used) <= keep;
            if !alive {
                released.push(t.image.clone());
            }
            alive
        });
        self.strips.retain(|_, s| now.saturating_sub(s.used) <= keep);
    }

    /// Mesh を GPU で描き、塗りに使う画像を返す。描画の前に overrides を Vello に登録すること
    pub fn run_world(&mut self, req: World) -> Result<ImageData> {
        let needs_new = self.targets.get(&req.shape).is_none_or(|t| t.width != req.width || t.height != req.height);
        if needs_new {
            let target = self.make_target(req.width, req.height)?;
            if let Some(old) = self.targets.insert(req.shape, target) {
                self.released.push(old.image);
            }
        }
        if self.meshes.is_none() {
            self.meshes = Some(crate::render::mesh::Meshes::new(&self.device));
        }
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let want = (req.vertices.len().max(3) * crate::render::mesh::VERTEX_BYTES) as u64;
        let depth = match needs_new {
            true => None,
            false => self.targets.get_mut(&req.shape).and_then(|t| t.depth.take()),
        };
        let depth = match depth {
            Some(view) => view,
            None => {
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("mophila mesh depth"),
                    size: wgpu::Extent3d { width: req.width, height: req.height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: crate::render::mesh::DEPTH,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                texture.create_view(&wgpu::TextureViewDescriptor::default())
            }
        };
        let points = match self.targets.get_mut(&req.shape).and_then(|t| t.points.take()) {
            Some(buffer) if buffer.size() >= want => buffer,
            _ => self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mophila mesh points"),
                size: want,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        };
        if let Some(e) = pollster::block_on(scope.pop()) {
            return err(Kind::OutOfMemory, oom("a 3D fill", req.width, req.height, self.bytes(), &e));
        }
        self.queue.write_buffer(&points, 0, &crate::render::mesh::bytes_of(req.vertices));
        let target = self.targets.get_mut(&req.shape).expect("inserted above");
        target.used = req.frame;
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mophila mesh") });
        self.meshes.as_ref().expect("made above").draw(
            &self.device,
            &self.queue,
            &mut encoder,
            &target.attach,
            &depth,
            &target.uniforms,
            &points,
            &req.view,
            req.vertices.len() as u32,
            req.background,
        );
        self.queue.submit([encoder.finish()]);
        let image = target.image.clone();
        self.overrides.push((image.clone(), target.texture.clone()));
        target.depth = Some(depth);
        target.points = Some(points);
        Ok(image)
    }

    /// compute shader を投入し、塗りに使う画像を返す。描画の前に overrides を Vello に登録すること
    pub fn run(&mut self, req: Request) -> Result<ImageData> {
        // camera の有無で WGSL が変わるので、鍵に混ぜる
        let key = Rc::as_ptr(req.closure) as usize | usize::from(req.camera.is_some());
        if !self.pipelines.contains_key(&key) {
            let wgsl = compile(req.closure, req.camera.is_some())?;
            let pipeline = self.build_pipeline(&wgsl)?;
            self.pipelines.insert(key, pipeline);
        }
        let needs_new = self.targets.get(&req.shape).is_none_or(|t| t.width != req.width || t.height != req.height);
        if needs_new {
            let target = self.make_target(req.width, req.height)?;
            if let Some(old) = self.targets.insert(req.shape, target) {
                self.released.push(old.image);
            }
        }
        self.targets.get_mut(&req.shape).expect("inserted above").used = req.frame;
        if let Some(strip) = self.strips.get_mut(&req.shape) {
            strip.used = req.frame;
        }
        let args = req.args.f32s();
        let args_len = args.len() as u32;
        {
            let target = self.targets.get_mut(&req.shape).expect("inserted above");
            let sent = match &req.args {
                Args::Shared(a) => target.shared.as_ref().is_some_and(|s| Rc::ptr_eq(s, a)),
                Args::Plain(_) => false,
            };
            if !sent {
                let needed = (args.len().max(4) * 4) as u64;
                if target.args.size() < needed {
                    target.args = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("mophila shader args"), size: needed, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
                }
                if !args.is_empty() {
                    let data: Vec<u8> = args.iter().flat_map(|f| f.to_le_bytes()).collect();
                    self.queue.write_buffer(&target.args, 0, &data);
                }
                target.shared = match &req.args {
                    Args::Shared(a) => Some(Rc::clone(a)),
                    Args::Plain(_) => None,
                };
            }
        }
        let grid = (f64::from(req.samples.max(1)).sqrt().ceil()) as u32;
        if let Some(map) = &req.zoom {
            return self.run_zoom(&req, map, key, grid, args_len);
        }
        let pipeline = &self.pipelines[&key];
        let target = self.targets.get_mut(&req.shape).expect("inserted above");
        let uniforms = Uniforms {
            t: req.t,
            origin: req.origin,
            step: req.step,
            w: req.width,
            h: req.height,
            n: args_len,
            s: grid,
            cam: req.camera.unwrap_or(1.0),
            ..Uniforms::default()
        };
        self.queue.write_buffer(&target.uniforms, 0, &uniforms.bytes());

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mophila shader"),
            layout: &pipeline.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: target.uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: target.args.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&target.view) },
            ],
        });
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mophila shader") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("mophila shader"), timestamp_writes: None });
            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(req.width.div_ceil(8), req.height.div_ceil(8), 1);
        }
        self.queue.submit([encoder.finish()]);
        self.overrides.push((target.image.clone(), target.texture.clone()));
        Ok(target.image.clone())
    }

    /// ズーム動画。足りない列だけ帯に足してから、帯を読んで 1 フレームを組む
    fn run_zoom(&mut self, req: &Request, map: &ZoomPath, key: usize, grid: u32, args_len: u32) -> Result<ImageData> {
        // 図形の中心が画面のどこかと、いちばん遠い隅までの距離 (ピクセル)
        let center_px =
            ((map.center.0 - req.origin.0) / req.step.0, (map.center.1 - req.origin.1) / req.step.1);
        let corner = |x: f64, y: f64| ((x - center_px.0).powi(2) + (y - center_px.1).powi(2)).sqrt();
        let radius = corner(0.0, 0.0)
            .max(corner(f64::from(req.width), 0.0))
            .max(corner(0.0, f64::from(req.height)))
            .max(corner(f64::from(req.width), f64::from(req.height)))
            .max(2.0);
        // 帯の細かさは、いちばん外側で 1 テクセル = 1 ピクセルになるように。
        // 列はテクスチャの上限を超えることがあるので (4K で 18500 列)、そのときは層に分ける。
        // 角度方向は方向ごとに画面の縁までの距離で足りるので、その積分 (RING と同じ式) を行数にする。
        // 中心が真ん中でなくても足りるように、左右・上下とも広いほうを使う
        let edge = (
            center_px.0.max(f64::from(req.width) - center_px.0).max(1.0),
            center_px.1.max(f64::from(req.height) - center_px.1).max(1.0),
        );
        let ring_marks = 4.0 * (edge.0 * (edge.1 / edge.0).asinh() + edge.1 * (edge.0 / edge.1).asinh());
        let efolds = (2.0 * radius).ln();
        let max = self.device.limits().max_texture_dimension_2d;
        let rows = (ring_marks.ceil() as u32).next_multiple_of(8).clamp(8, max);
        let want = ((efolds * radius).ceil() as u32 + 8).max(8);
        let wide = want.div_ceil(want.div_ceil(max)).next_multiple_of(8).min(max);
        let ring = wide * want.div_ceil(wide);
        let cpe = radius;
        // ln(スケール) はピクセル単位で持つ (箱 1 あたりの長さと、ピクセル 1 あたりの箱の長さ)
        let ln_px = req.step.0.abs().ln();
        let lnk0 = ln_px + map.ln_scale(0.0);
        let ustart = radius.ln() + lnk0;
        let fresh = self.strips.get(&req.shape).is_none_or(|s| {
            s.size != (req.width, req.height) || s.ring != ring || s.wide != wide || s.rows != rows || (s.ustart - ustart).abs() > 1e-9
        });
        if fresh {
            let strip = self.make_strip(ring, wide, rows, ustart, (req.width, req.height))?;
            self.strips.insert(req.shape, strip);
        }
        let lnk = ln_px + map.ln_scale(req.t);
        let lo = ((lnk0 - lnk) * cpe).floor() as i64 - 2;
        let hi = lo + (efolds * cpe).ceil() as i64 + 4;
        // 続きなら足りない列だけ、飛んだら窓ごと作り直す
        let strip = self.strips.get_mut(&req.shape).expect("inserted above");
        let todo = match strip.have {
            Some((a, b)) if lo >= a && hi <= b => None,
            Some((a, b)) if lo >= a && lo <= b + 1 => Some((b + 1, hi)),
            _ => Some((lo, hi)),
        };
        strip.have = Some((lo, hi));
        self.ensure_frame();
        let pipeline = &self.pipelines[&key];
        let frame = self.frame.as_ref().expect("made above");
        let strip = &self.strips[&req.shape];
        let target = &self.targets[&req.shape];
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mophila zoom") });
        if let Some((a, b)) = todo {
            // 動画の最初のコマは窓ぜんぶを作るので、1 回に投げると数分かかる dispatch になり、
            // GPU のドライバに切り上げられる (NVIDIA なら Xid 109 CTX SWITCH TIMEOUT)。列を区切って投げる
            let step = i64::from(STRIP_TEXELS / rows.max(1)).max(1);
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mophila zoom strip"),
                layout: &pipeline.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: target.uniforms.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: target.args.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&strip.view) },
                ],
            });
            let mut at = a;
            while at <= b {
                let end = (at + step - 1).min(b);
                let count = (end - at + 1) as u32;
                let uniforms = Uniforms {
                    t: req.t,
                    w: count,
                    h: rows,
                    n: args_len,
                    s: grid,
                    ustart,
                    step: (1.0 / cpe, std::f64::consts::TAU / f64::from(rows)),
                    c0: at as i32,
                    ring,
                    rows,
                    edge,
                    wide,
                    ..Uniforms::default()
                };
                self.queue.write_buffer(&target.uniforms, 0, &uniforms.bytes());
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("mophila zoom strip"), timestamp_writes: None });
                pass.set_pipeline(&pipeline.strip);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(count.div_ceil(8), rows.div_ceil(8), 1);
                drop(pass);
                // uniform を書き換えて次の区切りを投げるので、ここで 1 度流す
                self.queue.submit([encoder.finish()]);
                encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mophila zoom") });
                at = end + 1;
            }
        }
        let uniforms = Uniforms {
            t: req.t,
            w: req.width,
            h: req.height,
            center: center_px,
            ustart,
            lnk,
            cpe,
            ring,
            rows,
            edge,
            wide,
            ..Uniforms::default()
        };
        self.queue.write_buffer(&target.uniforms, 0, &uniforms.bytes());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mophila zoom frame"),
            layout: &frame.1,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: target.uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&strip.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&frame.2) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&target.view) },
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("mophila zoom frame"), timestamp_writes: None });
            pass.set_pipeline(&frame.0);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(req.width.div_ceil(8), req.height.div_ceil(8), 1);
        }
        self.queue.submit([encoder.finish()]);
        self.overrides.push((target.image.clone(), target.texture.clone()));
        Ok(target.image.clone())
    }

    /// 帯を作る。列は環状に使い回すので、窓 1 つ分の幅があれば足りる
    fn make_strip(&self, ring: u32, wide: u32, rows: u32, ustart: f64, size: (u32, u32)) -> Result<Strip> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mophila zoom strip"),
            size: wgpu::Extent3d { width: wide + 1, height: rows, depth_or_array_layers: ring / wide },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return err(Kind::OutOfMemory, oom("a zoom strip", ring, rows, self.bytes(), &e));
        }
        Ok(Strip { used: 0, view, ring, wide, rows, ustart, have: None, size })
    }

    /// 帯から 1 フレームを組む pipeline。1 度だけ作る
    fn ensure_frame(&mut self) {
        if self.frame.is_some() {
            return;
        }
        let source = format!("{UNIFORM}{RING}{FRAME}");
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("mophila zoom frame"), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty, count: None };
        let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mophila zoom frame"),
            entries: &[
                entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                entry(1, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2Array, multisampled: false }),
                entry(2, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(3, wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba8Unorm, view_dimension: wgpu::TextureViewDimension::D2Array }),
            ],
        });
        let pipeline_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("mophila zoom frame"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("mophila zoom frame"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        // 角度は端で回り込む。列は層をまたぐので、こちらで位置を出してから層の中を読む
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mophila zoom strip"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        self.frame = Some((pipeline, layout, sampler));
    }

    fn build_pipeline(&self, wgsl: &str) -> Result<Pipeline> {
        // MOPHILA_WGSL を付けると、組み立てた WGSL をそのまま出す (.moph からの変換を確かめる用)
        if std::env::var_os("MOPHILA_WGSL").is_some() {
            eprintln!("{wgsl}");
        }
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("mophila shader"), source: wgpu::ShaderSource::Wgsl(wgsl.into()) });
        let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mophila shader"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba8Unorm, view_dimension: wgpu::TextureViewDimension::D2Array }, count: None },
            ],
        });
        let pipeline_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("mophila shader"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let make = |entry: &str| {
            self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("mophila shader"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let pipeline = make("main");
        let strip = make("strip");
        if let Some(e) = pollster::block_on(scope.pop()) {
            return err(KIND, format!("the GPU rejected the shader: {e}\n--- WGSL ---\n{wgsl}"));
        }
        Ok(Pipeline { pipeline, strip, layout })
    }

    fn make_target(&self, width: u32, height: u32) -> Result<Target> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mophila shader"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let attach = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let image = ImageData {
            // 中身は使わない。描く直前に override_image でテクスチャに差し替えるので、
            // Vello が見るのは大きさと Blob の番号だけ。空にしておくと全画面 1 枚につき w×h×4 の RAM が浮く
            data: Blob::new(Arc::new(Vec::new())),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width,
            height,
        };
        // Shader と 3D で中身が違うので、大きいほうに合わせる
        let room = UNIFORM_BYTES.max(crate::render::mesh::VIEW_BYTES) as u64;
        let uniforms = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("mophila shader uniforms"), size: room, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let args = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("mophila shader args"), size: 16, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return err(Kind::OutOfMemory, oom("a Shader fill", width, height, self.bytes(), &e));
        }
        Ok(Target { used: 0, texture, view, attach, depth: None, points: None, width, height, image, uniforms, args, shared: None })
    }
}

/// 足りなかったときの言い方。何を、どれだけ、いまいくら持っているか
fn oom(what: &str, width: u32, height: u32, held: u64, e: &wgpu::Error) -> String {
    let bytes = u64::from(width) * u64::from(height) * 4;
    format!(
        "could not get {width}x{height} ({:.1}MB) on the GPU for {what}; \
         this shader runner already holds {:.1}MB. Make the shape smaller, lower --size, \
         or split the scene. ({e})",
        bytes as f64 / 1_048_576.0,
        held as f64 / 1_048_576.0,
    )
}

/// 描画の直前に呼ぶ。この フレームで計算した画像を、Vello の画像キャッシュにテクスチャごと登録する
pub fn apply_overrides(renderer: &mut vello::Renderer, runner: Option<&mut ShaderRunner>) {
    let Some(runner) = runner else { return };
    // 手放したものは、Vello 側の登録を外さないと、あちらが掴んだままになる
    for image in runner.released.drain(..) {
        renderer.unregister_texture(image);
    }
    for (image, texture) in runner.overrides.drain(..) {
        renderer.override_image(&image, Some(wgpu::TexelCopyTextureInfoBase { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }));
    }
}
