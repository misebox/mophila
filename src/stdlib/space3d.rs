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
    Entry { name: "box", signature: "space3d.box(at: Vector3, size: Vector3)", returns: "Mesh", doc: "直方体。at が中心" },
    Entry {
        name: "plane",
        signature: "space3d.plane(at: Vector3, w: Number, d: Number, cols: Number, rows: Number)",
        returns: "Mesh",
        doc: "xz 平面に寝かせた板。at が中心。cols は x 方向、rows は z 方向の分割数",
    },
    Entry {
        name: "sphere",
        signature: "space3d.sphere(at: Vector3, r: Number, rings: Number, segments: Number)",
        returns: "Mesh",
        doc: "球。rings は極から極への分割数、segments は横の分割数",
    },
    Entry { name: "path", signature: "space3d.path(points: List<Vector3>)", returns: "Mesh", doc: "3D の折れ線。線だけで面は持たない" },
    Entry {
        name: "grid",
        signature: "space3d.grid(at: Vector3, w: Number, d: Number, step: Number)",
        returns: "Mesh",
        doc: "床の格子線。step に近い間隔で割る。面は持たない",
    },
    Entry {
        name: "shade",
        signature: "space3d.shade(color: Color, normal: Vector3, light: Vector3, ambient: Number = 0.25)",
        returns: "Color",
        doc: "面の向きと光の向きから塗りの色を決める。光に正面から向いた面が元の色、背いた面が ambient 倍",
    },
    Entry { name: "Mesh", signature: "space3d.Mesh(points, edges, faces)", returns: "Mesh", doc: "頂点と、それを繋ぐ線や面。作り手 (box など) が返すもの" },
    Entry { name: "Face", signature: "space3d.Face(points, normal, depth)", returns: "Face", doc: "Mesh.faces が返す、投影済みの 1 面" },
    Entry { name: "Transform3", signature: "space3d.Transform3()", returns: "Transform3", doc: "何もしない変換。rotate_x などを繋いで組み立てる" },
    Entry { name: "PerspectiveCamera", signature: "space3d.PerspectiveCamera(from, to, up, fov, box)", returns: "PerspectiveCamera", doc: "透視投影。遠いものほど小さくなる" },
    Entry { name: "OrthographicCamera", signature: "space3d.OrthographicCamera(from, to, up, height, box)", returns: "OrthographicCamera", doc: "平行投影。遠くても大きさが変わらない" },
    Entry { name: "IsometricCamera", signature: "space3d.IsometricCamera(unit, box)", returns: "IsometricCamera", doc: "等角投影。x は右下、z は左下、y は上へ。視点は持たない" },
];

/// `import space3d` で束縛されるもの
pub fn module() -> Module {
    let mut items = HashMap::new();
    for f in ["box", "plane", "sphere", "path", "grid", "shade"] {
        items.insert(f.into(), Value::Builtin(name_of(f)));
    }
    for t in ["Vector3", "Mesh", "Face", "Transform3", "PerspectiveCamera", "OrthographicCamera", "IsometricCamera"] {
        items.insert(t.into(), Value::BuiltinType(t.into()));
    }
    Module { name: "space3d".into(), items }
}

/// Value::Builtin に持たせる名前。呼ぶときにどのモジュールか分かるように前置きする
fn name_of(f: &str) -> &'static str {
    match f {
        "box" => "space3d.box",
        "plane" => "space3d.plane",
        "sphere" => "space3d.sphere",
        "path" => "space3d.path",
        "grid" => "space3d.grid",
        "shade" => "space3d.shade",
        other => panic!("space3d に {other} は無い"),
    }
}

/// `space3d.…` の呼び出し。`name` は前置きを外したもの
pub fn call(name: &str, values: &[Value]) -> Result<Value> {
    // エラーには呼んだ通りの名前を出す
    let name = &format!("space3d.{name}")[..];
    match name.trim_start_matches("space3d.") {
        "box" => match values {
            [Value::Vector3(x, y, z), Value::Vector3(w, h, d)] => Ok(make_box((*x, *y, *z), (*w, *h, *d))),
            _ => err(Kind::ArgumentType, "space3d.box takes (at: Vector3, size: Vector3)"),
        },
        "plane" => match values {
            [Value::Vector3(x, y, z), Value::Number(w, _), Value::Number(d, _), Value::Number(cols, _), Value::Number(rows, _)] => {
                Ok(make_plane((*x, *y, *z), *w, *d, count(name, "cols", *cols)?, count(name, "rows", *rows)?, true))
            }
            _ => err(Kind::ArgumentType, "space3d.plane takes (at: Vector3, w: Number, d: Number, cols: Number, rows: Number)"),
        },
        "grid" => match values {
            [Value::Vector3(x, y, z), Value::Number(w, _), Value::Number(d, _), Value::Number(step, _)] => {
                if *step <= 0.0 {
                    return err(Kind::OutOfRange, format!("{name}: step must be positive, found {step}"));
                }
                let lines = |len: f64| ((len / step).round() as usize).max(1);
                Ok(make_plane((*x, *y, *z), *w, *d, lines(*w), lines(*d), false))
            }
            _ => err(Kind::ArgumentType, "space3d.grid takes (at: Vector3, w: Number, d: Number, step: Number)"),
        },
        "sphere" => match values {
            [Value::Vector3(x, y, z), Value::Number(r, _), Value::Number(rings, _), Value::Number(segments, _)] => {
                Ok(make_sphere((*x, *y, *z), *r, count(name, "rings", *rings)?.max(2), count(name, "segments", *segments)?.max(3)))
            }
            _ => err(Kind::ArgumentType, "space3d.sphere takes (at: Vector3, r: Number, rings: Number, segments: Number)"),
        },
        "path" => match values {
            [Value::List(xs)] => {
                let points = xs
                    .borrow()
                    .iter()
                    .map(|v| match v {
                        Value::Vector3(x, y, z) => Ok((*x, *y, *z)),
                        v => err(Kind::ArgumentType, format!("space3d.path takes a List of Vector3, found {} in it", v.type_name())),
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(make_path(points))
            }
            _ => err(Kind::ArgumentType, "space3d.path takes one List<Vector3>"),
        },
        "shade" => match values {
            [Value::Color(c), Value::Vector3(nx, ny, nz), Value::Vector3(lx, ly, lz), rest @ ..] => {
                let ambient = match rest {
                    [] => 0.25,
                    [Value::Number(a, _)] if (0.0..=1.0).contains(a) => *a,
                    _ => return err(Kind::ArgumentType, "space3d.shade takes ambient as a Number between 0 and 1"),
                };
                let n = normalized(name, "normal", (*nx, *ny, *nz))?;
                let l = normalized(name, "light", (*lx, *ly, *lz))?;
                let k = (ambient + (1.0 - ambient) * dot(n, l).max(0.0)) as f32;
                Ok(Value::Color([c[0] * k, c[1] * k, c[2] * k, c[3]]))
            }
            _ => err(Kind::ArgumentType, "space3d.shade takes (color: Color, normal: Vector3, light: Vector3, ambient: Number = 0.25)"),
        },
        other => err(Kind::UndefinedVariable, format!("space3d has no \"{other}\"")),
    }
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

    /// 箱の座標。y は下向きなので、上を向く成分は引く。映せない点は None
    fn spot(&self, p: P3) -> Option<(f64, f64)> {
        let (x, y, z) = self.seen(p);
        let k = match self.lens {
            Lens::Parallel(k) => k,
            Lens::Perspective(focal) if z > 1e-9 => focal / z,
            Lens::Perspective(_) => return None,
        };
        Some((self.center.0 + x * k, self.center.1 - y * k))
    }

    fn project(&self, name: &str, p: P3) -> Result<Value> {
        match self.spot(p) {
            Some((x, y)) => Ok(Value::Vector(x, y)),
            None => err(Kind::OutOfRange, format!("PerspectiveCamera.{name}: the point is at or behind the eye; check in_front(p) first")),
        }
    }

    /// その面がこちらを向いているか。向こう向きの面は描かない
    fn faces_us(&self, normal: P3, center: P3) -> bool {
        match self.lens {
            // 平行投影はどこでも同じ向きから見る
            Lens::Parallel(_) => dot(normal, self.fwd) < 0.0,
            Lens::Perspective(_) => dot(normal, sub(center, self.eye)) < 0.0,
        }
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

/// 組み立て中の Mesh。頂点は 1 か所に並べ、線と面はその番号で持つ
struct Build {
    points: Vec<P3>,
    edges: Vec<(usize, usize)>,
    faces: Vec<Vec<usize>>,
}

impl Build {
    fn new() -> Build {
        Build { points: Vec::new(), edges: Vec::new(), faces: Vec::new() }
    }

    /// 長さ 0 の線は描いても見えないので入れない (球の極で出る)
    fn edge(&mut self, a: usize, b: usize) {
        if self.points[a] != self.points[b] {
            self.edges.push((a, b));
        }
    }

    /// 格子の (行, 列) を頂点の番号にする。列は cols + 1 本
    fn grid_of(&mut self, rows: usize, cols: usize, faces: bool) {
        let at = |i: usize, j: usize| i * (cols + 1) + j;
        for i in 0..=rows {
            for j in 0..=cols {
                if j < cols {
                    self.edge(at(i, j), at(i, j + 1));
                }
                if i < rows {
                    self.edge(at(i, j), at(i + 1, j));
                }
                if faces && i < rows && j < cols {
                    self.faces.push(vec![at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)]);
                }
            }
        }
    }

    fn into_mesh(self) -> Value {
        let list = |xs: Vec<Value>| Value::List(std::rc::Rc::new(std::cell::RefCell::new(xs)));
        let mut attrs = HashMap::new();
        attrs.insert("points".to_string(), list(self.points.iter().map(|p| Value::Vector3(p.0, p.1, p.2)).collect()));
        attrs.insert("edges".to_string(), list(self.edges.iter().map(|(a, b)| Value::Tuple(vec![Value::num(*a as f64), Value::num(*b as f64)])).collect()));
        attrs.insert(
            "faces".to_string(),
            list(self.faces.iter().map(|f| list(f.iter().map(|i| Value::num(*i as f64)).collect())).collect()),
        );
        object("Mesh", attrs)
    }
}

/// Rust で組み立てた builtin の値
fn object(kind: &str, attrs: HashMap<String, Value>) -> Value {
    Value::Object(std::rc::Rc::new(std::cell::RefCell::new(Object {
        id: crate::lang::value::next_object_id(),
        kind: kind.to_string(),
        decl: None,
        attrs,
        children: Vec::new(),
        placed: false,
        tracks: Vec::new(),
    })))
}

/// 分割の数。1 以上の整数でなければ断る
fn count(name: &str, what: &str, n: f64) -> Result<usize> {
    if n < 1.0 || n.fract() != 0.0 || n > 4096.0 {
        return err(Kind::OutOfRange, format!("{name}: {what} must be a whole number between 1 and 4096, found {n}"));
    }
    Ok(n as usize)
}

/// 直方体。at が中心
fn make_box(at: P3, size: P3) -> Value {
    let (w, h, d) = (size.0 / 2.0, size.1 / 2.0, size.2 / 2.0);
    let corner = |sx: f64, sy: f64, sz: f64| (at.0 + sx * w, at.1 + sy * h, at.2 + sz * d);
    let mut b = Build::new();
    // 0..3 が奥 (-z) の四隅、4..7 が手前 (+z) の四隅。どちらも左下から反時計回り
    b.points = vec![
        corner(-1.0, -1.0, -1.0),
        corner(1.0, -1.0, -1.0),
        corner(1.0, 1.0, -1.0),
        corner(-1.0, 1.0, -1.0),
        corner(-1.0, -1.0, 1.0),
        corner(1.0, -1.0, 1.0),
        corner(1.0, 1.0, 1.0),
        corner(-1.0, 1.0, 1.0),
    ];
    for i in 0..4 {
        b.edge(i, (i + 1) % 4);
        b.edge(4 + i, 4 + (i + 1) % 4);
        b.edge(i, 4 + i);
    }
    // 面は外から見て反時計回りに並べる。この向きが表になる
    b.faces = vec![vec![4, 5, 6, 7], vec![1, 0, 3, 2], vec![5, 1, 2, 6], vec![0, 4, 7, 3], vec![7, 6, 2, 3], vec![0, 1, 5, 4]];
    b.into_mesh()
}

/// xz 平面に寝かせた板。at が中心。cols は x 方向、rows は z 方向の分割数
fn make_plane(at: P3, w: f64, d: f64, cols: usize, rows: usize, faces: bool) -> Value {
    let mut b = Build::new();
    for i in 0..=rows {
        for j in 0..=cols {
            let x = at.0 - w / 2.0 + w * j as f64 / cols as f64;
            let z = at.2 - d / 2.0 + d * i as f64 / rows as f64;
            b.points.push((x, at.1, z));
        }
    }
    b.grid_of(rows, cols, faces);
    b.into_mesh()
}

/// 球。rings は縦の分割数 (極から極)、segments は横の分割数
fn make_sphere(at: P3, r: f64, rings: usize, segments: usize) -> Value {
    let mut b = Build::new();
    for i in 0..=rings {
        let theta = std::f64::consts::PI * i as f64 / rings as f64;
        let (st, ct) = theta.sin_cos();
        for j in 0..=segments {
            let phi = std::f64::consts::TAU * j as f64 / segments as f64;
            let (sp, cp) = phi.sin_cos();
            b.points.push((at.0 + r * st * cp, at.1 + r * ct, at.2 + r * st * sp));
        }
    }
    // 格子の並びは板と同じ。ただし外を向くように、面の回る向きを逆にする
    b.grid_of(rings, segments, true);
    for f in &mut b.faces {
        f.reverse();
    }
    b.into_mesh()
}

/// 3D の折れ線。線だけで面は無い
fn make_path(points: Vec<P3>) -> Value {
    let mut b = Build::new();
    b.points = points;
    for i in 1..b.points.len() {
        b.edge(i - 1, i);
    }
    b.into_mesh()
}

/// Mesh の中身。attrs から読み直す
struct Shape {
    points: Vec<P3>,
    edges: Vec<(usize, usize)>,
    faces: Vec<Vec<usize>>,
}

fn indices(o: &Object, name: &str, v: &Value, len: usize) -> Result<Vec<usize>> {
    let of = |v: &Value| match v {
        Value::Number(n, _) if *n >= 0.0 && n.fract() == 0.0 && (*n as usize) < len => Ok(*n as usize),
        Value::Number(n, _) => err(Kind::OutOfRange, format!("{}.{name}: {n} is not a point number (points has {len})", o.kind)),
        v => err(Kind::AttributeType, format!("{}.{name} must hold numbers, found {}", o.kind, v.type_name())),
    };
    match v {
        Value::Tuple(xs) => xs.iter().map(of).collect(),
        Value::List(xs) => xs.borrow().iter().map(of).collect(),
        v => err(Kind::AttributeType, format!("{}.{name} must hold Tuple or List, found {}", o.kind, v.type_name())),
    }
}

impl Shape {
    fn of(o: &Object) -> Result<Shape> {
        let Some(Value::List(ps)) = o.attrs.get("points") else {
            return err(Kind::UndefinedAttribute, format!("{}.points is not set", o.kind));
        };
        let points = ps
            .borrow()
            .iter()
            .map(|v| match v {
                Value::Vector3(x, y, z) => Ok((*x, *y, *z)),
                v => err(Kind::AttributeType, format!("{}.points must be a List of Vector3, found {}", o.kind, v.type_name())),
            })
            .collect::<Result<Vec<_>>>()?;
        let n = points.len();
        let mut edges = Vec::new();
        if let Some(Value::List(es)) = o.attrs.get("edges") {
            for v in es.borrow().iter() {
                match indices(o, "edges", v, n)?.as_slice() {
                    [a, b] => edges.push((*a, *b)),
                    other => return err(Kind::OutOfRange, format!("{}.edges takes pairs of point numbers, found {} of them", o.kind, other.len())),
                }
            }
        }
        let mut faces = Vec::new();
        if let Some(Value::List(fs)) = o.attrs.get("faces") {
            for v in fs.borrow().iter() {
                let f = indices(o, "faces", v, n)?;
                if f.len() < 3 {
                    return err(Kind::OutOfRange, format!("{}.faces takes 3 or more point numbers per face, found {}", o.kind, f.len()));
                }
                faces.push(f);
            }
        }
        Ok(Shape { points, edges, faces })
    }
}

/// Mesh のメソッド
pub fn mesh_method(o: &Object, name: &str, args: &[(Option<String>, Value)]) -> Result<Value> {
    let list = |xs: Vec<Value>| Value::List(std::rc::Rc::new(std::cell::RefCell::new(xs)));
    let mesh = Shape::of(o)?;
    if name == "transformed" {
        let [(None, Value::Object(t))] = args else {
            return err(Kind::ArgumentType, "Mesh.transformed takes one Transform3");
        };
        if t.borrow().kind != "Transform3" {
            return err(Kind::ArgumentType, format!("Mesh.transformed takes one Transform3, found {}", t.borrow().kind));
        }
        let m = matrix(&t.borrow())?;
        let mut b = Build::new();
        b.points = mesh.points.iter().map(|p| applied(&m, *p)).collect();
        (b.edges, b.faces) = (mesh.edges, mesh.faces);
        return Ok(b.into_mesh());
    }
    // 残りはカメラを取る
    let [(None, Value::Object(c))] = args else {
        return err(Kind::ArgumentType, format!("Mesh.{name} takes one camera"));
    };
    let cam = Camera::of(&c.borrow())?;
    let spots: Vec<Option<(f64, f64)>> = mesh.points.iter().map(|p| cam.spot(*p)).collect();
    match name {
        // 映せない点を持つ線は落とす。視点より手前の点は箱のどこにも来ないので描けない
        "edges" => Ok(list(
            mesh.edges
                .iter()
                .filter_map(|(a, b)| match (spots[*a], spots[*b]) {
                    (Some(a), Some(b)) => Some(Value::Tuple(vec![Value::Vector(a.0, a.1), Value::Vector(b.0, b.1)])),
                    _ => None,
                })
                .collect(),
        )),
        "faces" => {
            let mut out: Vec<(f64, Value)> = Vec::new();
            for f in &mesh.faces {
                let at = |i: usize| mesh.points[f[i]];
                let normal = cross(sub(at(1), at(0)), sub(at(2), at(0)));
                let count = f.len() as f64;
                let sum = f.iter().fold((0.0, 0.0, 0.0), |a, i| (a.0 + mesh.points[*i].0, a.1 + mesh.points[*i].1, a.2 + mesh.points[*i].2));
                let center = (sum.0 / count, sum.1 / count, sum.2 / count);
                if dot(normal, normal) < 1e-24 || !cam.faces_us(normal, center) {
                    continue;
                }
                let Some(points) = f.iter().map(|i| spots[*i].map(|(x, y)| Value::Vector(x, y))).collect::<Option<Vec<_>>>() else {
                    continue;
                };
                let depth = cam.seen(center).2;
                let normal = normalized("Mesh.faces", "a face", normal)?;
                let mut attrs = HashMap::new();
                attrs.insert("points".to_string(), list(points));
                attrs.insert("normal".to_string(), Value::Vector3(normal.0, normal.1, normal.2));
                attrs.insert("depth".to_string(), Value::num(depth));
                out.push((depth, object("Face", attrs)));
            }
            // 奥から描けば手前が上に乗る
            out.sort_by(|a, b| b.0.total_cmp(&a.0));
            Ok(list(out.into_iter().map(|(_, f)| f).collect()))
        }
        _ => err(Kind::UndefinedAttribute, format!("Mesh has no method \"{name}\"")),
    }
}
