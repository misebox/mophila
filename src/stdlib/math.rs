//! `import math`。数学の定数と関数。Shader の中では render::shader が同じ名前を WGSL にする

use std::collections::HashMap;

use crate::docs::Entry;
use crate::lang::error::{Result, err};
use crate::lang::value::{Module, Value};

pub const DOCS: &[Entry] = &[
    Entry { name: "PI", signature: "math.PI", returns: "Number", doc: "円周率" },
    Entry { name: "TAU", signature: "math.TAU", returns: "Number", doc: "2π" },
    Entry { name: "E", signature: "math.E", returns: "Number", doc: "自然対数の底" },
    Entry { name: "sin", signature: "math.sin(x: Number)", returns: "Number", doc: "正弦 (ラジアン)" },
    Entry { name: "cos", signature: "math.cos(x: Number)", returns: "Number", doc: "余弦 (ラジアン)" },
    Entry { name: "floor", signature: "math.floor(x: Number)", returns: "Number", doc: "切り捨て" },
    Entry { name: "ceil", signature: "math.ceil(x: Number)", returns: "Number", doc: "切り上げ" },
    Entry { name: "abs", signature: "math.abs(x: Number)", returns: "Number", doc: "絶対値" },
    Entry { name: "sqrt", signature: "math.sqrt(x: Number)", returns: "Number", doc: "平方根" },
    Entry { name: "ln", signature: "math.ln(x: Number)", returns: "Number", doc: "自然対数" },
    Entry { name: "exp", signature: "math.exp(x: Number)", returns: "Number", doc: "e の x 乗" },
    Entry { name: "atan2", signature: "math.atan2(y: Number, x: Number)", returns: "Number", doc: "(x, y) の角度 (ラジアン)" },
    Entry { name: "max", signature: "math.max(a: Number, b: Number, ...)", returns: "Number", doc: "最大" },
    Entry { name: "min", signature: "math.min(a: Number, b: Number, ...)", returns: "Number", doc: "最小" },
];

pub fn module() -> Module {
    let mut items = HashMap::new();
    items.insert("PI".into(), Value::Number(std::f64::consts::PI));
    items.insert("TAU".into(), Value::Number(std::f64::consts::TAU));
    items.insert("E".into(), Value::Number(std::f64::consts::E));
    for f in ["sin", "cos", "floor", "ceil", "abs", "sqrt", "ln", "exp", "atan2", "max", "min"] {
        items.insert(f.into(), Value::Builtin(f));
    }
    Module { name: "math".into(), items }
}

pub fn call(name: &str, values: &[Value]) -> Result<Value> {
    let nums = values
        .iter()
        .map(|v| match v {
            Value::Number(n) => Ok(*n),
            v => err("TypeError.ArgumentType", format!("{name} expects Number, found {}", v.type_name())),
        })
        .collect::<Result<Vec<f64>>>()?;
    let unary = |f: fn(f64) -> f64| match nums.as_slice() {
        [x] => Ok(Value::Number(f(*x))),
        _ => err("TypeError.ArityMismatch", format!("{name} takes 1 argument, {} given", nums.len())),
    };
    match name {
        "sin" => unary(f64::sin),
        "cos" => unary(f64::cos),
        "floor" => unary(f64::floor),
        "ceil" => unary(f64::ceil),
        "abs" => unary(f64::abs),
        "sqrt" => unary(f64::sqrt),
        "ln" => unary(f64::ln),
        "exp" => unary(f64::exp),
        "atan2" => match nums.as_slice() {
            [y, x] => Ok(Value::Number(y.atan2(*x))),
            _ => err("TypeError.ArityMismatch", format!("atan2 takes 2 arguments (y, x), {} given", nums.len())),
        },
        "max" | "min" if nums.is_empty() => err("TypeError.ArityMismatch", format!("{name} needs at least 1 argument")),
        "max" => Ok(Value::Number(nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max))),
        "min" => Ok(Value::Number(nums.iter().cloned().fold(f64::INFINITY, f64::min))),
        _ => err("NameError.UndefinedAttribute", format!("no builtin \"{name}\"")),
    }
}
