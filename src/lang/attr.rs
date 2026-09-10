//! 値の属性の表。
//!
//! 属性 1 つにつき、読み方と書き方を同じ腕に並べて書く。属性を足すときは必ず両方を
//! 書くことになるので、「代入はできるのに読めない」というような食い違いが起きない。
//! 読み (`x.attr`)、書き (`x.attr = v`)、入れ子の書き (`x.a.b = v`) は、すべてここを引く。

use std::rc::Rc;

use crate::lang::error::{Kind, MophError, Result, err};
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
            None => err(Kind::UndefinedAttribute, format!("{what} cannot be assigned")),
        }
    }
}

/// 属性の値が Number でなければエラー
fn number(what: &str, v: Value) -> Result<f64> {
    match v {
        Value::Number(n, _) => Ok(n),
        v => err(Kind::AttributeType, format!("{what} expects Number, found {}", v.type_name())),
    }
}

fn wrote(v: Value) -> Result<Written> {
    Ok(Written::Replace(v))
}

/// builtin の値が持つ属性 1 つ。ここが唯一の定義で、実行時の読み書きも
/// `mophila doc` が出す説明も、同じ表を引く
pub struct AttrDef {
    /// この属性を持つ型
    pub receivers: &'static [&'static str],
    pub name: &'static str,
    pub ty: &'static str,
    pub doc: &'static str,
    pub get: fn(&Value) -> Result<Value>,
    /// None なら読み取り専用
    pub set: Option<fn(&Value, Value) -> Result<Written>>,
}

pub const ATTRS: &[AttrDef] = &[
    AttrDef { receivers: &["Vector", "Pos"], name: "x", ty: "Number", doc: "横の座標", get: get_x, set: Some(set_x) },
    AttrDef { receivers: &["Vector", "Pos"], name: "y", ty: "Number", doc: "縦の座標", get: get_y, set: Some(set_y) },
    AttrDef { receivers: &["Pos"], name: "anchor", ty: "Anchor", doc: "その座標が図形のどこを指すか", get: get_anchor, set: Some(set_anchor) },
    AttrDef { receivers: &["Pos"], name: "vector", ty: "Vector", doc: "基準点を外した座標", get: get_vector, set: Some(set_vector) },
    AttrDef { receivers: &["Color"], name: "r", ty: "Number", doc: "赤 0..255", get: get_r, set: None },
    AttrDef { receivers: &["Color"], name: "g", ty: "Number", doc: "緑 0..255", get: get_g, set: None },
    AttrDef { receivers: &["Color"], name: "b", ty: "Number", doc: "青 0..255", get: get_b, set: None },
    AttrDef { receivers: &["Color"], name: "a", ty: "Number", doc: "不透明度 0..1", get: get_a, set: None },
    AttrDef { receivers: &["Tuple"], name: "x", ty: "T", doc: "2 要素の組の 1 つ目 (Vector と同じ書き方で読める)", get: get_tuple_x, set: None },
    AttrDef { receivers: &["Tuple"], name: "y", ty: "T", doc: "2 要素の組の 2 つ目", get: get_tuple_y, set: None },
    AttrDef {
        receivers: &["Timeline", "Motion"],
        name: "duration",
        ty: "Duration",
        doc: "長さ。代入すると長さだけが変わる (行の時刻は動かない)。0..1 で書いた表では、これが実時間を決める。時刻ごと動かすなら scale / fit / trim",
        get: get_duration,
        set: Some(set_duration_attr),
    },
    AttrDef { receivers: &["Audio"], name: "duration", ty: "Duration", doc: "音声ファイルの長さ", get: get_duration, set: None },
    AttrDef { receivers: &["Audio"], name: "file", ty: "String", doc: "import に書いたパス", get: get_file, set: None },
];

fn def_of(receiver: &str, name: &str) -> Option<&'static AttrDef> {
    ATTRS.iter().find(|a| a.name == name && a.receivers.contains(&receiver))
}

fn nums(v: &Value) -> (f64, f64) {
    match v {
        Value::Vector(x, y) | Value::Apos(_, x, y) => (*x, *y),
        _ => (0.0, 0.0),
    }
}

fn rebuilt(v: &Value, x: f64, y: f64) -> Value {
    match v {
        Value::Apos(a, ..) => Value::Apos(a.clone(), x, y),
        _ => Value::Vector(x, y),
    }
}

fn get_x(v: &Value) -> Result<Value> {
    Ok(Value::num(nums(v).0))
}

fn get_y(v: &Value) -> Result<Value> {
    Ok(Value::num(nums(v).1))
}

fn set_x(v: &Value, nv: Value) -> Result<Written> {
    let (_, y) = nums(v);
    wrote(rebuilt(v, number("x", nv)?, y))
}

fn set_y(v: &Value, nv: Value) -> Result<Written> {
    let (x, _) = nums(v);
    wrote(rebuilt(v, x, number("y", nv)?))
}

fn get_anchor(v: &Value) -> Result<Value> {
    match v {
        Value::Apos(a, ..) => Ok(Value::Symbol(a.clone())),
        _ => Ok(Value::Symbol(String::new())),
    }
}

fn set_anchor(v: &Value, nv: Value) -> Result<Written> {
    let (x, y) = nums(v);
    match nv {
        Value::Symbol(s) => wrote(Value::Apos(s, x, y)),
        v => err(Kind::AttributeType, format!("anchor expects Anchor, found {}", v.type_name())),
    }
}

fn get_vector(v: &Value) -> Result<Value> {
    let (x, y) = nums(v);
    Ok(Value::Vector(x, y))
}

fn set_vector(v: &Value, nv: Value) -> Result<Written> {
    match nv {
        Value::Vector(x, y) => wrote(rebuilt(v, x, y)),
        v => err(Kind::AttributeType, format!("vector expects Vector, found {}", v.type_name())),
    }
}

/// 色の成分。r g b は 0..255、a は 0..1 (Color を作るときと同じ単位)
fn channel(v: &Value, i: usize, scale: f64) -> Result<Value> {
    let Value::Color(c) = v else { return Ok(Value::num(0.0)) };
    Ok(Value::num((c[i] as f64 * scale * 1000.0).round() / 1000.0))
}

fn get_r(v: &Value) -> Result<Value> {
    channel(v, 0, 255.0)
}

fn get_g(v: &Value) -> Result<Value> {
    channel(v, 1, 255.0)
}

fn get_b(v: &Value) -> Result<Value> {
    channel(v, 2, 255.0)
}

fn get_a(v: &Value) -> Result<Value> {
    channel(v, 3, 1.0)
}

fn tuple_at(v: &Value, i: usize) -> Result<Value> {
    match v {
        Value::Tuple(items) if items.len() == 2 => Ok(items[i].clone()),
        v => err(Kind::UndefinedAttribute, format!("{} has no attribute here", v.type_name())),
    }
}

fn get_tuple_x(v: &Value) -> Result<Value> {
    tuple_at(v, 0)
}

fn get_tuple_y(v: &Value) -> Result<Value> {
    tuple_at(v, 1)
}

fn get_duration(v: &Value) -> Result<Value> {
    Ok(Value::Duration(match v {
        Value::Timeline(t) => t.duration(),
        Value::Motion(m) => m.duration(),
        Value::Audio(a) => a.length,
        _ => 0.0,
    }))
}

fn set_duration_attr(v: &Value, nv: Value) -> Result<Written> {
    let Value::Duration(d) = nv else {
        return err(Kind::AttributeType, format!("duration expects Duration, found {}", nv.type_name()));
    };
    match v {
        Value::Timeline(t) => t.duration.set(Some(d)),
        Value::Motion(m) => m.duration.set(Some(d)),
        _ => return err(Kind::AttributeType, "duration cannot be assigned here"),
    }
    Ok(Written::Done)
}

fn get_file(v: &Value) -> Result<Value> {
    match v {
        Value::Audio(a) => Ok(Value::Str(a.name.clone())),
        v => err(Kind::UndefinedAttribute, format!("{} has no file", v.type_name())),
    }
}

impl Interp {
    /// 値が持つ属性。ない属性は None
    pub fn attr<'a>(&'a self, target: &'a Value, name: &'a str) -> Option<Attr<'a>> {
        if let Some(def) = def_of(&target.type_name(), name) {
            let get = move || (def.get)(target);
            return Some(match def.set {
                Some(set) => Attr::rw(get, move |v| set(target, v)),
                None => Attr::ro(get),
            });
        }
        Some(match (target, name) {
            (Value::Module(m), name) if m.items.contains_key(name) => Attr::ro(move || Ok(m.items[name].clone())),
            // struct / builtin の型のインスタンス。属性は宣言か schema が決める
            (Value::Object(o), name) => Attr::rw(
                move || {
                    self.field_ok(&o.borrow().decl, name)?;
                    let o = o.borrow();
                    o.attrs
                        .get(name)
                        .cloned()
                        .ok_or_else(|| MophError::new(Kind::UndefinedAttribute, format!("{} has no attribute \"{name}\"", o.kind)))
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
            return err(Kind::UndefinedAttribute, format!("{} has no attribute \"{name}\"", target.type_name()));
        };
        let value = if rest.is_empty() { value } else { self.write_path(attr.get()?, rest, value)? };
        match attr.set(&what, value)? {
            Written::Replace(v) => Ok(v),
            Written::Done => Ok(target),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// receivers に書いた型名が、docs が知っている型かどうか
    #[test]
    fn every_receiver_is_a_known_type() {
        for a in ATTRS {
            for r in a.receivers {
                assert!(crate::docs::TYPES.iter().any(|t| t.name == *r), "{r}.{} の受け手はbuiltin の型ではない", a.name);
                assert!(def_of(r, a.name).is_some(), "{r}.{} を引けない", a.name);
            }
        }
    }

    /// 同じ (受け手, 名前) が 2 つあると、後のほうが読まれずに死ぬ
    #[test]
    fn no_duplicate_attr() {
        let mut seen: Vec<(&str, &str)> = Vec::new();
        for a in ATTRS {
            for r in a.receivers {
                assert!(!seen.contains(&(r, a.name)), "{r}.{} が 2 つある", a.name);
                seen.push((r, a.name));
            }
        }
    }
}
