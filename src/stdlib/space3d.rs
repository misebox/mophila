//! `import space3d`。3D の空間で位置を決めて、2D の箱の座標に落とす。
//!
//! この層は 2D に触らない。外に出るのは箱の座標の `Vector` と、奥行きの `Number` だけ。
//! 点の並びを扱う関数は、並びを受け取って並びを返す (ループは Rust の中で回す)。

use std::collections::HashMap;

use crate::docs::Entry;
use crate::lang::error::{Kind, Result, err};
use crate::lang::value::{Module, Value};

pub const DOCS: &[Entry] = &[
    Entry { name: "Vector3", signature: "space3d.Vector3(x: Number, y: Number, z: Number)", returns: "Vector3", doc: "3D の点。+ - * / と .x .y .z が使える" },
];

/// `import space3d` で束縛されるもの
pub fn module() -> Module {
    let mut items = HashMap::new();
    for f in ["Vector3"] {
        items.insert(f.into(), Value::Builtin(name_of(f)));
    }
    Module { name: "space3d".into(), items }
}

/// Value::Builtin に持たせる名前。呼ぶときにどのモジュールか分かるように前置きする
fn name_of(f: &str) -> &'static str {
    match f {
        "Vector3" => "space3d.Vector3",
        other => panic!("space3d に {other} は無い"),
    }
}

/// `space3d.…` の呼び出し。`name` は前置きを外したもの
pub fn call(name: &str, values: &[Value]) -> Result<Value> {
    match name {
        "Vector3" => match nums(name, values)?.as_slice() {
            [x, y, z] => Ok(Value::Vector3(*x, *y, *z)),
            other => err(Kind::ArityMismatch, format!("Vector3 takes 3 arguments (x, y, z), {} given", other.len())),
        },
        other => err(Kind::UndefinedVariable, format!("space3d has no \"{other}\"")),
    }
}

fn nums(name: &str, values: &[Value]) -> Result<Vec<f64>> {
    values
        .iter()
        .map(|v| match v {
            Value::Number(n, _) => Ok(*n),
            v => err(Kind::ArgumentType, format!("{name} expects Number, found {}", v.type_name())),
        })
        .collect()
}
