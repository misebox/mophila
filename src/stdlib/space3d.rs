//! `import space3d`。3D の空間で位置を決めて、2D の箱の座標に落とす。
//!
//! この層は 2D に触らない。外に出るのは箱の座標の `Vector` と、奥行きの `Number` だけ。
//! 点の並びを扱う関数は、並びを受け取って並びを返す (ループは Rust の中で回す)。

use std::collections::HashMap;

use crate::docs::Entry;
use crate::lang::error::{Kind, Result, err};
use crate::lang::value::{Module, Object, Value};

pub const DOCS: &[Entry] = &[
    Entry { name: "Vector3", signature: "space3d.Vector3(x: Number, y: Number, z: Number)", returns: "Vector3", doc: "3D の点。+ - * / と .x .y .z が使える" },
    Entry { name: "Transform3", signature: "space3d.Transform3()", returns: "Transform3", doc: "何もしない変換。rotate_x などを繋いで組み立てる" },
    Entry { name: "PerspectiveCamera", signature: "space3d.PerspectiveCamera(from, to, up, fov, box)", returns: "PerspectiveCamera", doc: "透視投影。遠いものほど小さくなる" },
    Entry { name: "OrthographicCamera", signature: "space3d.OrthographicCamera(from, to, up, height, box)", returns: "OrthographicCamera", doc: "平行投影。遠くても大きさが変わらない" },
    Entry { name: "IsometricCamera", signature: "space3d.IsometricCamera(unit, box)", returns: "IsometricCamera", doc: "等角投影。x は右下、z は左下、y は上へ。視点は持たない" },
];

/// `import space3d` で束縛されるもの
pub fn module() -> Module {
    let mut items = HashMap::new();
    for f in ["Vector3"] {
        items.insert(f.into(), Value::Builtin(name_of(f)));
    }
    for t in ["Transform3", "PerspectiveCamera", "OrthographicCamera", "IsometricCamera"] {
        items.insert(t.into(), Value::BuiltinType(t.into()));
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

/// 3D の点。中の計算だけで使う
type P3 = (f64, f64, f64);

fn sub(a: P3, b: P3) -> P3 {
    (a.0 - b.0, a.1 - b.1, a.2 - b.2)
}

fn dot(a: P3, b: P3) -> f64 {
    a.0 * b.0 + a.1 * b.1 + a.2 * b.2
}

fn cross(a: P3, b: P3) -> P3 {
    (a.1 * b.2 - a.2 * b.1, a.2 * b.0 - a.0 * b.2, a.0 * b.1 - a.1 * b.0)
}

fn normalized(name: &str, what: &str, v: P3) -> Result<P3> {
    let len = dot(v, v).sqrt();
    if len < 1e-12 {
        return err(Kind::OutOfRange, format!("{name}: {what} has no direction"));
    }
    Ok((v.0 / len, v.1 / len, v.2 / len))
}

/// 奥行きの扱い。投影の仕方はこれだけで決まる
enum Lens {
    /// 箱の単位での焦点距離。奥行きで割るので、遠いものほど小さくなる
    Perspective(f64),
    /// 1 単位が箱でいくつになるか。奥行きによらない
    Parallel(f64),
}

/// 投影に要るものだけにしたカメラ。属性から毎回組み立てる
struct Camera {
    eye: P3,
    right: P3,
    up: P3,
    /// 視線の向き。奥行きはこれとの内積
    fwd: P3,
    lens: Lens,
    /// 箱の中心。ここが視線の先になる
    center: (f64, f64),
    /// 視点を持つか。持たない (等角) なら、奥行きの符号に意味が無い
    has_eye: bool,
}

/// 等角投影の視線。(1, 1, 1) から原点を見る向き。x は右下、z は左下、y は上に出る
const ISO_EYE: P3 = (1.0, 1.0, 1.0);

impl Camera {
    /// 属性からカメラを組む。既定値は構築のときに入っているので、ここでは欠けを見るだけ
    fn of(o: &Object) -> Result<Camera> {
        let kind = o.kind.as_str();
        let num = |name: &str| -> Result<f64> {
            match o.attrs.get(name) {
                Some(Value::Number(n, _)) => Ok(*n),
                _ => err(Kind::UndefinedAttribute, format!("{kind}.{name} is not set")),
            }
        };
        let point = |name: &str| -> Result<P3> {
            match o.attrs.get(name) {
                Some(Value::Vector3(x, y, z)) => Ok((*x, *y, *z)),
                _ => err(Kind::UndefinedAttribute, format!("{kind}.{name} is not set")),
            }
        };
        let Some(Value::Vector(bw, bh)) = o.attrs.get("box") else {
            return err(Kind::UndefinedAttribute, format!("{kind}.box is not set"));
        };
        let center = (bw / 2.0, bh / 2.0);
        let iso = kind == "IsometricCamera";
        let (eye, fwd, up) = if iso {
            (ISO_EYE, normalized(kind, "the line of sight", (-ISO_EYE.0, -ISO_EYE.1, -ISO_EYE.2))?, (0.0, 1.0, 0.0))
        } else {
            let (eye, to) = (point("from")?, point("to")?);
            (eye, normalized(kind, "from and to are the same point", sub(to, eye))?, point("up")?)
        };
        let right = normalized(kind, "up is along the line of sight", cross(fwd, up))?;
        let lens = match kind {
            "PerspectiveCamera" => {
                let fov = num("fov")?;
                if fov <= 0.0 || fov >= 180.0 {
                    return err(Kind::OutOfRange, format!("PerspectiveCamera.fov must be between 0 and 180 degrees, found {fov}"));
                }
                Lens::Perspective(center.1 / (fov.to_radians() / 2.0).tan())
            }
            "OrthographicCamera" => {
                let height = num("height")?;
                if height <= 0.0 {
                    return err(Kind::OutOfRange, format!("OrthographicCamera.height must be positive, found {height}"));
                }
                Lens::Parallel(bh / height)
            }
            _ => Lens::Parallel(num("unit")?),
        };
        Ok(Camera { eye, right, up: cross(right, fwd), fwd, lens, center, has_eye: !iso })
    }

    /// 視点から見た座標。(右, 上, 奥)
    fn seen(&self, p: P3) -> P3 {
        let d = sub(p, self.eye);
        (dot(d, self.right), dot(d, self.up), dot(d, self.fwd))
    }

    /// その奥行きで、1 単位が箱でいくつになるか
    fn scale_at(&self, name: &str, z: f64) -> Result<f64> {
        match self.lens {
            Lens::Parallel(k) => Ok(k),
            Lens::Perspective(focal) if z > 1e-9 => Ok(focal / z),
            // 視点より手前のものは、どこに映るとも言えない
            Lens::Perspective(_) => {
                err(Kind::OutOfRange, format!("PerspectiveCamera.{name}: the point is at or behind the eye; check in_front(p) first"))
            }
        }
    }

    /// 箱の座標。y は下向きなので、上を向く成分は引く
    fn project(&self, name: &str, p: P3) -> Result<Value> {
        let (x, y, z) = self.seen(p);
        let k = self.scale_at(name, z)?;
        Ok(Value::Vector(self.center.0 + x * k, self.center.1 - y * k))
    }
}

/// カメラ 3 種のメソッド。名前は METHODS の表と揃える
pub fn camera_method(o: &Object, name: &str, args: &[(Option<String>, Value)]) -> Result<Value> {
    let cam = Camera::of(o)?;
    let one = || -> Result<P3> {
        match args {
            [(None, Value::Vector3(x, y, z))] => Ok((*x, *y, *z)),
            _ => err(Kind::ArgumentType, format!("{}.{name} takes one Vector3", o.kind)),
        }
    };
    let many = || -> Result<Vec<P3>> {
        match args {
            [(None, Value::List(xs))] => xs
                .borrow()
                .iter()
                .map(|v| match v {
                    Value::Vector3(x, y, z) => Ok((*x, *y, *z)),
                    v => err(Kind::ArgumentType, format!("{}.{name} takes a List of Vector3, found {} in it", o.kind, v.type_name())),
                })
                .collect(),
            _ => err(Kind::ArgumentType, format!("{}.{name} takes one List<Vector3>", o.kind)),
        }
    };
    let list = |xs: Vec<Value>| Value::List(std::rc::Rc::new(std::cell::RefCell::new(xs)));
    match name {
        "project" => cam.project(name, one()?),
        "project_all" => Ok(list(many()?.into_iter().map(|p| cam.project(name, p)).collect::<Result<Vec<_>>>()?)),
        "depth" => Ok(Value::num(cam.seen(one()?).2)),
        "depth_all" => Ok(list(many()?.into_iter().map(|p| Value::num(cam.seen(p).2)).collect())),
        "scale_at" => Ok(Value::num(cam.scale_at(name, cam.seen(one()?).2)?)),
        // 視点を持たないカメラには、手前も奥も無い
        "in_front" => Ok(Value::Bool(!cam.has_eye || cam.seen(one()?).2 > 1e-9)),
        _ => err(Kind::UndefinedAttribute, format!("{} has no method \"{name}\"", o.kind)),
    }
}

/// 3x4 の行列。左の 3x3 が回転と拡大、右の 1 列が平行移動。行優先
type M34 = [f64; 12];

const IDENTITY: M34 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// 何もしない Transform3 の中身。構築のときの既定値
pub fn identity() -> Value {
    numbers(&IDENTITY)
}

/// 点に b が先に効き、その後 a が効く 1 つの行列
fn compose(a: &M34, b: &M34) -> M34 {
    let mut out = [0.0; 12];
    for r in 0..3 {
        for c in 0..3 {
            out[r * 4 + c] = (0..3).map(|k| a[r * 4 + k] * b[k * 4 + c]).sum();
        }
        out[r * 4 + 3] = (0..3).map(|k| a[r * 4 + k] * b[k * 4 + 3]).sum::<f64>() + a[r * 4 + 3];
    }
    out
}

fn applied(m: &M34, p: P3) -> P3 {
    let row = |r: usize| m[r * 4] * p.0 + m[r * 4 + 1] * p.1 + m[r * 4 + 2] * p.2 + m[r * 4 + 3];
    (row(0), row(1), row(2))
}

/// 軸まわりの回転。axis は 0 が x、1 が y、2 が z。右ねじの向き (軸の先から見て反時計回り)
fn rotation(axis: usize, degrees: f64) -> M34 {
    let (s, c) = degrees.to_radians().sin_cos();
    let mut m = IDENTITY;
    // 回す平面の 2 軸。その 2 つだけが混ざる
    let (u, v) = [(1, 2), (2, 0), (0, 1)][axis];
    m[u * 4 + u] = c;
    m[u * 4 + v] = -s;
    m[v * 4 + u] = s;
    m[v * 4 + v] = c;
    m
}

fn numbers(m: &M34) -> Value {
    Value::List(std::rc::Rc::new(std::cell::RefCell::new(m.iter().map(|n| Value::num(*n)).collect())))
}

/// Transform3 の中身。attrs の m は自分で作ったものなので、壊れていたら作り直せと言う
fn matrix(o: &Object) -> Result<M34> {
    let Some(Value::List(xs)) = o.attrs.get("m") else {
        return err(Kind::UndefinedAttribute, "Transform3.m is not set; make it with space3d.Transform3()");
    };
    let xs = xs.borrow();
    let mut m = [0.0; 12];
    if xs.len() != m.len() {
        return err(Kind::OutOfRange, format!("Transform3.m must have {} numbers, found {}", m.len(), xs.len()));
    }
    for (slot, v) in m.iter_mut().zip(xs.iter()) {
        let Value::Number(n, _) = v else {
            return err(Kind::AttributeType, format!("Transform3.m must be a List of Number, found {}", v.type_name()));
        };
        *slot = *n;
    }
    Ok(m)
}

/// 中身の違う新しい Transform3。元は変えない
fn transform(m: &M34) -> Value {
    let mut attrs = HashMap::new();
    attrs.insert("m".to_string(), numbers(m));
    Value::Object(std::rc::Rc::new(std::cell::RefCell::new(Object {
        id: crate::lang::value::next_object_id(),
        kind: "Transform3".to_string(),
        decl: None,
        attrs,
        children: Vec::new(),
        placed: false,
        tracks: Vec::new(),
    })))
}

/// Transform3 のメソッド。どれも新しい Transform3 か、効かせた点を返す
pub fn transform_method(o: &Object, name: &str, args: &[(Option<String>, Value)]) -> Result<Value> {
    let m = matrix(o)?;
    let degrees = || -> Result<f64> {
        match args {
            [(None, Value::Number(n, _))] => Ok(*n),
            _ => err(Kind::ArgumentType, format!("Transform3.{name} takes one Number (degrees)")),
        }
    };
    let point = || -> Result<P3> {
        match args {
            [(None, Value::Vector3(x, y, z))] => Ok((*x, *y, *z)),
            _ => err(Kind::ArgumentType, format!("Transform3.{name} takes one Vector3")),
        }
    };
    // 後から足したものが、点には後に効く
    let add = |next: M34| Ok(transform(&compose(&next, &m)));
    match name {
        "rotate_x" => add(rotation(0, degrees()?)),
        "rotate_y" => add(rotation(1, degrees()?)),
        "rotate_z" => add(rotation(2, degrees()?)),
        "translate" => {
            let (x, y, z) = point()?;
            let mut next = IDENTITY;
            (next[3], next[7], next[11]) = (x, y, z);
            add(next)
        }
        "scale" => {
            let k = degrees()?;
            let mut next = IDENTITY;
            (next[0], next[5], next[10]) = (k, k, k);
            add(next)
        }
        "then" => match args {
            [(None, Value::Object(other))] if other.borrow().kind == "Transform3" => add(matrix(&other.borrow())?),
            _ => err(Kind::ArgumentType, "Transform3.then takes one Transform3"),
        },
        "apply" => {
            let (x, y, z) = applied(&m, point()?);
            Ok(Value::Vector3(x, y, z))
        }
        "apply_all" => match args {
            [(None, Value::List(xs))] => {
                let out = xs
                    .borrow()
                    .iter()
                    .map(|v| match v {
                        Value::Vector3(x, y, z) => {
                            let (x, y, z) = applied(&m, (*x, *y, *z));
                            Ok(Value::Vector3(x, y, z))
                        }
                        v => err(Kind::ArgumentType, format!("Transform3.apply_all takes a List of Vector3, found {} in it", v.type_name())),
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(Value::List(std::rc::Rc::new(std::cell::RefCell::new(out))))
            }
            _ => err(Kind::ArgumentType, "Transform3.apply_all takes one List<Vector3>"),
        },
        _ => err(Kind::UndefinedAttribute, format!("Transform3 has no method \"{name}\"")),
    }
}
