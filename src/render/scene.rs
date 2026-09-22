use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use vello::Scene;
use vello::kurbo::{Affine, BezPath, Circle, Ellipse, Line, Point, Rect, RoundedRect, Shape, Stroke};
use vello::kurbo::{Cap, Join, ParamCurve};
use vello::peniko::{BlendMode, Brush, Color, ColorStops, Compose, Fill, Gradient, ImageBrush, Mix};

use crate::lang::error::{Kind, Result, err};
use crate::render::text::{self, RenderCache};
use crate::lang::value::{ObjRef, Value};

type Attrs = HashMap<String, Value>;

/// 書き出す画の選び方。箱のどこを、どれだけの大きさで出すか
#[derive(Clone, Copy)]
pub struct Shot {
    /// 出力のピクセル
    pub width: f64,
    pub height: f64,
    /// 切り取る大きさ。書かなければ箱全体
    pub crop: Option<Crop>,
    /// 余ったところのどこに寄せるか。0 が左上、1 が右下
    pub align: (f64, f64),
    /// 絵の載らないところ (帯と、箱の外) の色
    pub pad: Color,
}

/// 切り取る大きさ
#[derive(Clone, Copy)]
pub enum Crop {
    /// 高さを箱いっぱいにして、幅は出力の比から決める
    Height,
    /// 幅を箱いっぱいにして、高さは出力の比から決める
    Width,
    /// 箱に対する割合で直に
    Size(f64, f64),
}

impl Shot {
    /// 箱をそのまま出す (切り取らない)
    pub fn whole(width: f64, height: f64) -> Self {
        Shot { width, height, crop: None, align: (0.5, 0.5), pad: Color::WHITE }
    }

    /// 箱のどこを出すか (箱の座標)。切り取らなければ箱全体
    fn source(self, bw: f64, bh: f64) -> Rect {
        let (w, h) = match self.crop {
            None => (bw, bh),
            Some(Crop::Height) => (bh * self.width / self.height, bh),
            Some(Crop::Width) => (bw, bw * self.height / self.width),
            Some(Crop::Size(cw, ch)) => (bw * cw, bh * ch),
        };
        // 余り (箱 − 切り取る大きさ) のどこに寄せるか。余りが負なら箱の外へ出る
        let (x, y) = ((bw - w) * self.align.0, (bh - h) * self.align.1);
        Rect::new(x, y, x + w, y + h)
    }
}

/// View を描画命令に変換する。切り取った範囲を出力サイズに収め (比率維持)、中央に置く
/// t は動画の時刻 (秒)。Shader の塗りに渡す
pub fn build(view: &ObjRef, shot: Shot, t: f64, cache: &mut RenderCache) -> Result<Scene> {
    cache.frame += 1;
    cache.layer_bytes = 0;
    let (bw, bh) = view_box(view)?;
    let source = shot.source(bw, bh);
    let area = picture(view, shot)?;
    let scale = area.width() / source.width();
    let transform = Affine::translate((area.x0, area.y0)) * Affine::scale(scale) * Affine::translate((-source.x0, -source.y0));
    let mut scene = Scene::new();
    // 絵が載るのはこの矩形の中だけ。外は帯なので、塗りもレイヤーもここで切る
    let frame = Frame { picture: area, t };
    // 切り取ったときは、絵の載る矩形の外に中身がはみ出す。帯の中に描かないように切る
    let bands = area.x0 > 0.0 || area.y0 > 0.0 || area.x1 < shot.width || area.y1 < shot.height;
    if bands {
        cache.layer_bytes += (area.width() * area.height() * 4.0) as u64;
        scene.push_layer(Fill::NonZero, BlendMode::new(Mix::Normal, Compose::SrcOver), 1.0, Affine::IDENTITY, &area.to_path(0.01));
    }
    draw_view(&mut scene, view, transform, &frame, cache)?;
    if bands {
        scene.pop_layer();
    }
    drop_unused(cache);
    Ok(scene)
}

/// このフレームで使わなかったものを手放す。数フレームは残しておく
/// (出たり入ったりする図形で、毎フレーム作り直さないように)
fn drop_unused(cache: &mut RenderCache) {
    const KEEP: u64 = 2;
    let now = cache.frame;
    cache.fragments.retain(|_, f| now.saturating_sub(f.used) <= KEEP);
    // 文字は出たり消えたりするので、図形より長く持つ
    cache.drop_layouts(KEEP * 60);
    if let Some(runner) = cache.shaders.as_mut() {
        runner.drop_unused(now, KEEP);
    }
}

/// 切り取った範囲を出力に収めたとき、絵が実際に載る矩形。
/// 縦横比が合わなければ上下か左右が余る (そこは pad の色)。字幕はこの中に出す
pub fn picture(view: &ObjRef, shot: Shot) -> Result<Rect> {
    let (bw, bh) = view_box(view)?;
    let source = shot.source(bw, bh);
    let scale = (shot.width / source.width()).min(shot.height / source.height());
    let (w, h) = (source.width() * scale, source.height() * scale);
    Ok(Rect::new((shot.width - w) / 2.0, (shot.height - h) / 2.0, (shot.width + w) / 2.0, (shot.height + h) / 2.0))
}

/// 1 フレームの間、全図形に共通のもの
struct Frame {
    /// 出力の中で絵が実際に載る矩形
    picture: Rect,
    t: f64,
}

fn view_box(view: &ObjRef) -> Result<(f64, f64)> {
    match view.borrow().attrs.get("box") {
        Some(Value::Vector(w, h)) => Ok((*w, *h)),
        _ => err(Kind::AttributeType, "View.box must be a Vector"),
    }
}

/// View の中身を描く。transform は箱の座標から出力座標へ。
/// View の opacity は中身をまとめて 1 枚の層として掛ける (中の図形が重なっても二重に薄くならない)
fn draw_view(scene: &mut Scene, view: &ObjRef, transform: Affine, frame: &Frame, cache: &mut RenderCache) -> Result<()> {
    let v = view.borrow();
    let opacity = match v.attrs.get("opacity") {
        Some(Value::Number(o, _)) => o.clamp(0.0, 1.0),
        _ => 1.0,
    };
    if opacity <= 0.0 {
        return Ok(());
    }
    // opacity か blend か clip が付いていたら、中身をまとめて 1 枚のレイヤーにする
    let blend = blend_mode(&v.attrs)?.unwrap_or(BlendMode::new(Mix::Normal, Compose::SrcOver));
    let clip = matches!(v.attrs.get("clip"), Some(Value::Bool(true)));
    let grouped = clip || opacity < 1.0 || blend.mix != Mix::Normal || blend.compose != Compose::SrcOver;
    if grouped {
        // clip なら箱の四角でレイヤーを切る。外へはみ出した中身は描かれない。
        // clip でなければ、中身が描く範囲でレイヤーを作る。画面全体で作ると、
        // 半透明の View が増えたときに GPU の混色用の領域が尽きて、フレームが丸ごと消える
        let (bw, bh) = view_box(view)?;
        let area = match clip {
            true => transform.transform_rect_bbox(Rect::new(0.0, 0.0, bw, bh)),
            // 文字の縁など、囲む四角の計算がわずかに小さいことがあるので少し広げる
            false => drawn_bounds(view, transform, cache)?.inflate(2.0, 2.0).intersect(frame.picture),
        };
        // 混色用の領域は、レイヤーの面積ぶん GPU に要る
        cache.layer_bytes += (area.width().max(0.0) * area.height().max(0.0) * 4.0) as u64;
        scene.push_layer(Fill::NonZero, blend, opacity as f32, Affine::IDENTITY, &area.to_path(0.01));
    }
    // カメラは枠を動かさず、中身だけ動かす。clip はカメラの前の枠で切る
    let transform = match camera_of(&v.attrs)? {
        Some((affine, _, _)) => transform * affine,
        None => transform,
    };
    for child in &order(&v.children) {
        if child.borrow().kind == "View" {
            let sub = sub_transform(child, transform)?;
            draw_view(scene, child, sub, frame, cache)?;
            continue;
        }
        let c = child.borrow();
        let key = child.borrow().id;
        // Shader の塗りは毎フレーム計算し直す
        if let Some(shader) = shader_fill(&c) {
            draw_object(scene, &c, transform, frame, key, Some(&shader), cache)?;
            continue;
        }
        // 属性と変換が前回と同じなら、前回の描画命令をそのまま使う
        let fp = fingerprint(&c, transform);
        let now = cache.frame;
        if let Some(prev) = cache.fragments.get_mut(&key) {
            if prev.print == fp {
                prev.used = now;
                scene.append(&prev.scene, None);
                continue;
            }
        }
        let mut fragment = Scene::new();
        draw_object(&mut fragment, &c, transform, frame, key, None, cache)?;
        scene.append(&fragment, None);
        cache.fragments.insert(key, crate::render::text::Fragment { print: fp, scene: fragment, used: now });
    }
    if grouped {
        scene.pop_layer();
    }
    Ok(())
}

/// View の中身が描く範囲 (出力座標)。draw_view と同じ順でたどるので、置き方も回転も線の太さも入る。
/// 中身が無ければ大きさの無い四角
fn drawn_bounds(view: &ObjRef, transform: Affine, cache: &mut RenderCache) -> Result<Rect> {
    let v = view.borrow();
    // clip した View は箱から出ない
    let (bw, bh) = view_box(view)?;
    if matches!(v.attrs.get("clip"), Some(Value::Bool(true))) {
        return Ok(transform.transform_rect_bbox(Rect::new(0.0, 0.0, bw, bh)));
    }
    let transform = match camera_of(&v.attrs)? {
        Some((affine, _, _)) => transform * affine,
        None => transform,
    };
    let mut out: Option<Rect> = None;
    for child in &v.children {
        let bounds = match child.borrow().kind == "View" {
            true => drawn_bounds(child, sub_transform(child, transform)?, cache)?,
            false => object_bounds(&child.borrow(), transform, cache)?,
        };
        out = Some(match out {
            Some(all) => all.union(bounds),
            None => bounds,
        });
    }
    Ok(out.unwrap_or_default())
}

/// 図形 1 つが描く範囲 (出力座標)。draw_object と同じ置き方で囲む四角を出す
fn object_bounds(c: &crate::lang::value::Object, transform: Affine, cache: &mut RenderCache) -> Result<Rect> {
    let scale = transform.as_coeffs()[0].hypot(transform.as_coeffs()[1]);
    if c.kind == "TextArea" {
        let (w, h, cx, cy) = text_box(&c.attrs, scale, cache)?;
        let top_left = transform * Point::new(cx - w / 2.0, cy - h / 2.0);
        let placed = spin_of(c, transform, Point::new(cx, cy))? * Affine::translate(top_left.to_vec2());
        return Ok(placed.transform_rect_bbox(Rect::from_origin_size(Point::ZERO, (w * scale, h * scale))));
    }
    let (path, origin) = outline(c)?;
    let placed = spin_of(c, transform, origin)? * transform;
    let bounds = placed.transform_rect_bbox(path.bounding_box());
    // 線は輪郭の外へ半分はみ出す
    match color(&c.attrs, "stroke", &c.kind)? {
        Some(_) => {
            let half = stroke_style(&c.attrs, None)?.width * scale / 2.0;
            Ok(bounds.inflate(half, half))
        }
        None => Ok(bounds),
    }
}

/// 描く順。zIndex の小さいものから。同じ値なら place した順のまま。
/// zIndex を 1 つも書いていなければ並べ替えない (ほとんどの View がこちら)
fn order(children: &[ObjRef]) -> Vec<ObjRef> {
    let z = |c: &ObjRef| match c.borrow().attrs.get("zIndex") {
        Some(Value::Number(n, _)) => *n,
        _ => 0.0,
    };
    if !children.iter().any(|c| z(c) != 0.0) {
        return children.to_vec();
    }
    let mut out = children.to_vec();
    out.sort_by(|a, b| z(a).partial_cmp(&z(b)).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// 親に置かれた View の変換。position と w / h で決まる矩形に、比率を保って収める
fn sub_transform(child: &ObjRef, parent: Affine) -> Result<Affine> {
    let (bw, bh) = view_box(child)?;
    let c = child.borrow();
    let w = match c.attrs.get("w") { Some(Value::Number(w, _)) => Some(*w), _ => None };
    let h = match c.attrs.get("h") { Some(Value::Number(h, _)) => Some(*h), _ => None };
    let (w, h) = match (w, h) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * bh / bw),
        (None, Some(h)) => (h * bw / bh, h),
        (None, None) => return err(Kind::ArityMismatch, "a placed View needs w or h"),
    };
    let (cx, cy) = anchored_center(&c.attrs, w, h, "View")?;
    let fit = (w / bw).min(h / bh);
    let local = Affine::translate((cx - bw * fit / 2.0, cy - bh * fit / 2.0)) * Affine::scale(fit);
    let placed = parent * local;
    // 中身の座標はそのままに、置いたものを 1 枚として回して寄る。
    // 中心は pivot (子の箱の座標)。書いていなければ箱の真ん中
    let center = match c.attrs.get("pivot") {
        Some(v) => point_of(v, "View.pivot")?,
        None => Point::new(bw / 2.0, bh / 2.0),
    };
    let about = placed * center;
    let spin = match c.attrs.get("rotation") {
        Some(Value::Number(deg, _)) if *deg != 0.0 => Affine::rotate_about(deg.to_radians(), about),
        _ => Affine::IDENTITY,
    };
    let zoom = match c.attrs.get("scale") {
        Some(Value::Number(k, _)) if *k != 1.0 => Affine::scale_about(*k, about),
        _ => Affine::IDENTITY,
    };
    Ok(spin * zoom * placed)
}

/// View.camera の変換。中身の from が to の位置に来るように置いて scale 倍する
fn camera_of(attrs: &Attrs) -> Result<Option<(Affine, Point, f64)>> {
    let Some(value) = attrs.get("camera") else { return Ok(None) };
    let Value::Object(o) = value else {
        return err(Kind::AttributeType, format!("View.camera expects Camera, found {}", value.type_name()));
    };
    let c = o.borrow();
    if c.kind != "Camera" {
        return err(Kind::AttributeType, format!("View.camera expects Camera, found {}", c.kind));
    }
    let from = vector(&c.attrs, "from", "Camera")?;
    // to を書かなければ、中身は動かさずに拡大だけする
    let to = match c.attrs.get("to") {
        Some(v) => point_of(v, "Camera.to")?,
        None => from,
    };
    let scale = match c.attrs.get("scale") {
        Some(Value::Number(k, _)) if *k > 0.0 => *k,
        Some(Value::Number(k, _)) => return err(Kind::OutOfRange, format!("Camera.scale must be above 0, found {k}")),
        Some(v) => return err(Kind::AttributeType, format!("Camera.scale expects Number, found {}", v.type_name())),
        None => 1.0,
    };
    Ok(Some((Affine::translate(to.to_vec2()) * Affine::scale(scale) * Affine::translate(-from.to_vec2()), to, scale)))
}

/// fill が Shader ならその実体
fn shader_fill(c: &crate::lang::value::Object) -> Option<ObjRef> {
    match c.attrs.get("fill") {
        Some(Value::Object(o)) if o.borrow().kind == "Shader" => Some(o.clone()),
        _ => None,
    }
}

/// 図形の輪郭と、回転の中心 (囲む四角形の中心)。描くときと、長さを測るときに使う
pub fn outline(c: &crate::lang::value::Object) -> Result<(BezPath, Point)> {
    let kind = c.kind.as_str();
    Ok(match kind {
        "Circle" => {
            let r = number(&c.attrs, "radius", kind)?;
            let (x, y) = anchored_center(&c.attrs, 2.0 * r, 2.0 * r, kind)?;
            (Circle::new((x, y), r).to_path(0.01), Point::new(x, y))
        }
        "Ellipse" => {
            let (rx, ry) = (number(&c.attrs, "rx", kind)?, number(&c.attrs, "ry", kind)?);
            let (x, y) = anchored_center(&c.attrs, 2.0 * rx, 2.0 * ry, kind)?;
            (Ellipse::new((x, y), (rx, ry), 0.0).to_path(0.01), Point::new(x, y))
        }
        "Rect" => {
            let (w, h) = (number(&c.attrs, "w", kind)?, number(&c.attrs, "h", kind)?);
            let (x, y) = anchored_center(&c.attrs, w, h, kind)?;
            let rect = Rect::from_center_size((x, y), (w, h));
            let path = match c.attrs.get("radius") {
                Some(Value::Number(r, _)) if *r > 0.0 => RoundedRect::from_rect(rect, *r).to_path(0.01),
                _ => rect.to_path(0.01),
            };
            (path, Point::new(x, y))
        }
        "Line" => {
            let (from, to) = (vector(&c.attrs, "from", kind)?, vector(&c.attrs, "to", kind)?);
            (Line::new(from, to).to_path(0.01), Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0))
        }
        "Polygon" => {
            let Some(Value::List(points)) = c.attrs.get("points") else {
                return err(Kind::AttributeType, "Polygon.points must be a List of Vector");
            };
            let mut path = BezPath::new();
            for (i, p) in points.borrow().iter().enumerate() {
                let p = point_of(p, "Polygon.points")?;
                if i == 0 { path.move_to(p) } else { path.line_to(p) }
            }
            path.close_path();
            let center = path.bounding_box().center();
            (path, center)
        }
        "Path" => {
            let Some(Value::List(segments)) = c.attrs.get("segments") else {
                return err(Kind::AttributeType, "Path.segments must be a List");
            };
            let mut path = BezPath::new();
            path.move_to(vector(&c.attrs, "from", kind)?);
            // upto は「先頭から何割の区間を描くか」。最後の 1 区間は途中で切る
            let segs = segments.borrow();
            let upto = match c.attrs.get("upto") {
                Some(Value::Number(u, _)) => u.clamp(0.0, 1.0),
                _ => 1.0,
            };
            let keep = upto * segs.len() as f64;
            let (full, frac) = (keep.floor(), keep.fract());
            for (i, seg) in segs.iter().enumerate() {
                let part = match (i as f64) < full {
                    true => 1.0,
                    false if (i as f64) == full && frac > 0.0 => frac,
                    false => break,
                };
                let Value::Tuple(items) = seg else {
                    return err(Kind::AttributeType, format!("Path.segments expects a Tuple like (:line, point), found {}", seg.type_name()));
                };
                let Some(Value::Symbol(op)) = items.first() else {
                    return err(Kind::AttributeType, "the first item of a Path segment must be :move, :line, :quad or :curve");
                };
                let pts: Vec<Point> = items[1..].iter().map(|p| point_of(p, "Path.segments")).collect::<Result<_>>()?;
                let p0 = path.current_position().unwrap_or(Point::ZERO);
                match (op.as_str(), pts.as_slice()) {
                    ("move", [p]) => path.move_to(*p),
                    ("line", [p]) => path.line_to(p0.lerp(*p, part)),
                    ("quad", [c1, p]) => {
                        let q = vello::kurbo::QuadBez::new(p0, *c1, *p).subsegment(0.0..part);
                        path.quad_to(q.p1, q.p2);
                    }
                    ("curve", [c1, c2, p]) => {
                        let b = vello::kurbo::CubicBez::new(p0, *c1, *c2, *p).subsegment(0.0..part);
                        path.curve_to(b.p1, b.p2, b.p3);
                    }
                    (op, pts) => return err(Kind::ArityMismatch, format!(":{op} got {} points; :move and :line take 1, :quad takes 2, :curve takes 3", pts.len())),
                }
            }
            // 途中までしか描いていないものは閉じない
            if upto >= 1.0 && matches!(c.attrs.get("closed"), Some(Value::Bool(true))) {
                path.close_path();
            }
            let center = path.bounding_box().center();
            (path, center)
        }
        kind => return err(Kind::NotPlaceable, format!("cannot draw {kind}")),

    })
}

/// 図形 1 つを描く。transform は箱の座標から出力座標への変換。shader は fill が Shader のときその実体
fn draw_object(scene: &mut Scene, c: &crate::lang::value::Object, transform: Affine, frame: &Frame, key: u64, shader: Option<&ObjRef>, cache: &mut RenderCache) -> Result<()> {
    let kind = c.kind.as_str();
    let scale = transform.as_coeffs()[0].hypot(transform.as_coeffs()[1]);
    let opacity = match c.attrs.get("opacity") {
        Some(Value::Number(o, _)) => o.clamp(0.0, 1.0) as f32,
        _ => 1.0,
    };
    let spin = |bbox: Point| -> Result<Affine> { spin_of(c, transform, bbox) };
    if kind == "TextArea" {
        return draw_text(scene, &c.attrs, transform, &spin, scale, opacity, cache);
    }
    let (path, origin) = outline(c)?;
    let placed = spin(origin)? * transform;
    // blend が付いていたら、その図形の描画だけを 1 枚のレイヤーにして重ね方を変える
    let blend = blend_mode(&c.attrs)?;
    if let Some(mode) = blend {
        scene.push_layer(Fill::NonZero, mode, 1.0, placed, &path);
    }
    if let Some(shader) = shader {
        draw_shader_fill(scene, &path, placed, opacity, shader, frame, key, cache)?;
    } else if image_fill(scene, c, &path, placed, opacity, cache)? {
    } else if let Some(fill) = brush(&c.attrs, "fill", kind)? {
        scene.fill(Fill::NonZero, placed, &fill.multiply_alpha(opacity), None, &path);
    }
    if let Some(stroke) = color(&c.attrs, "stroke", kind)? {
        let length = c.attrs.contains_key("dash").then(|| vello::kurbo::Shape::perimeter(&path, 1e-4));
        scene.stroke(&stroke_style(&c.attrs, length)?, placed, stroke.multiply_alpha(opacity), None, &path);
    }
    if blend.is_some() {
        scene.pop_layer();
    }
    Ok(())
}

/// 回転の変換。中心は pivot、書いていなければ渡した点 (囲む四角形の中心)。
/// 描く座標で回すので、置く変換の後から掛ける
fn spin_of(c: &crate::lang::value::Object, transform: Affine, bbox: Point) -> Result<Affine> {
    let Some(Value::Number(deg, _)) = c.attrs.get("rotation") else { return Ok(Affine::IDENTITY) };
    if *deg == 0.0 {
        return Ok(Affine::IDENTITY);
    }
    let origin = match c.attrs.get("pivot") {
        Some(v) => point_of(v, &format!("{}.pivot", c.kind))?,
        None => bbox,
    };
    Ok(Affine::rotate_about(deg.to_radians(), transform * origin))
}

/// 描画に関わる属性の指紋。変換と親の不透明度も含める
fn fingerprint(c: &crate::lang::value::Object, transform: Affine) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    c.kind.hash(&mut h);
    transform.as_coeffs().iter().for_each(|x| x.to_bits().hash(&mut h));
    let mut keys: Vec<&String> = c.attrs.keys().collect();
    keys.sort();
    for k in keys {
        k.hash(&mut h);
        hash_value(&c.attrs[k], &mut h);
    }
    h.finish()
}

fn hash_value(v: &Value, h: &mut impl Hasher) {
    match v {
        Value::Number(n, _) => n.to_bits().hash(h),
        Value::Bool(b) => b.hash(h),
        Value::Str(s) | Value::Symbol(s) => s.hash(h),
        Value::Duration(d) => d.to_bits().hash(h),
        Value::Color(c) => c.iter().for_each(|x| x.to_bits().hash(h)),
        Value::Vector(x, y) => (x.to_bits(), y.to_bits()).hash(h),
        Value::Apos(a, x, y) => (a, x.to_bits(), y.to_bits()).hash(h),
        Value::Tuple(items) => items.iter().for_each(|i| hash_value(i, h)),
        Value::List(items) => items.borrow().iter().for_each(|i| hash_value(i, h)),
        other => other.type_name().hash(h),
    }
}

/// TextArea。fontSize と w は箱の単位なので、ピクセルに直してレイアウトする
fn draw_text(scene: &mut Scene, attrs: &Attrs, transform: Affine, spin: &dyn Fn(Point) -> Result<Affine>, scale: f64, opacity: f32, cache: &mut RenderCache) -> Result<()> {
    let kind = "TextArea";
    let Some(Value::Str(content)) = attrs.get("text") else {
        return err(Kind::UndefinedAttribute, "TextArea.text is not set");
    };
    let (w, h, cx, cy) = text_box(attrs, scale, cache)?;
    let (family, max_width, align) = text_style(attrs, scale, cache);
    let layout = cache.layout(content, family.as_deref(), (number(attrs, "fontSize", kind)? * scale) as f32, max_width, align);
    let top_left = transform * Point::new(cx - w / 2.0, cy - h / 2.0);
    let placed = spin(Point::new(cx, cy))? * Affine::translate(top_left.to_vec2());
    let fill = color(attrs, "fill", kind)?.unwrap_or(Color::BLACK);
    let outline = color(attrs, "stroke", kind)?;
    let style = stroke_style(attrs, None)?;
    let stroke = outline.map(|ink| (&style, ink.multiply_alpha(opacity)));
    // blend が付いていたら、文字の箱の中だけ重ね方を変える
    let blend = blend_mode(attrs)?;
    let box_path = Rect::from_origin_size(Point::ZERO, (w * scale, h * scale)).to_path(0.01);
    if let Some(mode) = blend {
        scene.push_layer(Fill::NonZero, mode, 1.0, placed, &box_path);
    }
    text::draw(scene, layout, placed, fill.multiply_alpha(opacity), stroke);
    if blend.is_some() {
        scene.pop_layer();
    }
    Ok(())
}

/// TextArea が占める箱 (幅, 高さ, 中心の x, 中心の y)。幅と高さは箱の座標。
/// w を書いていれば その幅で折り返し、書いていなければ 1 行の幅
fn text_box(attrs: &Attrs, scale: f64, cache: &mut RenderCache) -> Result<(f64, f64, f64, f64)> {
    let kind = "TextArea";
    let Some(Value::Str(content)) = attrs.get("text") else {
        return err(Kind::UndefinedAttribute, "TextArea.text is not set");
    };
    let (family, max_width, align) = text_style(attrs, scale, cache);
    let layout = cache.layout(content, family.as_deref(), (number(attrs, "fontSize", kind)? * scale) as f32, max_width, align);
    let (w, h) = (max_width.unwrap_or(layout.width()) as f64 / scale, f64::from(layout.height()) / scale);
    let (cx, cy) = anchored_center(attrs, w, h, kind)?;
    Ok((w, h, cx, cy))
}

/// 文字の組み方 (書体, 折り返す幅, 寄せ)。書体は候補のうち、この機械にある最初のもの
fn text_style(attrs: &Attrs, scale: f64, cache: &mut RenderCache) -> (Option<String>, Option<f32>, text::Alignment) {
    let names = attrs.get("font").map(crate::lang::eval::font_names).unwrap_or_default();
    let family = cache.first_family(&names);
    let max_width = match attrs.get("w") {
        Some(Value::Number(w, _)) => Some((*w * scale) as f32),
        _ => None,
    };
    let align = match attrs.get("align") {
        Some(Value::Symbol(a)) => text::alignment(Some(a)),
        _ => text::alignment(None),
    };
    (family, max_width, align)
}

fn number(attrs: &Attrs, name: &str, kind: &str) -> Result<f64> {
    match attrs.get(name) {
        Some(Value::Number(v, _)) => Ok(*v),
        Some(v) => err(Kind::AttributeType, format!("{kind}.{name} expects Number, found {}", v.type_name())),
        None => err(Kind::UndefinedAttribute, format!("{kind}.{name} is not set")),
    }
}

/// Vector か、数値 2 つの Tuple を点として読む
fn point_of(v: &Value, whose: &str) -> Result<Point> {
    match v {
        Value::Vector(x, y) => Ok(Point::new(*x, *y)),
        Value::Tuple(items) => match items.as_slice() {
            [Value::Number(x, _), Value::Number(y, _)] => Ok(Point::new(*x, *y)),
            _ => err(Kind::AttributeType, format!("{whose} expects Vector, found {}", v.type_name())),
        },
        other => err(Kind::AttributeType, format!("{whose} expects Vector, found {}", other.type_name())),
    }
}

fn vector(attrs: &Attrs, name: &str, kind: &str) -> Result<Point> {
    match attrs.get(name) {
        Some(Value::Vector(x, y)) => Ok(Point::new(*x, *y)),
        Some(v) => err(Kind::AttributeType, format!("{kind}.{name} expects Vector, found {}", v.type_name())),
        None => err(Kind::UndefinedAttribute, format!("{kind}.{name} is not set")),
    }
}

/// fill に入れられる塗り。Color か Gradient (Shader は別扱い)
fn brush(attrs: &Attrs, name: &str, kind: &str) -> Result<Option<Brush>> {
    match attrs.get(name) {
        Some(Value::Color([r, g, b, a])) => Ok(Some(Brush::Solid(Color::new([*r, *g, *b, *a])))),
        Some(Value::Object(o)) if o.borrow().kind == "Gradient" => Ok(Some(Brush::Gradient(gradient(&o.borrow().attrs)?))),
        // 画像は呼ぶ側が形の大きさに合わせるので、ここでは扱わない
        Some(Value::Image(_)) => Ok(None),
        Some(v) => err(Kind::AttributeType, format!("{kind}.{name} expects a Paint, found {}", v.type_name())),
        None => Ok(None),
    }
}

/// 画像の塗り。画像を形の外接矩形にちょうど収める (縦横比は形に合わせる)
fn image_fill(scene: &mut Scene, c: &crate::lang::value::Object, path: &BezPath, placed: Affine, opacity: f32, cache: &mut RenderCache) -> Result<bool> {
    let Some(Value::Image(file)) = c.attrs.get("fill") else { return Ok(false) };
    let data = cache
        .image(&file.path)
        .map_err(|e| crate::lang::error::MophError::new(Kind::ImageUnreadable, format!("cannot read \"{}\": {e}", file.name)))?;
    let (iw, ih) = (f64::from(data.width), f64::from(data.height));
    let bounds = path.bounding_box();
    if bounds.is_zero_area() || iw <= 0.0 || ih <= 0.0 {
        return Ok(true);
    }
    let mut image = ImageBrush::new(data);
    image.sampler.alpha = opacity;
    // 画像のピクセル座標 → 図形の座標
    let fit = Affine::translate((bounds.x0, bounds.y0)) * Affine::scale_non_uniform(bounds.width() / iw, bounds.height() / ih);
    scene.fill(Fill::NonZero, placed, &Brush::Image(image), Some(fit), path);
    Ok(true)
}

/// Gradient を peniko の Gradient にする。座標は箱の座標
fn gradient(attrs: &Attrs) -> Result<Gradient> {
    let Some(Value::List(items)) = attrs.get("stops") else {
        return err(Kind::UndefinedAttribute, "Gradient.stops must be a List of Color");
    };
    let colors: Vec<Color> = items
        .borrow()
        .iter()
        .map(|c| match c {
            Value::Color([r, g, b, a]) => Ok(Color::new([*r, *g, *b, *a])),
            other => err(Kind::AttributeType, format!("Gradient.stops expects Color, found {}", other.type_name())),
        })
        .collect::<Result<_>>()?;
    if colors.len() < 2 {
        return err(Kind::OutOfRange, "Gradient.stops needs at least 2 colors");
    }
    let mut stops = ColorStops::default();
    vello::peniko::ColorStopsSource::collect_stops(colors.as_slice(), &mut stops);
    let from = vector(attrs, "from", "Gradient")?;
    let kind = match attrs.get("kind") {
        Some(Value::Symbol(k)) => k.as_str(),
        _ => "linear",
    };
    let g = match kind {
        "linear" => Gradient::new_linear(from, vector(attrs, "to", "Gradient")?),
        "radial" => Gradient::new_radial(from, number(attrs, "radius", "Gradient")? as f32),
        "sweep" => Gradient::new_sweep(from, 0.0, std::f32::consts::TAU),
        other => return err(Kind::OutOfRange, format!("unknown Gradient.kind :{other}")),
    };
    Ok(g.with_stops(stops))
}

/// 破線の「線」の部分が、輪郭を最初からひと続きで覆い切るか。
/// 位相の進め方は kurbo に合わせてある
fn dash_covers(pattern: &[f64], offset: f64, length: f64) -> bool {
    let sum: f64 = pattern.iter().sum();
    // 奇数個は 2 周で 1 周期 (SVG と同じ)
    let period = if pattern.len() % 2 == 1 { sum * 2.0 } else { sum };
    if !(period > 0.0) {
        return false;
    }
    let mut i = 0;
    let mut remaining = pattern[0] - offset.rem_euclid(period);
    let mut on = true;
    // 1 周期ぶん進めれば必ず正になる。何周もしないよう回数で止める
    for _ in 0..pattern.len() * 2 {
        if remaining >= 0.0 {
            break;
        }
        i = (i + 1) % pattern.len();
        remaining += pattern[i];
        on = !on;
    }
    on && remaining >= length
}

/// 線の描き方。端の形、角の形、破線。
/// length は輪郭の長さ。分かるときだけ渡す (文字の輪郭では分からない)
fn stroke_style(attrs: &Attrs, length: Option<f64>) -> Result<Stroke> {
    let width = match attrs.get("strokeWidth") {
        Some(Value::Number(w, _)) => *w,
        _ => 0.01,
    };
    let mut stroke = Stroke::new(width);
    if let Some(Value::Symbol(cap)) = attrs.get("strokeCap") {
        stroke = stroke.with_caps(match cap.as_str() {
            "butt" => Cap::Butt,
            "round" => Cap::Round,
            "square" => Cap::Square,
            other => return err(Kind::OutOfRange, format!("unknown strokeCap :{other}")),
        });
    }
    if let Some(Value::Symbol(join)) = attrs.get("strokeJoin") {
        stroke = stroke.with_join(match join.as_str() {
            "miter" => Join::Miter,
            "round" => Join::Round,
            "bevel" => Join::Bevel,
            other => return err(Kind::OutOfRange, format!("unknown strokeJoin :{other}")),
        });
    }
    if let Some(Value::List(items)) = attrs.get("dash") {
        let pattern: Vec<f64> = items
            .borrow()
            .iter()
            .map(|v| match v {
                Value::Number(n, _) => Ok(*n),
                other => err(Kind::AttributeType, format!("dash expects Number, found {}", other.type_name())),
            })
            .collect::<Result<_>>()?;
        if !pattern.is_empty() {
            let offset = match attrs.get("dashOffset") {
                Some(Value::Number(o, _)) => *o,
                _ => 0.0,
            };
            // 線が輪郭を丸ごと覆うなら破線にしない。そのまま渡すと、
            // kurbo 0.13.1 が閉じた輪郭で ClosePath を最後の曲線より前に出してしまい、
            // 円の最後の 4 分の 1 が直線と小さな輪になる
            if !length.is_some_and(|len| dash_covers(&pattern, offset, len)) {
                stroke = stroke.with_dashes(offset, pattern);
            }
        }
    }
    Ok(stroke)
}

/// 重ね方。既定は普通に重ねる
fn blend_mode(attrs: &Attrs) -> Result<Option<BlendMode>> {
    let Some(Value::Symbol(name)) = attrs.get("blend") else {
        return Ok(None);
    };
    let mix = match name.as_str() {
        "normal" => return Ok(None),
        "multiply" => Mix::Multiply,
        "screen" => Mix::Screen,
        "overlay" => Mix::Overlay,
        "darken" => Mix::Darken,
        "lighten" => Mix::Lighten,
        "difference" => Mix::Difference,
        "add" => return Ok(Some(BlendMode::new(Mix::Normal, Compose::Plus))),
        other => return err(Kind::OutOfRange, format!("unknown blend :{other}")),
    };
    Ok(Some(BlendMode::new(mix, Compose::SrcOver)))
}

/// 未設定なら None (stroke は任意)
fn color(attrs: &Attrs, name: &str, kind: &str) -> Result<Option<Color>> {
    match attrs.get(name) {
        Some(Value::Color([r, g, b, a])) => Ok(Some(Color::new([*r, *g, *b, *a]))),
        Some(v) => err(Kind::AttributeType, format!("{kind}.{name} expects Color, found {}", v.type_name())),
        None => Ok(None),
    }
}

/// position (Pos) と大きさから中心座標を求める

fn anchored_center(attrs: &Attrs, w: f64, h: f64, kind: &str) -> Result<(f64, f64)> {
    let Some(Value::Apos(anchor, x, y)) = attrs.get("position") else {
        return err(Kind::AttributeType, format!("{kind}.position must be a Pos"));
    };
    let (dx, dy) = match anchor.as_str() {
        "center" => (0.0, 0.0),
        "topLeft" => (w / 2.0, h / 2.0),
        "topRight" => (-w / 2.0, h / 2.0),
        "bottomLeft" => (w / 2.0, -h / 2.0),
        "bottomRight" => (-w / 2.0, -h / 2.0),
        "top" => (0.0, h / 2.0),
        "bottom" => (0.0, -h / 2.0),
        "left" => (w / 2.0, 0.0),
        "right" => (-w / 2.0, 0.0),
        other => return err(Kind::OutOfRange, format!("unknown anchor :{other}")),
    };
    Ok((x + dx, y + dy))
}

/// preview と sheet で、その時刻に出ている字幕を画面の下に重ねる (プレイヤーの表示に似せる)。動画には入らない
pub fn overlay_subtitles(scene: &mut Scene, cache: &mut RenderCache, cues: &[crate::render::media::Cue], t: f64, area: Rect) {
    let size = (area.height() * 0.045) as f32;
    let box_w = (area.width() * 0.9) as f32;
    let pad = f64::from(size) * 0.35;
    let mut bottom = area.y0 + area.height() * 0.94;
    // 同時に出ている字幕は、先に始まったものを下にして積む
    for cue in cues.iter().filter(|c| c.at <= t && t < c.at + c.length) {
        let layout = cache.layout(&cue.text, None, size, Some(box_w), text::alignment(Some("center")));
        let (w, h) = (f64::from(layout.width()), f64::from(layout.height()));
        let top = bottom - h - pad * 2.0;
        let x0 = area.x0 + (area.width() - w) / 2.0 - pad;
        scene.fill(Fill::NonZero, Affine::IDENTITY, Color::from_rgba8(0, 0, 0, 150), None, &RoundedRect::new(x0, top, x0 + w + pad * 2.0, bottom, pad));
        text::draw(scene, layout, Affine::translate((area.x0 + (area.width() - f64::from(box_w)) / 2.0, top + pad)), Color::WHITE, None);
        bottom = top - pad;
    }
}

/// preview の状態を出す帯。絵の外に取った場所に描くので、中身には被らない
pub fn status_bar(scene: &mut Scene, cache: &mut RenderCache, text: &str, bar: Rect) {
    let size = (bar.height() * 0.45) as f32;
    let layout = cache.layout(text, None, size, None, text::alignment(Some("left")));
    scene.fill(Fill::NonZero, Affine::IDENTITY, Color::from_rgba8(24, 24, 28, 255), None, &bar);
    let y = bar.y0 + (bar.height() - f64::from(layout.height())) / 2.0;
    text::draw(scene, layout, Affine::translate((bar.x0 + bar.height() * 0.6, y)), Color::from_rgba8(225, 225, 230, 255), None);
}

/// ステータスバーの高さ。出力の高さから決める
pub fn status_height(height: f64) -> f64 {
    (height * 0.05).clamp(26.0, 64.0)
}

/// preview の操作の一覧を、絵の真ん中に重ねる
pub fn overlay_help(scene: &mut Scene, cache: &mut RenderCache, text: &str, area: Rect) {
    let size = (area.height() * 0.035).max(11.0) as f32;
    let pad = f64::from(size) * 1.4;
    let layout = cache.layout(text, None, size, None, text::alignment(Some("left")));
    let (w, h) = (f64::from(layout.width()), f64::from(layout.height()));
    let x0 = area.x0 + (area.width() - w) / 2.0 - pad;
    let y0 = area.y0 + (area.height() - h) / 2.0 - pad;
    let panel = RoundedRect::new(x0, y0, x0 + w + pad * 2.0, y0 + h + pad * 2.0, pad / 2.0);
    scene.fill(Fill::NonZero, Affine::IDENTITY, Color::from_rgba8(0, 0, 0, 190), None, &panel);
    text::draw(scene, layout, Affine::translate((x0 + pad, y0 + pad)), Color::from_rgba8(240, 240, 240, 255), None);
}

/// Shader の塗り。図形の範囲 (画面内) のピクセルを compute shader で計算し、その画像で図形を塗る
#[allow(clippy::too_many_arguments)]
fn draw_shader_fill(scene: &mut Scene, path: &BezPath, transform: Affine, opacity: f32, shader: &ObjRef, frame: &Frame, key: u64, cache: &mut RenderCache) -> Result<()> {
    let sh = shader.borrow();
    let Some(Value::Func(closure)) = sh.attrs.get("color") else {
        return err(Kind::AttributeType, "Shader.color must be a func (x, y, t)");
    };
    // Array はそのまま渡す (同じ実体なら GPU に送り直さない)。List は毎フレーム写す
    let plain: Vec<f32>;
    let args = match sh.attrs.get("args") {
        Some(Value::Array(a)) => crate::render::shader::Args::Shared(a),
        Some(Value::List(items)) => {
            plain = items
                .borrow()
                .iter()
                .map(|v| match v {
                    Value::Number(n, _) => Ok(*n as f32),
                    other => err(Kind::AttributeType, format!("Shader.args must hold Numbers, found {}", other.type_name())),
                })
                .collect::<Result<_>>()?;
            crate::render::shader::Args::Plain(&plain)
        }
        Some(other) => return err(Kind::AttributeType, format!("Shader.args expects Array or List, found {}", other.type_name())),
        None => crate::render::shader::Args::Plain(&[]),
    };
    // ズーム動画。中心と、時刻から倍率への表をもらう
    let zoom = match sh.attrs.get("zoom") {
        Some(Value::Object(o)) if o.borrow().kind == "ZoomPath" => {
            let m = o.borrow();
            let Some(Value::Vector(cx, cy)) = m.attrs.get("center") else {
                return err(Kind::AttributeType, "ZoomPath.center must be a Vector");
            };
            let Some(Value::Duration(duration)) = m.attrs.get("duration") else {
                return err(Kind::AttributeType, "ZoomPath.duration must be a Duration");
            };
            let Some(Value::List(items)) = m.attrs.get("scale") else {
                return err(Kind::AttributeType, "ZoomPath.scale must be a List of Numbers");
            };
            let scale: Vec<f64> = items
                .borrow()
                .iter()
                .map(|v| match v {
                    Value::Number(n, _) => Ok(*n),
                    other => err(Kind::AttributeType, format!("ZoomPath.scale must hold Numbers, found {}", other.type_name())),
                })
                .collect::<Result<_>>()?;
            if scale.len() < 2 || *duration <= 0.0 {
                return err(Kind::AttributeType, "ZoomPath.scale needs 2 or more Numbers and a duration above 0");
            }
            Some((*cx, *cy, *duration, scale))
        }
        Some(other) => return err(Kind::AttributeType, format!("Shader.zoom expects ZoomPath, found {}", other.type_name())),
        None => None,
    };
    // カメラを入れた Shader は、箱の座標そのものではなく camera.from からの差を受け取る。
    // 引き算をここ (f64) で済ませるので、倍率をいくつ上げても f32 の刻みが効く
    let camera = match sh.attrs.get("camera") {
        Some(Value::Object(o)) if o.borrow().kind == "Camera" => camera_of(&HashMap::from([("camera".to_string(), Value::Object(o.clone()))]))?,
        Some(other) => return err(Kind::AttributeType, format!("Shader.camera expects Camera, found {}", other.type_name())),
        None => None,
    };
    let samples = match sh.attrs.get("samples") {
        Some(Value::Number(n, _)) if *n >= 1.0 => *n as u32,
        Some(other) => return err(Kind::AttributeType, format!("Shader.samples must be a Number of 1 or more, found {other}")),
        None => 1,
    };
    let Some(runner) = cache.shaders.as_mut() else {
        return err(Kind::ShaderUnavailable, "a Shader fill needs the GPU (render, preview, sheet)");
    };
    let bounds = transform.transform_rect_bbox(path.bounding_box()).intersect(frame.picture);
    if bounds.is_zero_area() {
        return Ok(());
    }
    let (x0, y0) = (bounds.x0.floor(), bounds.y0.floor());
    let width = (bounds.x1.ceil() - x0).max(1.0) as u32;
    let height = (bounds.y1.ceil() - y0).max(1.0) as u32;
    // 箱 → ピクセルは拡大と平行移動だけなので、逆は 1 次式
    let [s, _, _, _, tx, ty] = transform.as_coeffs();
    let request = crate::render::shader::Request {
        frame: cache.frame,
        shape: key,
        closure,
        args,
        t: frame.t,
        width,
        height,
        // カメラを入れた Shader は、図形を動かさずにシェーダの座標だけ寄せる。
        // 箱の座標 q が映す中身の点は from + (q - to) / scale なので、その差を渡す
        origin: match camera {
            Some((_, to, k)) => (((x0 - tx) / s - to.x) / k, ((y0 - ty) / s - to.y) / k),
            None => ((x0 - tx) / s, (y0 - ty) / s),
        },
        step: match camera {
            Some((_, _, k)) => (1.0 / s / k, 1.0 / s / k),
            None => (1.0 / s, 1.0 / s),
        },
        samples,
        camera: camera.map(|(_, _, scale)| scale),
        zoom: zoom.as_ref().map(|(cx, cy, duration, scale)| crate::render::shader::ZoomPath {
            center: (*cx, *cy),
            scale,
            duration: *duration,
        }),
    };
    let image = runner.run(request)?;
    let mut brush = ImageBrush::new(image);
    brush.sampler.alpha = opacity;
    scene.fill(Fill::NonZero, transform, &Brush::Image(brush), Some(transform.inverse() * Affine::translate((x0, y0))), path);
    Ok(())
}

/// View の中に Shader の塗りがあるか。あると描画命令を組むのに GPU が要る
pub fn uses_shader(view: &ObjRef) -> bool {
    let v = view.borrow();
    shader_fill(&v).is_some() || v.children.iter().any(uses_shader)
}


#[cfg(test)]
mod tests {
    use super::*;

    /// 線の部分が輪郭を覆い切るときは破線にしない。そのまま kurbo に渡すと
    /// 閉じた輪郭で ClosePath が最後の曲線より前に出て、円の最後の 4 分の 1 が
    /// 直線と小さな輪になる
    #[test]
    fn a_dash_longer_than_the_outline_is_not_a_dash() {
        let len = 12.566;
        assert!(dash_covers(&[len * 1.01, len], 0.0, len));
        assert!(dash_covers(&[len, len], 0.0, len));
        // 輪郭より短ければ破線のまま (終わりに隙間ができる)
        assert!(!dash_covers(&[len * 0.999, len], 0.0, len));
        assert!(!dash_covers(&[0.4, 0.3], 0.0, len));
    }

    /// dashOffset で位相をずらした分は、覆える長さから引かれる
    #[test]
    fn the_offset_eats_into_what_the_dash_covers() {
        let len = 10.0;
        assert!(dash_covers(&[20.0, 10.0], 5.0, len));
        assert!(!dash_covers(&[20.0, 10.0], 11.0, len));
        // 空白から始まる位相は、そもそも線ではない
        assert!(!dash_covers(&[5.0, 5.0], 6.0, 1.0));
        // 奇数個は 2 周で 1 周期 (SVG と同じ)
        assert!(dash_covers(&[30.0], 0.0, len));
    }
}
