use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use vello::Scene;
use vello::kurbo::{Affine, BezPath, Circle, Ellipse, Line, Point, Rect, RoundedRect, Shape, Stroke};
use vello::kurbo::{Cap, Join};
use vello::peniko::{BlendMode, Brush, Color, ColorStops, Compose, Fill, Gradient, ImageBrush, Mix};

use crate::lang::error::{Result, err};
use crate::render::text::{self, RenderCache};
use crate::lang::value::{ObjRef, Value};

type Attrs = HashMap<String, Value>;

/// View を描画命令に変換する。View の box を出力サイズに収め (比率維持)、中央に置く
/// t は動画の時刻 (秒)。Shader の塗りに渡す
pub fn build(view: &ObjRef, width: f64, height: f64, t: f64, cache: &mut RenderCache) -> Result<Scene> {
    let (bw, bh) = view_box(view)?;
    let scale = (width / bw).min(height / bh);
    let transform = Affine::translate(((width - bw * scale) / 2.0, (height - bh * scale) / 2.0)) * Affine::scale(scale);
    let mut scene = Scene::new();
    let frame = Frame { viewport: Rect::new(0.0, 0.0, width, height), t };
    draw_view(&mut scene, view, transform, &frame, cache)?;
    Ok(scene)
}

/// 1 フレームの間、全図形に共通のもの
struct Frame {
    viewport: Rect,
    t: f64,
}

fn view_box(view: &ObjRef) -> Result<(f64, f64)> {
    match view.borrow().attrs.get("box") {
        Some(Value::Vector(w, h)) => Ok((*w, *h)),
        _ => err("TypeError.AttributeType", "View.box must be a Vector"),
    }
}

/// View の中身を描く。transform は箱の座標から出力座標へ。
/// View の opacity は中身をまとめて 1 枚の層として掛ける (中の図形が重なっても二重に薄くならない)
fn draw_view(scene: &mut Scene, view: &ObjRef, transform: Affine, frame: &Frame, cache: &mut RenderCache) -> Result<()> {
    let v = view.borrow();
    let opacity = match v.attrs.get("opacity") {
        Some(Value::Number(o)) => o.clamp(0.0, 1.0),
        _ => 1.0,
    };
    if opacity <= 0.0 {
        return Ok(());
    }
    // opacity か blend が付いていたら、中身をまとめて 1 枚のレイヤーにする
    let blend = blend_mode(&v.attrs)?.unwrap_or(BlendMode::new(Mix::Normal, Compose::SrcOver));
    let grouped = opacity < 1.0 || blend.mix != Mix::Normal || blend.compose != Compose::SrcOver;
    if grouped {
        scene.push_layer(Fill::NonZero, blend, opacity as f32, Affine::IDENTITY, &frame.viewport);
    }
    for child in &v.children {
        if child.borrow().kind == "View" {
            let sub = sub_transform(child, transform)?;
            draw_view(scene, child, sub, frame, cache)?;
            continue;
        }
        let c = child.borrow();
        let key = Rc::as_ptr(child) as usize;
        // Shader の塗りは毎フレーム計算し直す
        if let Some(shader) = shader_fill(&c) {
            draw_object(scene, &c, transform, frame, key, Some(&shader), cache)?;
            continue;
        }
        // 属性と変換が前回と同じなら、前回の描画命令をそのまま使う
        let fp = fingerprint(&c, transform);
        if let Some((prev, fragment)) = cache.fragments.get(&key) {
            if *prev == fp {
                scene.append(fragment, None);
                continue;
            }
        }
        let mut fragment = Scene::new();
        draw_object(&mut fragment, &c, transform, frame, key, None, cache)?;
        scene.append(&fragment, None);
        cache.fragments.insert(key, (fp, fragment));
    }
    if grouped {
        scene.pop_layer();
    }
    Ok(())
}

/// 親に置かれた View の変換。position と w / h で決まる矩形に、比率を保って収める
fn sub_transform(child: &ObjRef, parent: Affine) -> Result<Affine> {
    let (bw, bh) = view_box(child)?;
    let c = child.borrow();
    let w = match c.attrs.get("w") { Some(Value::Number(w)) => Some(*w), _ => None };
    let h = match c.attrs.get("h") { Some(Value::Number(h)) => Some(*h), _ => None };
    let (w, h) = match (w, h) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * bh / bw),
        (None, Some(h)) => (h * bw / bh, h),
        (None, None) => return err("TypeError.ArityMismatch", "a placed View needs w or h"),
    };
    let (cx, cy) = anchored_center(&c.attrs, w, h, "View")?;
    let scale = (w / bw).min(h / bh);
    let local = Affine::translate((cx - bw * scale / 2.0, cy - bh * scale / 2.0)) * Affine::scale(scale);
    Ok(parent * local)
}

/// fill が Shader ならその実体
fn shader_fill(c: &crate::lang::value::Object) -> Option<ObjRef> {
    match c.attrs.get("fill") {
        Some(Value::Object(o)) if o.borrow().kind == "Shader" => Some(o.clone()),
        _ => None,
    }
}

/// 図形 1 つを描く。transform は箱の座標から出力座標への変換。shader は fill が Shader のときその実体
fn draw_object(scene: &mut Scene, c: &crate::lang::value::Object, transform: Affine, frame: &Frame, key: usize, shader: Option<&ObjRef>, cache: &mut RenderCache) -> Result<()> {
    let kind = c.kind.as_str();
    let scale = transform.as_coeffs()[0].hypot(transform.as_coeffs()[1]);
    let opacity = match c.attrs.get("opacity") {
        Some(Value::Number(o)) => o.clamp(0.0, 1.0) as f32,
        _ => 1.0,
    };
    // 回転は箱の座標で先に掛ける。中心は図形ごとの基準点 (position、Line は from、Polygon は頂点の重心)
    let spin = |origin: Point| match c.attrs.get("rotation") {
        Some(Value::Number(deg)) if *deg != 0.0 => transform * Affine::rotate_about(deg.to_radians(), origin),
        _ => transform,
    };
    if kind == "TextArea" {
        return draw_text(scene, &c.attrs, spin(anchor_point(&c.attrs, kind)?), scale, opacity, cache);
    }
    let (path, origin) = match kind {
        "Circle" => {
            let r = number(&c.attrs, "radius", kind)?;
            let (x, y) = anchored_center(&c.attrs, 2.0 * r, 2.0 * r, kind)?;
            (Circle::new((x, y), r).to_path(0.01), anchor_point(&c.attrs, kind)?)
        }
        "Ellipse" => {
            let (rx, ry) = (number(&c.attrs, "rx", kind)?, number(&c.attrs, "ry", kind)?);
            let (x, y) = anchored_center(&c.attrs, 2.0 * rx, 2.0 * ry, kind)?;
            (Ellipse::new((x, y), (rx, ry), 0.0).to_path(0.01), anchor_point(&c.attrs, kind)?)
        }
        "Rect" => {
            let (w, h) = (number(&c.attrs, "w", kind)?, number(&c.attrs, "h", kind)?);
            let (x, y) = anchored_center(&c.attrs, w, h, kind)?;
            let rect = Rect::from_center_size((x, y), (w, h));
            let path = match c.attrs.get("radius") {
                Some(Value::Number(r)) if *r > 0.0 => RoundedRect::from_rect(rect, *r).to_path(0.01),
                _ => rect.to_path(0.01),
            };
            (path, anchor_point(&c.attrs, kind)?)
        }
        "Line" => {
            let from = vector(&c.attrs, "from", kind)?;
            (Line::new(from, vector(&c.attrs, "to", kind)?).to_path(0.01), from)
        }
        "Polygon" => {
            let Some(Value::List(points)) = c.attrs.get("points") else {
                return err("TypeError.AttributeType", "Polygon.points must be a List of Vector");
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
                return err("TypeError.AttributeType", "Path.segments must be a List");
            };
            let mut path = BezPath::new();
            path.move_to(vector(&c.attrs, "from", kind)?);
            for seg in segments.borrow().iter() {
                let Value::Tuple(items) = seg else {
                    return err("TypeError.AttributeType", format!("Path.segments expects a Tuple like (:line, point), found {}", seg.type_name()));
                };
                let Some(Value::Symbol(op)) = items.first() else {
                    return err("TypeError.AttributeType", "the first item of a Path segment must be :move, :line, :quad or :curve");
                };
                let pts: Vec<Point> = items[1..].iter().map(|p| point_of(p, "Path.segments")).collect::<Result<_>>()?;
                match (op.as_str(), pts.as_slice()) {
                    ("move", [p]) => path.move_to(*p),
                    ("line", [p]) => path.line_to(*p),
                    ("quad", [c1, p]) => path.quad_to(*c1, *p),
                    ("curve", [c1, c2, p]) => path.curve_to(*c1, *c2, *p),
                    (op, pts) => return err("TypeError.ArityMismatch", format!(":{op} got {} points; :move and :line take 1, :quad takes 2, :curve takes 3", pts.len())),
                }
            }
            if matches!(c.attrs.get("closed"), Some(Value::Bool(true))) {
                path.close_path();
            }
            let center = path.bounding_box().center();
            (path, center)
        }
        kind => return err("TypeError.NotPlaceable", format!("cannot draw {kind}")),
    };
    let placed = spin(origin);
    // blend が付いていたら、その図形の描画だけを 1 枚のレイヤーにして重ね方を変える
    let blend = blend_mode(&c.attrs)?;
    if let Some(mode) = blend {
        scene.push_layer(Fill::NonZero, mode, 1.0, placed, &path);
    }
    if let Some(shader) = shader {
        draw_shader_fill(scene, &path, placed, opacity, shader, frame, key, cache)?;
    } else if let Some(fill) = brush(&c.attrs, "fill", kind)? {
        scene.fill(Fill::NonZero, placed, &fill.multiply_alpha(opacity), None, &path);
    }
    if let Some(stroke) = color(&c.attrs, "stroke", kind)? {
        scene.stroke(&stroke_style(&c.attrs)?, placed, stroke.multiply_alpha(opacity), None, &path);
    }
    if blend.is_some() {
        scene.pop_layer();
    }
    Ok(())
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
        Value::Number(n) => n.to_bits().hash(h),
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
fn draw_text(scene: &mut Scene, attrs: &Attrs, transform: Affine, scale: f64, opacity: f32, cache: &mut RenderCache) -> Result<()> {
    let kind = "TextArea";
    let Some(Value::Str(content)) = attrs.get("text") else {
        return err("NameError.UndefinedAttribute", "TextArea.text is not set");
    };
    let font_size = number(attrs, "fontSize", kind)?;
    let family = match attrs.get("font") {
        Some(Value::Str(f)) => Some(f.as_str()),
        _ => None,
    };
    let max_width = match attrs.get("w") {
        Some(Value::Number(w)) => Some((*w * scale) as f32),
        _ => None,
    };
    let align = match attrs.get("align") {
        Some(Value::Symbol(a)) => text::alignment(Some(a)),
        _ => text::alignment(None),
    };
    let layout = cache.layout(content, family, (font_size * scale) as f32, max_width, align);
    let (w, h) = (max_width.unwrap_or(layout.width()) as f64 / scale, f64::from(layout.height()) / scale);
    let (cx, cy) = anchored_center(attrs, w, h, kind)?;
    let top_left = transform * Point::new(cx - w / 2.0, cy - h / 2.0);
    let fill = color(attrs, "fill", kind)?.unwrap_or(Color::BLACK);
    text::draw(scene, layout, Affine::translate(top_left.to_vec2()), fill.multiply_alpha(opacity));
    Ok(())
}

fn number(attrs: &Attrs, name: &str, kind: &str) -> Result<f64> {
    match attrs.get(name) {
        Some(Value::Number(v)) => Ok(*v),
        Some(v) => err("TypeError.AttributeType", format!("{kind}.{name} expects Number, found {}", v.type_name())),
        None => err("NameError.UndefinedAttribute", format!("{kind}.{name} is not set")),
    }
}

/// Vector か、数値 2 つの Tuple を点として読む
fn point_of(v: &Value, whose: &str) -> Result<Point> {
    match v {
        Value::Vector(x, y) => Ok(Point::new(*x, *y)),
        Value::Tuple(items) => match items.as_slice() {
            [Value::Number(x), Value::Number(y)] => Ok(Point::new(*x, *y)),
            _ => err("TypeError.AttributeType", format!("{whose} expects Vector, found {}", v.type_name())),
        },
        other => err("TypeError.AttributeType", format!("{whose} expects Vector, found {}", other.type_name())),
    }
}

fn vector(attrs: &Attrs, name: &str, kind: &str) -> Result<Point> {
    match attrs.get(name) {
        Some(Value::Vector(x, y)) => Ok(Point::new(*x, *y)),
        Some(v) => err("TypeError.AttributeType", format!("{kind}.{name} expects Vector, found {}", v.type_name())),
        None => err("NameError.UndefinedAttribute", format!("{kind}.{name} is not set")),
    }
}

/// fill に入れられる塗り。Color か Gradient (Shader は別扱い)
fn brush(attrs: &Attrs, name: &str, kind: &str) -> Result<Option<Brush>> {
    match attrs.get(name) {
        Some(Value::Color([r, g, b, a])) => Ok(Some(Brush::Solid(Color::new([*r, *g, *b, *a])))),
        Some(Value::Object(o)) if o.borrow().kind == "Gradient" => Ok(Some(Brush::Gradient(gradient(&o.borrow().attrs)?))),
        Some(v) => err("TypeError.AttributeType", format!("{kind}.{name} expects a Paint, found {}", v.type_name())),
        None => Ok(None),
    }
}

/// Gradient を peniko の Gradient にする。座標は箱の座標
fn gradient(attrs: &Attrs) -> Result<Gradient> {
    let Some(Value::List(items)) = attrs.get("stops") else {
        return err("NameError.UndefinedAttribute", "Gradient.stops must be a List of Color");
    };
    let colors: Vec<Color> = items
        .borrow()
        .iter()
        .map(|c| match c {
            Value::Color([r, g, b, a]) => Ok(Color::new([*r, *g, *b, *a])),
            other => err("TypeError.AttributeType", format!("Gradient.stops expects Color, found {}", other.type_name())),
        })
        .collect::<Result<_>>()?;
    if colors.len() < 2 {
        return err("ValueError.OutOfRange", "Gradient.stops needs at least 2 colors");
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
        other => return err("ValueError.OutOfRange", format!("unknown Gradient.kind :{other}")),
    };
    Ok(g.with_stops(stops))
}

/// 線の描き方。端の形、角の形、破線
fn stroke_style(attrs: &Attrs) -> Result<Stroke> {
    let width = match attrs.get("strokeWidth") {
        Some(Value::Number(w)) => *w,
        _ => 0.01,
    };
    let mut stroke = Stroke::new(width);
    if let Some(Value::Symbol(cap)) = attrs.get("strokeCap") {
        stroke = stroke.with_caps(match cap.as_str() {
            "butt" => Cap::Butt,
            "round" => Cap::Round,
            "square" => Cap::Square,
            other => return err("ValueError.OutOfRange", format!("unknown strokeCap :{other}")),
        });
    }
    if let Some(Value::Symbol(join)) = attrs.get("strokeJoin") {
        stroke = stroke.with_join(match join.as_str() {
            "miter" => Join::Miter,
            "round" => Join::Round,
            "bevel" => Join::Bevel,
            other => return err("ValueError.OutOfRange", format!("unknown strokeJoin :{other}")),
        });
    }
    if let Some(Value::List(items)) = attrs.get("dash") {
        let pattern: Vec<f64> = items
            .borrow()
            .iter()
            .map(|v| match v {
                Value::Number(n) => Ok(*n),
                other => err("TypeError.AttributeType", format!("dash expects Number, found {}", other.type_name())),
            })
            .collect::<Result<_>>()?;
        if !pattern.is_empty() {
            let offset = match attrs.get("dashOffset") {
                Some(Value::Number(o)) => *o,
                _ => 0.0,
            };
            stroke = stroke.with_dashes(offset, pattern);
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
        other => return err("ValueError.OutOfRange", format!("unknown blend :{other}")),
    };
    Ok(Some(BlendMode::new(mix, Compose::SrcOver)))
}

/// 未設定なら None (stroke は任意)
fn color(attrs: &Attrs, name: &str, kind: &str) -> Result<Option<Color>> {
    match attrs.get(name) {
        Some(Value::Color([r, g, b, a])) => Ok(Some(Color::new([*r, *g, *b, *a]))),
        Some(v) => err("TypeError.AttributeType", format!("{kind}.{name} expects Color, found {}", v.type_name())),
        None => Ok(None),
    }
}

/// position (Pos) と大きさから中心座標を求める
/// position に書いた点そのもの。回転の中心に使う
fn anchor_point(attrs: &Attrs, kind: &str) -> Result<Point> {
    match attrs.get("position") {
        Some(Value::Apos(_, x, y)) => Ok(Point::new(*x, *y)),
        _ => err("TypeError.AttributeType", format!("{kind}.position must be a Pos")),
    }
}

fn anchored_center(attrs: &Attrs, w: f64, h: f64, kind: &str) -> Result<(f64, f64)> {
    let Some(Value::Apos(anchor, x, y)) = attrs.get("position") else {
        return err("TypeError.AttributeType", format!("{kind}.position must be a Pos"));
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
        other => return err("ValueError.OutOfRange", format!("unknown anchor :{other}")),
    };
    Ok((x + dx, y + dy))
}

/// preview と sheet で、その時刻に出ている字幕を画面の下に重ねる (プレイヤーの表示に似せる)。動画には入らない
pub fn overlay_subtitles(scene: &mut Scene, cache: &mut RenderCache, cues: &[crate::render::media::Cue], t: f64, width: f64, height: f64) {
    let size = (height * 0.045) as f32;
    let box_w = (width * 0.9) as f32;
    let pad = f64::from(size) * 0.35;
    let mut bottom = height * 0.94;
    // 同時に出ている字幕は、先に始まったものを下にして積む
    for cue in cues.iter().filter(|c| c.at <= t && t < c.at + c.length) {
        let layout = cache.layout(&cue.text, None, size, Some(box_w), text::alignment(Some("center")));
        let (w, h) = (f64::from(layout.width()), f64::from(layout.height()));
        let top = bottom - h - pad * 2.0;
        let x0 = (width - w) / 2.0 - pad;
        scene.fill(Fill::NonZero, Affine::IDENTITY, Color::from_rgba8(0, 0, 0, 150), None, &RoundedRect::new(x0, top, x0 + w + pad * 2.0, bottom, pad));
        text::draw(scene, layout, Affine::translate(((width - f64::from(box_w)) / 2.0, top + pad)), Color::WHITE);
        bottom = top - pad;
    }
}

/// Shader の塗り。図形の範囲 (画面内) のピクセルを compute shader で計算し、その画像で図形を塗る
#[allow(clippy::too_many_arguments)]
fn draw_shader_fill(scene: &mut Scene, path: &BezPath, transform: Affine, opacity: f32, shader: &ObjRef, frame: &Frame, key: usize, cache: &mut RenderCache) -> Result<()> {
    let sh = shader.borrow();
    let Some(Value::Func(closure)) = sh.attrs.get("color") else {
        return err("TypeError.AttributeType", "Shader.color must be a func (x, y, t)");
    };
    let args: Vec<f32> = match sh.attrs.get("args") {
        Some(Value::List(items)) => items
            .borrow()
            .iter()
            .map(|v| match v {
                Value::Number(n) => Ok(*n as f32),
                other => err("TypeError.AttributeType", format!("Shader.args must hold Numbers, found {}", other.type_name())),
            })
            .collect::<Result<_>>()?,
        Some(other) => return err("TypeError.AttributeType", format!("Shader.args expects List, found {}", other.type_name())),
        None => Vec::new(),
    };
    let samples = match sh.attrs.get("samples") {
        Some(Value::Number(n)) if *n >= 1.0 => *n as u32,
        Some(other) => return err("TypeError.AttributeType", format!("Shader.samples must be a Number of 1 or more, found {other}")),
        None => 1,
    };
    let Some(runner) = cache.shaders.as_mut() else {
        return err("RuntimeError.ShaderUnavailable", "a Shader fill needs the GPU (render, preview, sheet)");
    };
    let bounds = transform.transform_rect_bbox(path.bounding_box()).intersect(frame.viewport);
    if bounds.is_zero_area() {
        return Ok(());
    }
    let (x0, y0) = (bounds.x0.floor(), bounds.y0.floor());
    let width = (bounds.x1.ceil() - x0).max(1.0) as u32;
    let height = (bounds.y1.ceil() - y0).max(1.0) as u32;
    // 箱 → ピクセルは拡大と平行移動だけなので、逆は 1 次式
    let [s, _, _, _, tx, ty] = transform.as_coeffs();
    let request = crate::render::shader::Request {
        shape: key,
        closure,
        args: &args,
        t: frame.t,
        width,
        height,
        origin: ((x0 - tx) / s, (y0 - ty) / s),
        step: (1.0 / s, 1.0 / s),
        samples,
    };
    let image = runner.run(request)?;
    let mut brush = ImageBrush::new(image);
    brush.sampler.alpha = opacity;
    scene.fill(Fill::NonZero, transform, &Brush::Image(brush), Some(transform.inverse() * Affine::translate((x0, y0))), path);
    Ok(())
}
