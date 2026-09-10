//! 組み込みの値が持つメソッドの表。
//!
//! 1 つのメソッドにつき、呼び出し先と説明を同じ場所に書く。ここが唯一の定義で、
//! 実行時の呼び出しも `mophila doc` が出す説明も同じ表を引く。片方だけ増えたり、
//! 実装していないメソッドが文書に載ったりしない。

use std::cell::RefCell;
use std::rc::Rc;

use crate::lang::error::{Result, err};
use crate::lang::eval::{Interp, cmp, equals, format, whole};
use crate::lang::value::{Motion, Timeline, Value};

type Args = Vec<(Option<String>, Value)>;
type Call = fn(&mut Interp, Value, Args) -> Result<Value>;

pub struct Method {
    /// このメソッドを持つ型
    pub receivers: &'static [&'static str],
    pub name: &'static str,
    pub signature: &'static str,
    pub returns: &'static str,
    pub doc: &'static str,
    pub call: Call,
}

/// 並びのある入れ物 (List / Tuple / Range) が共通で持つメソッドの受け手
const SEQ: &[&str] = &["List", "Tuple", "Range"];

pub const METHODS: &[Method] = &[
    Method { receivers: &["String"], name: "len", signature: "s.len()", returns: "Number", doc: "文字数", call: str_len },
    Method { receivers: &["String"], name: "replace", signature: "s.replace(from: String, to: String)", returns: "String", doc: "from を to に置き換えた文字列", call: str_replace },
    Method {
        receivers: &["String"],
        name: "format",
        signature: "\"{name}\".format(値, ...)",
        returns: "String",
        doc: "{ } を順に引数で置き換える。名前は説明用。Dict を 1 つ渡すと、{ } の中の名前で引く。引数はどの型でもよいので、型を書けない",
        call: str_format,
    },
    Method { receivers: SEQ, name: "len", signature: "xs.len()", returns: "Number", doc: "要素数", call: seq_len },
    Method { receivers: &["List"], name: "push", signature: "xs.push(値: T)", returns: "Nothing", doc: "末尾に追加する。その List 自身が変わる", call: list_push },
    Method { receivers: SEQ, name: "enumerate", signature: "xs.enumerate()", returns: "List<(Number, T)>", doc: "番号と要素の組。for (i, x) in xs.enumerate() で使う", call: seq_enumerate },
    Method { receivers: SEQ, name: "reverse", signature: "xs.reverse()", returns: "List<T>", doc: "逆順にした新しい List", call: seq_reverse },
    Method { receivers: SEQ, name: "contains", signature: "xs.contains(値: T)", returns: "Bool", doc: "その値を含むか", call: seq_contains },
    Method { receivers: SEQ, name: "index_of", signature: "xs.index_of(値: T)", returns: "Number", doc: "最初に現れる位置。無ければ -1", call: seq_index_of },
    Method { receivers: SEQ, name: "sum", signature: "xs.sum()", returns: "Number", doc: "Number の合計", call: seq_sum },
    Method { receivers: SEQ, name: "join", signature: "xs.join(sep: String)", returns: "String", doc: "各要素を文字列にして sep でつなぐ", call: seq_join },
    Method { receivers: SEQ, name: "map", signature: "xs.map(f: (T) -> U)", returns: "List<U>", doc: "各要素に f を通した結果の List", call: seq_map },
    Method { receivers: SEQ, name: "filter", signature: "xs.filter(f: (T) -> Bool)", returns: "List<T>", doc: "f が true を返した要素だけの List", call: seq_filter },
    Method { receivers: SEQ, name: "reduce", signature: "xs.reduce(初期値: U, f: (U, T) -> U)", returns: "U", doc: "初期値から順に f を通して 1 つの値にする", call: seq_reduce },
    Method { receivers: SEQ, name: "sort", signature: "xs.sort()", returns: "List<T>", doc: "昇順に並べた新しい List (要素は Number か Duration)", call: seq_sort },
    Method { receivers: SEQ, name: "zip", signature: "xs.zip(ys: List<U>)", returns: "List<(T, U)>", doc: "同じ位置どうしを組にする。短い方に合わせる", call: seq_zip },
    Method { receivers: &["Tuple", "Range"], name: "to_list", signature: "(0..n).to_list()", returns: "List<T>", doc: "中身を並べた List", call: seq_to_list },
    Method { receivers: &["Range"], name: "steps", signature: "(0..=1).steps(n: Number)", returns: "List<Number>", doc: "両端を含めて n 等分した値の List (要素は n + 1 個)", call: range_steps },
    Method { receivers: &["Dict"], name: "keys", signature: "d.keys()", returns: "List<String>", doc: "キーの List", call: dict_keys },
    Method { receivers: &["Dict"], name: "values", signature: "d.values()", returns: "List<T>", doc: "値の List", call: dict_values },
    Method { receivers: &["Dict"], name: "has", signature: "d.has(key: String)", returns: "Bool", doc: "そのキーがあるか", call: dict_has },
    Method { receivers: &["Dict"], name: "len", signature: "d.len()", returns: "Number", doc: "要素数", call: dict_len },
    Method {
        receivers: &["Timeline"],
        name: "place",
        signature: "tl.place(x: Timeline | View | Audio | Subtitle, at: Duration, fadeIn: Duration, fadeOut: Duration, duration: Duration, volume: Number, loop: Bool)",
        returns: "Nothing",
        doc: "中に 1 本置く。at はこの Timeline の中での開始時刻",
        call: tl_place,
    },
    Method { receivers: &["Timeline", "Motion"], name: "reverse", signature: "tl.reverse()", returns: "Timeline", doc: "時間の向きを逆にしたもの。ease の in と out も入れ替わる", call: reverse },
    Method {
        receivers: &["Timeline", "Motion"],
        name: "scale",
        signature: "tl.scale(k: Number)",
        returns: "Timeline",
        doc: "時間の軸を k 倍したもの。時刻も長さも k 倍になる",
        call: scale,
    },
    Method {
        receivers: &["Timeline", "Motion"],
        name: "fit",
        signature: "tl.fit(d: Duration)",
        returns: "Timeline",
        doc: "長さが d になるように時間の軸を伸縮したもの",
        call: fit,
    },
    Method {
        receivers: &["Timeline", "Motion"],
        name: "trim",
        signature: "tl.trim(from: Duration, to: Duration)",
        returns: "Timeline",
        doc: "from から to までを取り出し、0 から始まるものにする。その区間の外にあるキーフレームと、置いたものは入らない",
        call: trim,
    },
    Method {
        receivers: &["Motion"],
        name: "apply",
        signature: "m.apply(target: Placeable, f: (Placeable, Duration, List<Number>) -> Nothing)",
        returns: "Timeline",
        doc: "各行で f(対象, 時刻, [列...]) を呼び、その中で対象の属性に代入された値をキーフレームにする",
        call: motion_apply,
    },
];

/// 受け手の型と名前でメソッドを引く
pub fn find(receiver: &str, name: &str) -> Option<&'static Method> {
    METHODS.iter().find(|m| m.name == name && m.receivers.contains(&receiver))
}

fn list(items: Vec<Value>) -> Value {
    Value::List(Rc::new(RefCell::new(items)))
}

/// 引数の数が合わないときの言い方
fn arity<T>(m: &str, want: &str, got: usize) -> Result<T> {
    err("TypeError.ArityMismatch", format!("{m} takes {want}, {got} given"))
}

fn text_of(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        _ => String::new(),
    }
}

/// 並びのある入れ物の中身
fn items_of(v: &Value) -> Vec<Value> {
    match v {
        Value::List(items) => items.borrow().clone(),
        Value::Tuple(items) => items.clone(),
        Value::Range(a, b) => (*a..*b).map(|i| Value::num(i as f64)).collect(),
        _ => Vec::new(),
    }
}

fn one(args: &Args) -> Option<&Value> {
    match args.as_slice() {
        [(None, v)] => Some(v),
        _ => None,
    }
}

fn str_len(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    if !args.is_empty() {
        return arity("String.len", "no arguments", args.len());
    }
    Ok(Value::num(text_of(&r).chars().count() as f64))
}

fn str_replace(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let [(None, Value::Str(from)), (None, Value::Str(to))] = args.as_slice() else {
        return err("TypeError.ArgumentType", "String.replace takes two Strings");
    };
    Ok(Value::Str(text_of(&r).replace(from.as_str(), to)))
}

fn str_format(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    format(&text_of(&r), &args.into_iter().map(|(_, v)| v).collect::<Vec<_>>())
}

fn seq_len(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    if !args.is_empty() {
        return arity("len", "no arguments", args.len());
    }
    Ok(Value::num(match &r {
        Value::List(items) => items.borrow().len() as f64,
        Value::Tuple(items) => items.len() as f64,
        Value::Range(a, b) => (b - a).max(0) as f64,
        _ => 0.0,
    }))
}

fn list_push(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let (Value::List(target), Some(v)) = (&r, one(&args)) else {
        return err("TypeError.ArgumentType", "List.push takes one value");
    };
    target.borrow_mut().push(v.clone());
    Ok(Value::Nothing)
}

fn seq_enumerate(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    if !args.is_empty() {
        return arity("enumerate", "no arguments", args.len());
    }
    Ok(list(items_of(&r).into_iter().enumerate().map(|(i, v)| Value::Tuple(vec![Value::num(i as f64), v])).collect()))
}

fn seq_reverse(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    if !args.is_empty() {
        return arity("reverse", "no arguments", args.len());
    }
    Ok(list(items_of(&r).into_iter().rev().collect()))
}

fn seq_to_list(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    if !args.is_empty() {
        return arity("to_list", "no arguments", args.len());
    }
    Ok(list(items_of(&r)))
}

fn seq_contains(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let Some(v) = one(&args) else { return err("TypeError.ArgumentType", "contains takes one value") };
    Ok(Value::Bool(items_of(&r).iter().any(|x| equals(x, v))))
}

fn seq_index_of(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let Some(v) = one(&args) else { return err("TypeError.ArgumentType", "index_of takes one value") };
    Ok(Value::num(items_of(&r).iter().position(|x| equals(x, v)).map_or(-1.0, |i| i as f64)))
}

fn seq_sum(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    if !args.is_empty() {
        return arity("sum", "no arguments", args.len());
    }
    items_of(&r)
        .iter()
        .try_fold(0.0, |acc, v| match v {
            Value::Number(n, _) => Ok(acc + n),
            v => err("TypeError.OperandType", format!("cannot sum {}", v.type_name())),
        })
        .map(Value::num)
}

fn seq_join(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let [(None, Value::Str(sep))] = args.as_slice() else {
        return err("TypeError.ArgumentType", "join takes one String");
    };
    Ok(Value::Str(items_of(&r).iter().map(|v| v.to_string()).collect::<Vec<_>>().join(sep)))
}

fn func_arg<'a>(args: &'a Args, whose: &str, at: usize) -> Result<&'a Value> {
    match args.get(at) {
        Some((None, f @ (Value::Func(_) | Value::Builtin(_)))) => Ok(f),
        Some((_, v)) => err("TypeError.ArgumentType", format!("{whose} expects a func, found {}", v.type_name())),
        None => err("TypeError.ArityMismatch", format!("{whose} needs a func")),
    }
}

fn seq_map(it: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let f = func_arg(&args, "map", 0)?.clone();
    let out = items_of(&r).into_iter().map(|v| it.call_func(&f, vec![v])).collect::<Result<Vec<_>>>()?;
    Ok(list(out))
}

fn seq_filter(it: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let f = func_arg(&args, "filter", 0)?.clone();
    let mut out = Vec::new();
    for v in items_of(&r) {
        match it.call_func(&f, vec![v.clone()])? {
            Value::Bool(true) => out.push(v),
            Value::Bool(false) => {}
            other => return err("TypeError.ArgumentType", format!("filter expects Bool, found {}", other.type_name())),
        }
    }
    Ok(list(out))
}

fn seq_reduce(it: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let Some((None, init)) = args.first() else {
        return err("TypeError.ArityMismatch", "reduce takes an initial value and a func");
    };
    let f = func_arg(&args, "reduce", 1)?.clone();
    let mut acc = init.clone();
    for v in items_of(&r) {
        acc = it.call_func(&f, vec![acc, v])?;
    }
    Ok(acc)
}

fn seq_sort(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    if !args.is_empty() {
        return arity("sort", "no arguments", args.len());
    }
    let mut out = items_of(&r);
    let mut failed = None;
    out.sort_by(|a, b| {
        cmp(a, b).unwrap_or_else(|| {
            failed = Some(format!("cannot compare {} and {}", a.type_name(), b.type_name()));
            std::cmp::Ordering::Equal
        })
    });
    match failed {
        Some(msg) => err("TypeError.OperandType", msg),
        None => Ok(list(out)),
    }
}

fn seq_zip(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let Some(other) = one(&args) else { return err("TypeError.ArgumentType", "zip takes one List") };
    if !matches!(other, Value::List(_) | Value::Tuple(_) | Value::Range(..)) {
        return err("TypeError.ArgumentType", format!("zip expects List, found {}", other.type_name()));
    }
    Ok(list(items_of(&r).into_iter().zip(items_of(other)).map(|(a, b)| Value::Tuple(vec![a, b])).collect()))
}

fn range_steps(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let ([(None, Value::Number(n, _))], Value::Range(a, b)) = (args.as_slice(), &r) else {
        return err("TypeError.ArgumentType", "Range.steps takes one Number");
    };
    // 端を含めて n 等分。Range は end を含まないので、..= で作ったものは b-1 が終端
    let steps = whole(*n, "steps")?;
    let (a, b) = (*a as f64, (*b - 1) as f64);
    Ok(list((0..=steps).map(|i| Value::num(a + (b - a) * i as f64 / n)).collect()))
}

fn entries_of(v: &Value) -> Vec<(String, Value)> {
    match v {
        Value::Dict(entries) => entries.borrow().clone(),
        _ => Vec::new(),
    }
}

fn dict_keys(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    if !args.is_empty() {
        return arity("Dict.keys", "no arguments", args.len());
    }
    Ok(list(entries_of(&r).into_iter().map(|(k, _)| Value::Str(k)).collect()))
}

fn dict_values(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    if !args.is_empty() {
        return arity("Dict.values", "no arguments", args.len());
    }
    Ok(list(entries_of(&r).into_iter().map(|(_, v)| v).collect()))
}

fn dict_has(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let [(None, Value::Str(key))] = args.as_slice() else {
        return err("TypeError.ArgumentType", "Dict.has takes one String");
    };
    Ok(Value::Bool(entries_of(&r).iter().any(|(k, _)| k == key)))
}

fn dict_len(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    if !args.is_empty() {
        return arity("Dict.len", "no arguments", args.len());
    }
    Ok(Value::num(entries_of(&r).len() as f64))
}

fn tl_place(it: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let Value::Timeline(t) = &r else { return err("TypeError.ArgumentType", "place is a method of Timeline") };
    let placed = it.make_placed("Timeline.place", &args)?;
    t.tracks.borrow_mut().push(placed);
    Ok(Value::Nothing)
}

fn reverse(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    if !args.is_empty() {
        return arity("reverse", "no arguments", args.len());
    }
    match &r {
        Value::Timeline(t) => Ok(Value::Timeline(Rc::new(t.reverse()))),
        Value::Motion(m) => Ok(Value::Motion(Rc::new(m.reverse()))),
        v => err("TypeError.ArgumentType", format!("reverse is a method of Timeline and Motion, found {}", v.type_name())),
    }
}

/// Timeline と Motion で同じ形の変換を書く
fn convert(r: &Value, tl: impl Fn(&Timeline) -> Timeline, mo: impl Fn(&Motion) -> Motion, whose: &str) -> Result<Value> {
    match r {
        Value::Timeline(t) => Ok(Value::Timeline(Rc::new(tl(t)))),
        Value::Motion(m) => Ok(Value::Motion(Rc::new(mo(m)))),
        v => err("TypeError.ArgumentType", format!("{whose} is a method of Timeline and Motion, found {}", v.type_name())),
    }
}

fn scale(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let [(None, Value::Number(k, _))] = args.as_slice() else {
        return err("TypeError.ArgumentType", "scale takes one Number");
    };
    if *k <= 0.0 {
        return err("ValueError.OutOfRange", format!("scale expects a positive Number, found {k}"));
    }
    convert(&r, |t| t.scaled(*k), |m| m.scaled(*k), "scale")
}

fn fit(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let [(None, Value::Duration(d))] = args.as_slice() else {
        return err("TypeError.ArgumentType", "fit takes one Duration");
    };
    convert(&r, |t| t.fit(*d), |m| m.fit(*d), "fit")
}

fn trim(_: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let mut span = [None, None];
    for (i, (name, v)) in args.iter().enumerate() {
        let slot = match name.as_deref() {
            Some("from") => 0,
            Some("to") => 1,
            Some(n) => return err("TypeError.ArgumentType", format!("trim has no argument \"{n}\"; it takes from and to")),
            None if i < 2 => i,
            None => return arity("trim", "at most 2 arguments", args.len()),
        };
        let Value::Duration(d) = v else {
            return err("TypeError.ArgumentType", format!("trim expects Duration, found {}", v.type_name()));
        };
        span[slot] = Some(*d);
    }
    let whole_length = match &r {
        Value::Timeline(t) => t.duration(),
        Value::Motion(m) => m.duration(),
        v => return err("TypeError.ArgumentType", format!("trim is a method of Timeline and Motion, found {}", v.type_name())),
    };
    let (from, to) = (span[0].unwrap_or(0.0), span[1].unwrap_or(whole_length));
    if to < from {
        return err("ValueError.OutOfRange", format!("trim: from is {from}s but to is {to}s"));
    }
    convert(&r, |t| t.trim(from, to), |m| m.trim(from, to), "trim")
}

fn motion_apply(it: &mut Interp, r: Value, args: Args) -> Result<Value> {
    let Value::Motion(m) = &r else { return err("TypeError.ArgumentType", "apply is a method of Motion") };
    let m = m.clone();
    it.apply_motion(&m, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// receivers に書いた型名が、docs が知っている型かどうか。綴りを間違えると引けなくなる
    #[test]
    fn every_receiver_is_a_known_type() {
        for m in METHODS {
            for r in m.receivers {
                assert!(crate::docs::TYPES.iter().any(|t| t.name == *r), "{}.{} の受け手 \"{r}\" は組み込みの型ではない", r, m.name);
                assert!(find(r, m.name).is_some(), "{r}.{} を引けない", m.name);
            }
        }
    }

    /// 同じ (受け手, 名前) が 2 つあると、後のほうが呼ばれずに死ぬ
    #[test]
    fn no_duplicate_method() {
        let mut seen: Vec<(&str, &str)> = Vec::new();
        for m in METHODS {
            for r in m.receivers {
                assert!(!seen.contains(&(r, m.name)), "{r}.{} が 2 つある", m.name);
                seen.push((r, m.name));
            }
        }
    }
}
