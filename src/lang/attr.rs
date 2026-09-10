//! 値の属性の表。
//!
//! 属性 1 つにつき、読み方と書き方を同じ腕に並べて書く。属性を足すときは必ず両方を
//! 書くことになるので、「代入はできるのに読めない」というような食い違いが起きない。
//! 読み (`x.attr`)、書き (`x.attr = v`)、入れ子の書き (`x.a.b = v`) は、すべてここを引く。

use std::rc::Rc;

use crate::lang::error::{MophError, Result, err};
use crate::lang::eval::Interp;
use crate::lang::value::{Record, Value};

/// 書いた結果。値型は作り直すので、入れ物を持っている側が入れ直す
pub enum Written {
    Replace(Value),
    /// 参照型はその場で書き換わった
    Done,
}

type Get<'a> = Box<dyn Fn() -> Result<Value> + 'a>;
type Set<'a> = Box<dyn FnOnce(Value) -> Result<Written> + 'a>;

/// 属性 1 つ。`set` がなければ読み取り専用
pub struct Attr<'a> {
    get: Get<'a>,
    set: Option<Set<'a>>,
}

impl<'a> Attr<'a> {
    /// 読み書きできる属性
    fn rw(get: impl Fn() -> Result<Value> + 'a, set: impl FnOnce(Value) -> Result<Written> + 'a) -> Self {
        Attr { get: Box::new(get), set: Some(Box::new(set)) }
    }

    /// 読むだけの属性
    fn ro(get: impl Fn() -> Result<Value> + 'a) -> Self {
        Attr { get: Box::new(get), set: None }
    }

    pub fn get(&self) -> Result<Value> {
        (self.get)()
    }

    /// `what` は "Audio.file" のような、エラーに出す名前
    pub fn set(self, what: &str, v: Value) -> Result<Written> {
        match self.set {
            Some(set) => set(v),
            None => err("NameError.UndefinedAttribute", format!("{what} cannot be assigned")),
        }
    }
}

/// 属性の値が Number でなければエラー
fn number(what: &str, v: Value) -> Result<f64> {
    match v {
        Value::Number(n, _) => Ok(n),
        v => err("TypeError.AttributeType", format!("{what} expects Number, found {}", v.type_name())),
    }
}

fn wrote(v: Value) -> Result<Written> {
    Ok(Written::Replace(v))
}

impl Interp {
    /// 値が持つ属性。ない属性は None
    pub fn attr<'a>(&'a self, target: &'a Value, name: &'a str) -> Option<Attr<'a>> {
        // 0..255 で見せている色の成分 (new Color { } と同じ単位)
        let channel = |i: usize, scale: f64| move || Ok(Value::num(component(target, i, scale)));
        Some(match (target, name) {
            (Value::Vector(x, y), "x") => Attr::rw(move || Ok(Value::num(*x)), move |v| wrote(Value::Vector(number("x", v)?, *y))),
            (Value::Vector(x, y), "y") => Attr::rw(move || Ok(Value::num(*y)), move |v| wrote(Value::Vector(*x, number("y", v)?))),
            (Value::Apos(a, x, y), "x") => Attr::rw(move || Ok(Value::num(*x)), move |v| wrote(Value::Apos(a.clone(), number("x", v)?, *y))),
            (Value::Apos(a, x, y), "y") => Attr::rw(move || Ok(Value::num(*y)), move |v| wrote(Value::Apos(a.clone(), *x, number("y", v)?))),
            (Value::Apos(a, x, y), "vector") => Attr::rw(
                move || Ok(Value::Vector(*x, *y)),
                move |v| match v {
                    Value::Vector(nx, ny) => wrote(Value::Apos(a.clone(), nx, ny)),
                    v => err("TypeError.AttributeType", format!("vector expects Vector, found {}", v.type_name())),
                },
            ),
            (Value::Apos(_, x, y), "anchor") => Attr::rw(
                move || Ok(Value::Symbol(anchor(target))),
                move |v| match v {
                    Value::Symbol(s) => wrote(Value::Apos(s, *x, *y)),
                    v => err("TypeError.AttributeType", format!("anchor expects Anchor, found {}", v.type_name())),
                },
            ),
            // 色の成分は読むだけ。書くときは Color を作り直す
            (Value::Color(_), "r") => Attr::ro(channel(0, 255.0)),
            (Value::Color(_), "g") => Attr::ro(channel(1, 255.0)),
            (Value::Color(_), "b") => Attr::ro(channel(2, 255.0)),
            (Value::Color(_), "a") => Attr::ro(channel(3, 1.0)),
            // (x, y) は Vector と同じように読める
            (Value::Tuple(items), "x" | "y") if items.len() == 2 => {
                let i = if name == "x" { 0 } else { 1 };
                Attr::ro(move || Ok(items[i].clone()))
            }
            (Value::Timeline(t), "duration") => Attr::rw(move || Ok(Value::Duration(t.duration())), move |v| set_duration(&t.duration, v)),
            (Value::Motion(m), "duration") => Attr::rw(move || Ok(Value::Duration(m.duration())), move |v| set_duration(&m.duration, v)),
            (Value::Audio(a), "duration") => Attr::ro(move || Ok(Value::Duration(a.length))),
            (Value::Audio(a), "file") => Attr::ro(move || Ok(Value::Str(a.name.clone()))),
            (Value::Module(m), name) if m.items.contains_key(name) => Attr::ro(move || Ok(m.items[name].clone())),
            // struct / 組み込みの型のインスタンス。属性は宣言か schema が決める
            (Value::Object(o), name) => Attr::rw(
                move || {
                    self.field_ok(&o.borrow().decl, name)?;
                    let o = o.borrow();
                    o.attrs
                        .get(name)
                        .cloned()
                        .ok_or_else(|| MophError::new("NameError.UndefinedAttribute", format!("{} has no attribute \"{name}\"", o.kind)))
                },
                move |v| {
                    self.field_ok(&o.borrow().decl, name)?;
                    self.set_attr(o, name, v)?;
                    Ok(Written::Done)
                },
            ),
            // record は書き換えられないので、同じフィールドだけ差し替えた record を作る
            (Value::Record(r), name) if r.fields.iter().any(|(f, _)| f == name) => Attr::rw(
                move || {
                    self.field_ok(&r.decl, name)?;
                    Ok(r.fields.iter().find(|(f, _)| f == name).map(|(_, v)| v.clone()).expect("field"))
                },
                move |v| {
                    self.field_ok(&r.decl, name)?;
                    let fields = r.fields.iter().map(|(f, old)| (f.clone(), if f == name { v.clone() } else { old.clone() })).collect();
                    wrote(Value::Record(Rc::new(Record { name: r.name.clone(), decl: r.decl.clone(), fields })))
                },
            ),
            _ => return None,
        })
    }

    /// 属性のパスに値を書く。[position, x] なら position (Pos) の x だけを変える。
    /// 値型は作り直しになるので、書き換えた受け手を返す
    pub fn write_path(&self, target: Value, path: &[String], value: Value) -> Result<Value> {
        let [name, rest @ ..] = path else { return Ok(value) };
        let what = format!("{}.{name}", target.type_name());
        let Some(attr) = self.attr(&target, name) else {
            return err("NameError.UndefinedAttribute", format!("{} has no attribute \"{name}\"", target.type_name()));
        };
        let value = if rest.is_empty() { value } else { self.write_path(attr.get()?, rest, value)? };
        match attr.set(&what, value)? {
            Written::Replace(v) => Ok(v),
            Written::Done => Ok(target),
        }
    }
}

/// Color の成分。表示は 3 桁で丸める
fn component(v: &Value, i: usize, scale: f64) -> f64 {
    let Value::Color(c) = v else { return 0.0 };
    (c[i] as f64 * scale * 1000.0).round() / 1000.0
}

fn anchor(v: &Value) -> String {
    match v {
        Value::Apos(a, _, _) => a.clone(),
        _ => String::new(),
    }
}

fn set_duration(slot: &std::cell::Cell<Option<f64>>, v: Value) -> Result<Written> {
    match v {
        Value::Duration(d) => {
            slot.set(Some(d));
            Ok(Written::Done)
        }
        v => err("TypeError.AttributeType", format!("duration expects Duration, found {}", v.type_name())),
    }
}
