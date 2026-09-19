//! `import math`。数学の定数と関数。Shader の中では render::shader が同じ名前を WGSL にする

use std::collections::HashMap;

use crate::docs::Entry;
use crate::lang::error::{Kind, Result, err};
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
    Entry { name: "max", signature: "math.max(a: Number, b: Number, ...)", returns: "Number", doc: "最大。全部 Duration なら Duration のまま返す" },
    Entry { name: "min", signature: "math.min(a: Number, b: Number, ...)", returns: "Number", doc: "最小。全部 Duration なら Duration のまま返す" },
    Entry { name: "tan", signature: "math.tan(x: Number)", returns: "Number", doc: "正接 (ラジアン)" },
    Entry { name: "asin", signature: "math.asin(x: Number)", returns: "Number", doc: "逆正弦。戻りはラジアン" },
    Entry { name: "acos", signature: "math.acos(x: Number)", returns: "Number", doc: "逆余弦。戻りはラジアン" },
    Entry { name: "atan", signature: "math.atan(x: Number)", returns: "Number", doc: "逆正接。戻りはラジアン" },
    Entry { name: "round", signature: "math.round(x: Number, digits: Number = 0)", returns: "Number", doc: "四捨五入。digits で小数の桁を決める" },
    Entry { name: "sign", signature: "math.sign(x: Number)", returns: "Number", doc: "符号。-1 / 0 / 1" },
    Entry { name: "pow", signature: "math.pow(a: Number, b: Number)", returns: "Number", doc: "a の b 乗。a ^ b と同じ" },
    Entry { name: "log10", signature: "math.log10(x: Number)", returns: "Number", doc: "常用対数。組み込みの log (表示) と紛れるので、この名前にしてある" },
    Entry { name: "log2", signature: "math.log2(x: Number)", returns: "Number", doc: "2 を底とする対数" },
    Entry { name: "hypot", signature: "math.hypot(x: Number, y: Number)", returns: "Number", doc: "原点からの距離" },
    Entry { name: "clamp", signature: "math.clamp(x: Number, lo: Number, hi: Number)", returns: "Number", doc: "lo と hi の間に収める。全部 Duration なら Duration のまま返す" },
    Entry { name: "lerp", signature: "math.lerp(a: Number, b: Number, k: Number)", returns: "Number", doc: "a と b の間。k は 0..1" },
    Entry { name: "unlerp", signature: "math.unlerp(a: Number, b: Number, x: Number)", returns: "Number", doc: "x が a..b のどこか。lerp の逆" },
    Entry { name: "map_range", signature: "math.map_range(x: Number, a0: Number, a1: Number, b0: Number, b1: Number)", returns: "Number", doc: "a0..a1 の x を b0..b1 に写す。グラフの軸に使う" },
    Entry { name: "nice_step", signature: "math.nice_step(span: Number, count: Number)", returns: "Number", doc: "span を count 個に分けるときの、切りの良い目盛りの間隔 (1, 2, 5 の倍数)" },
];

pub fn module() -> Module {
    let mut items = HashMap::new();
    items.insert("PI".into(), Value::num(std::f64::consts::PI));
    items.insert("TAU".into(), Value::num(std::f64::consts::TAU));
    items.insert("E".into(), Value::num(std::f64::consts::E));
    for f in [
        "sin", "cos", "tan", "asin", "acos", "atan", "floor", "ceil", "round", "abs", "sign", "sqrt", "pow", "ln", "log", "exp", "log10", "log2", "atan2", "hypot", "max", "min", "clamp", "lerp",
        "unlerp", "map_range", "nice_step",
    ] {
        items.insert(f.into(), Value::Builtin(f));
    }
    Module { name: "math".into(), items }
}

pub fn call(name: &str, values: &[Value]) -> Result<Value> {
    // 長さを比べるだけなら、秒に直して掛け直さなくて済む。単位が混ざるものは通さない
    if matches!(name, "max" | "min" | "clamp") && !values.is_empty() && values.iter().all(|v| matches!(v, Value::Duration(_))) {
        return duration_pick(name, values);
    }
    let nums = values
        .iter()
        .map(|v| match v {
            Value::Number(n, _) => Ok(*n),
            v => err(Kind::ArgumentType, format!("{name} expects Number, found {}", v.type_name())),
        })
        .collect::<Result<Vec<f64>>>()?;
    let unary = |f: fn(f64) -> f64| match nums.as_slice() {
        [x] => Ok(Value::num(f(*x))),
        _ => err(Kind::ArityMismatch, format!("{name} takes 1 argument, {} given", nums.len())),
    };
    let binary = |f: fn(f64, f64) -> f64| match nums.as_slice() {
        [a, b] => Ok(Value::num(f(*a, *b))),
        _ => err(Kind::ArityMismatch, format!("{name} takes 2 arguments, {} given", nums.len())),
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
        "tan" => unary(f64::tan),
        "asin" => unary(f64::asin),
        "acos" => unary(f64::acos),
        "atan" => unary(f64::atan),
        "sign" => unary(f64::signum),
        "pow" => binary(f64::powf),
        "hypot" => binary(f64::hypot),
        // 小数の桁を決めて四捨五入する。桁を書かなければ整数へ
        "round" => match nums.as_slice() {
            [x] => Ok(Value::num(x.round())),
            [x, digits] => {
                let scale = 10_f64.powi(*digits as i32);
                Ok(Value::num((x * scale).round() / scale))
            }
            _ => err(Kind::ArityMismatch, format!("round takes 1 or 2 arguments, {} given", nums.len())),
        },
        "log10" => unary(f64::log10),
        "log2" => unary(f64::log2),
        "clamp" => match nums.as_slice() {
            [x, lo, hi] if lo <= hi => Ok(Value::num(x.clamp(*lo, *hi))),
            [_, lo, hi] => err(Kind::OutOfRange, format!("clamp needs lo <= hi, found {lo} and {hi}")),
            _ => err(Kind::ArityMismatch, format!("clamp takes 3 arguments (x, lo, hi), {} given", nums.len())),
        },
        "lerp" => match nums.as_slice() {
            [a, b, k] => Ok(Value::num(a + (b - a) * k)),
            _ => err(Kind::ArityMismatch, format!("lerp takes 3 arguments (a, b, k), {} given", nums.len())),
        },
        "unlerp" => match nums.as_slice() {
            [a, b, x] if a != b => Ok(Value::num((x - a) / (b - a))),
            [a, ..] => Ok(Value::num(if nums[2] < *a { 0.0 } else { 1.0 })),
            _ => err(Kind::ArityMismatch, format!("unlerp takes 3 arguments (a, b, x), {} given", nums.len())),
        },
        "map_range" => match nums.as_slice() {
            [x, a0, a1, b0, b1] if a0 != a1 => Ok(Value::num(b0 + (b1 - b0) * (x - a0) / (a1 - a0))),
            [.., b0, _] => Ok(Value::num(*b0)),
            _ => err(Kind::ArityMismatch, format!("map_range takes 5 arguments (x, a0, a1, b0, b1), {} given", nums.len())),
        },
        // 目盛りの間隔。1, 2, 5 の 10 のべき乗倍から、要求に近いものを選ぶ
        "nice_step" => match nums.as_slice() {
            [span, count] if *count > 0.0 && *span > 0.0 => {
                let rough = span / count;
                let power = 10_f64.powf(rough.log10().floor());
                let step = [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * power).find(|s| *s >= rough).unwrap_or(power * 10.0);
                Ok(Value::num(step))
            }
            [_, _] => err(Kind::OutOfRange, "nice_step needs a positive span and count"),
            _ => err(Kind::ArityMismatch, format!("nice_step takes 2 arguments (span, count), {} given", nums.len())),
        },
        "atan2" => match nums.as_slice() {
            [y, x] => Ok(Value::num(y.atan2(*x))),
            _ => err(Kind::ArityMismatch, format!("atan2 takes 2 arguments (y, x), {} given", nums.len())),
        },
        "max" | "min" if nums.is_empty() => err(Kind::ArityMismatch, format!("{name} needs at least 1 argument")),
        "max" => Ok(Value::num(nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max))),
        "min" => Ok(Value::num(nums.iter().cloned().fold(f64::INFINITY, f64::min))),
        _ => err(Kind::UndefinedAttribute, format!("no builtin \"{name}\"")),
    }
}

/// Duration だけを受け取る max / min / clamp
fn duration_pick(name: &str, values: &[Value]) -> Result<Value> {
    let secs: Vec<f64> = values.iter().map(|v| match v { Value::Duration(d) => *d, _ => 0.0 }).collect();
    let out = match name {
        "max" => secs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        "min" => secs.iter().cloned().fold(f64::INFINITY, f64::min),
        _ => match secs.as_slice() {
            [x, lo, hi] if lo <= hi => x.clamp(*lo, *hi),
            [_, lo, hi] => return err(Kind::OutOfRange, format!("clamp needs lo <= hi, found {lo}s and {hi}s")),
            _ => return err(Kind::ArityMismatch, format!("clamp takes 3 arguments (x, lo, hi), {} given", secs.len())),
        },
    };
    Ok(Value::Duration(out))
}
