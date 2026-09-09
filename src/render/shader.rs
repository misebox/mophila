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
use crate::lang::error::{MophError, Result, err};
use crate::lang::value::{Closure, Value};

const KIND: &str = "RuntimeError.ShaderCompile";

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
            return err("TypeError.ArityMismatch", format!("shader function takes {} arguments, {} given", params.len(), arg_tys.len()));
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
                    return err("TypeError.OperandType", format!("cannot assign {} to {name} ({})", ty.name(), expected.name()));
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
                        return err("TypeError.OperandType", format!("cannot assign {} to {name} ({})", ty.name(), expected.name()));
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
                    return err("TypeError.OperandType", "range bounds must be Number");
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
            _ => err(KIND, "this statement is not allowed in a shader (only let, assignment, for over a range, return, and expressions)"),
        }
    }

    /// if。両方の枝が値を持ち、値が要るなら一時変数に入れて式の値にする
    #[allow(clippy::too_many_arguments)]
    fn if_(&mut self, cond: &Expr, then: &[Stmt], otherwise: Option<&[Stmt]>, out: &mut Body, env: &mut Env, closure: &Closure, ret: &mut Option<Ty>, want_value: bool) -> Result<Option<(String, Ty)>> {
        let (c, ct) = self.expr(cond, out, env, closure, ret)?;
        if ct != Ty::Bool {
            return err("TypeError.OperandType", format!("if condition must be Bool, found {}", ct.name()));
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
                    return err("TypeError.OperandType", format!("cannot negate {}", ty.name()));
                }
                (format!("(-{v})"), ty)
            }
            Expr::Not(inner) => {
                let (v, ty) = self.expr(inner, out, env, closure, ret)?;
                if ty != Ty::Bool {
                    return err("TypeError.OperandType", format!("cannot apply not to {}", ty.name()));
                }
                (format!("(!{v})"), Ty::Bool)
            }
            Expr::Binary(op, l, r) => {
                let (lv, lt) = self.expr(l, out, env, closure, ret)?;
                let (rv, rt) = self.expr(r, out, env, closure, ret)?;
                binary(*op, &lv, lt, &rv, rt)?
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
                        return err("NameError.UndefinedAttribute", format!("{} has no attribute \"{attr}\" in a shader", ty.name()));
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
                    Expr::Ident(fname) => {
                        if lookup(env, fname).is_some() {
                            return err(KIND, format!("\"{fname}\" is a variable, not a function"));
                        }
                        let Some(Value::Func(f)) = find_captured(fname, closure) else {
                            return err("NameError.UndefinedVariable", format!("\"{fname}\" is not a function known to the shader"));
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
                    return err("TypeError.OperandType", "index must be Number");
                }
                let elem = match tt {
                    Ty::Nums(_) => Ty::Num,
                    Ty::Colors(_) => Ty::Color,
                    Ty::Vecs(_) => Ty::Vec,
                    other => return err("TypeError.OperandType", format!("cannot index {}", other.name())),
                };
                (format!("{tv}[u32({iv})]"), elem)
            }
            Expr::Specific(name, args) if name == "vector" => {
                let mut parts = Vec::new();
                for a in args {
                    let (v, ty) = self.expr(a, out, env, closure, ret)?;
                    if ty != Ty::Num {
                        return err("TypeError.ArgumentType", format!("vector! expects Number, found {}", ty.name()));
                    }
                    parts.push(v);
                }
                let [x, y] = parts.as_slice() else { return err("TypeError.ArityMismatch", "vector! takes 2 arguments") };
                (format!("vec2<f32>({x}, {y})"), Ty::Vec)
            }
            Expr::Tuple(_) => return err(KIND, "a plain tuple is not available in a shader; write vector!(x, y)"),
            Expr::Specific(name, args) if name == "rgb" || name == "rgba" => {
                let mut parts = Vec::new();
                for a in args {
                    let (v, ty) = self.expr(a, out, env, closure, ret)?;
                    if ty != Ty::Num {
                        return err("TypeError.ArgumentType", format!("{name}! expects Number, found {}", ty.name()));
                    }
                    parts.push(v);
                }
                match (name.as_str(), parts.as_slice()) {
                    ("rgb", [r, g, b]) => (format!("vec4<f32>({r} / 255.0, {g} / 255.0, {b} / 255.0, 1.0)"), Ty::Color),
                    ("rgba", [r, g, b, a]) => (format!("vec4<f32>({r} / 255.0, {g} / 255.0, {b} / 255.0, {a})"), Ty::Color),
                    _ => return err("TypeError.ArityMismatch", format!("{name}! takes {} arguments", if name == "rgb" { 3 } else { 4 })),
                }
            }
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
            return err("NameError.UndefinedVariable", format!("\"{name}\" is not defined"));
        };
        Ok(match v {
            Value::Number(n) => (lit(n), Ty::Num),
            Value::Bool(b) => (b.to_string(), Ty::Bool),
            Value::Color(c) => (color_lit(&c), Ty::Color),
            Value::Vector(x, y) => (vec_lit(x, y), Ty::Vec),
            Value::List(items) => {
                let key = Rc::as_ptr(&items) as usize;
                if let Some(found) = self.consts.get(&key) {
                    return Ok(found.clone());
                }
                let items = items.borrow();
                let (ty, elems): (Ty, Vec<String>) = if items.iter().all(|v| matches!(v, Value::Number(_))) {
                    (Ty::Nums(Some(items.len())), items.iter().map(|v| match v { Value::Number(n) => lit(*n), _ => unreachable!() }).collect())
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
        Some(existing) if *existing != ty => err("TypeError.OperandType", format!("shader function returns both {} and {}", existing.name(), ty.name())),
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
    let bad = || MophError::new("TypeError.OperandType", format!("cannot {} {} and {} in a shader", crate::lang::eval::verb(op), lt.name(), rt.name()));
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

fn math_call(name: &str, args: &[(String, Ty)]) -> Result<(String, Ty)> {
    if args.iter().any(|(_, t)| *t != Ty::Num) {
        return err("TypeError.ArgumentType", format!("math.{name} takes Numbers"));
    }
    let list: Vec<&str> = args.iter().map(|(v, _)| v.as_str()).collect();
    let unary = |f: &str| -> Result<(String, Ty)> {
        match list.as_slice() {
            [x] => Ok((format!("{f}({x})"), Ty::Num)),
            _ => err("TypeError.ArityMismatch", format!("math.{name} takes 1 argument, {} given", list.len())),
        }
    };
    match name {
        "sin" | "cos" | "floor" | "ceil" | "abs" | "sqrt" | "exp" => unary(name),
        "ln" => unary("log"),
        "atan2" => match list.as_slice() {
            [y, x] => Ok((format!("atan2({y}, {x})"), Ty::Num)),
            _ => err("TypeError.ArityMismatch", "math.atan2 takes 2 arguments (y, x)"),
        },
        "max" | "min" => match list.as_slice() {
            [] => err("TypeError.ArityMismatch", format!("math.{name} needs at least 1 argument")),
            [x] => Ok((x.to_string(), Ty::Num)),
            _ => Ok((list[1..].iter().fold(list[0].to_string(), |acc, x| format!("{name}({acc}, {x})")), Ty::Num)),
        },
        _ => err("NameError.UndefinedAttribute", format!("math has no \"{name}\" usable in a shader")),
    }
}

/// color: func (x, y, t [, args]) を、ピクセルごとに走る compute shader の WGSL にする
pub fn compile(closure: &Closure) -> Result<String> {
    let params = &closure.def.params;
    if params.len() != 3 && params.len() != 4 {
        return err("TypeError.ArityMismatch", format!("Shader.color must be func (x, y, t) or func (x, y, t, args), found {} parameters", params.len()));
    }
    let mut g = Gen { decls: Vec::new(), funcs: HashMap::new(), consts: HashMap::new(), counter: 0, stack: Vec::new() };
    let mut env: Env = vec![HashMap::new()];
    let names = ["x", "y", "t"];
    for (i, p) in params.iter().enumerate() {
        let Pattern::Name(n) = &p.pattern else { return err(KIND, "Shader.color parameters must be plain names") };
        let entry = if i < 3 { (names[i].to_string(), Ty::Num) } else { ("args".to_string(), Ty::Nums(None)) };
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
        return err("TypeError.ArgumentType", format!("Shader.color must return Color, found {}", ret.map_or("nothing".to_string(), Ty::name)));
    }
    let mut wgsl = String::from(
        "struct U { t: f32, ox: f32, oy: f32, sx: f32, sy: f32, w: u32, h: u32, n: u32, s: u32, p0: u32, p1: u32, p2: u32 }\n\
         @group(0) @binding(0) var<uniform> u: U;\n\
         @group(0) @binding(1) var<storage, read> args: array<f32>;\n\
         @group(0) @binding(2) var out: texture_storage_2d<rgba8unorm, write>;\n\n",
    );
    for d in &g.decls {
        wgsl.push_str(d);
        wgsl.push('\n');
    }
    wgsl.push_str("fn color(x: f32, y: f32, t: f32) -> vec4<f32> {\n");
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
         \x20           acc += clamp(color(x, y, u.t), vec4<f32>(0.0), vec4<f32>(1.0));\n\
         \x20       }\n\
         \x20   }\n\
         \x20   textureStore(out, vec2<i32>(i32(id.x), i32(id.y)), acc / f32(g * g));\n\
         }\n",
    );
    Ok(wgsl)
}

// ---------- GPU で走らせる ----------

/// 1 つの図形の塗りの依頼
pub struct Request<'a> {
    /// 図形の実体 (テクスチャの使い回しの鍵)
    pub shape: usize,
    pub closure: &'a Rc<Closure>,
    pub args: &'a [f32],
    pub t: f64,
    pub width: u32,
    pub height: u32,
    /// ピクセル (0, 0) の中心の箱の座標と、1 ピクセルあたりの箱の座標の増分
    pub origin: (f64, f64),
    pub step: (f64, f64),
    /// 1 ピクセルあたりのサンプル数。平方数に切り上げる (4 → 2x2)
    pub samples: u32,
}

struct Pipeline {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    /// Vello に渡す画像。中身は使わず、テクスチャで差し替える
    image: ImageData,
    uniforms: wgpu::Buffer,
    args: wgpu::Buffer,
}

pub struct ShaderRunner {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// 関数の実体 → pipeline
    pipelines: HashMap<usize, Pipeline>,
    /// 図形 → テクスチャ
    targets: HashMap<usize, Target>,
    /// 描画の前に Vello へ登録する (画像, テクスチャ)
    pub overrides: Vec<(ImageData, wgpu::Texture)>,
}

impl ShaderRunner {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self { device, queue, pipelines: HashMap::new(), targets: HashMap::new(), overrides: Vec::new() }
    }

    /// compute shader を投入し、塗りに使う画像を返す。描画の前に overrides を Vello に登録すること
    pub fn run(&mut self, req: Request) -> Result<ImageData> {
        let key = Rc::as_ptr(req.closure) as usize;
        if !self.pipelines.contains_key(&key) {
            let wgsl = compile(req.closure)?;
            let pipeline = self.build_pipeline(&wgsl)?;
            self.pipelines.insert(key, pipeline);
        }
        let needs_new = self.targets.get(&req.shape).is_none_or(|t| t.width != req.width || t.height != req.height);
        if needs_new {
            let target = self.make_target(req.width, req.height);
            self.targets.insert(req.shape, target);
        }
        let pipeline = &self.pipelines[&key];
        let target = self.targets.get_mut(&req.shape).expect("inserted above");

        // uniform: f32 x5, u32 x4, 詰め物 x3 (16 バイトの倍数にする)
        let grid = (f64::from(req.samples.max(1)).sqrt().ceil()) as u32;
        let mut bytes = Vec::with_capacity(48);
        for f in [req.t as f32, req.origin.0 as f32, req.origin.1 as f32, req.step.0 as f32, req.step.1 as f32] {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        for n in [req.width, req.height, req.args.len() as u32, grid, 0, 0, 0] {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        self.queue.write_buffer(&target.uniforms, 0, &bytes);
        let needed = (req.args.len().max(4) * 4) as u64;
        if target.args.size() < needed {
            target.args = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("mophila shader args"), size: needed, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        }
        if !req.args.is_empty() {
            let data: Vec<u8> = req.args.iter().flat_map(|f| f.to_le_bytes()).collect();
            self.queue.write_buffer(&target.args, 0, &data);
        }

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

    fn build_pipeline(&self, wgsl: &str) -> Result<Pipeline> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("mophila shader"), source: wgpu::ShaderSource::Wgsl(wgsl.into()) });
        let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mophila shader"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba8Unorm, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            ],
        });
        let pipeline_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("mophila shader"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("mophila shader"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return err(KIND, format!("the GPU rejected the shader: {e}\n--- WGSL ---\n{wgsl}"));
        }
        Ok(Pipeline { pipeline, layout })
    }

    fn make_target(&self, width: u32, height: u32) -> Target {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mophila shader"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let image = ImageData {
            data: Blob::new(Arc::new(vec![0u8; (width * height * 4) as usize])),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width,
            height,
        };
        let uniforms = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("mophila shader uniforms"), size: 48, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let args = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("mophila shader args"), size: 16, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Target { texture, view, width, height, image, uniforms, args }
    }
}

/// 描画の直前に呼ぶ。この フレームで計算した画像を、Vello の画像キャッシュにテクスチャごと登録する
pub fn apply_overrides(renderer: &mut vello::Renderer, runner: Option<&mut ShaderRunner>) {
    let Some(runner) = runner else { return };
    for (image, texture) in runner.overrides.drain(..) {
        renderer.override_image(&image, Some(wgpu::TexelCopyTextureInfoBase { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }));
    }
}
