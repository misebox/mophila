//! テキストのレイアウトと、描画命令のキャッシュ。フォントの列挙は parley (fontique) に任せる

use std::collections::HashMap;

use parley::fontique::Collection;
pub use parley::layout::Alignment;
use parley::layout::{AlignmentOptions, Layout, PositionedLayoutItem};
use parley::style::{FontFamily, FontFamilyName, GenericFamily, StyleProperty};
use parley::{FontContext, LayoutContext};
use vello::Scene;
use vello::kurbo::{Affine, Stroke};
use vello::peniko::{Color, Fill};
use vello::peniko::StyleRef;

/// フレームをまたいで持ち回るもの。テキストのレイアウトと、図形ごとの描画命令
pub struct RenderCache {
    fonts: FontContext,
    layouts: LayoutContext<[u8; 4]>,
    /// (text, family, size_px, max_width, align) → レイアウト
    layout_cache: HashMap<LayoutKey, Layout<[u8; 4]>>,
    /// 図形の通し番号 → 描画命令。指紋が同じならそのまま使う
    pub fragments: HashMap<u64, Fragment>,
    /// いま組んでいるフレームの番号。使わなくなったものを手放すのに使う
    pub frame: u64,
    /// このフレームで作った、透ける View のレイヤーの合計 (バイト)
    pub layer_bytes: u64,
    /// Shader を GPU で走らせるもの。描画する側 (render / preview) が装置を作ってから入れる
    pub shaders: Option<crate::render::shader::ShaderRunner>,
    /// 読み込んだ画像。ファイルごとに 1 度だけ開く
    images: HashMap<std::path::PathBuf, vello::peniko::ImageData>,
}

/// 1 フレームに要った GPU のメモリ (バイト)
#[derive(Default, Clone, Copy)]
pub struct Bytes {
    /// Shader の塗りのテクスチャと、ズームの帯
    pub shaders: u64,
    /// 透ける View のレイヤー (混色用)
    pub layers: u64,
}

impl Bytes {
    pub fn total(self) -> u64 {
        self.shaders + self.layers
    }
}

/// 前に組んだ描画命令
pub struct Fragment {
    /// 描画に関わる属性の指紋。同じならそのまま使える
    pub print: u64,
    pub scene: Scene,
    /// 最後に使ったフレームの番号
    pub used: u64,
}

#[derive(Hash, PartialEq, Eq)]
struct LayoutKey {
    text: String,
    family: Option<String>,
    size_bits: u32,
    width_bits: Option<u32>,
    align: u8,
}

impl RenderCache {
    pub fn new() -> Self {
        Self { fonts: FontContext::new(), layouts: LayoutContext::new(), layout_cache: HashMap::new(), fragments: HashMap::new(), frame: 0, layer_bytes: 0, shaders: None, images: HashMap::new() }
    }

    /// システムにそのファミリ名のフォントがあるか
    pub fn family_exists(&mut self, family: &str) -> bool {
        let collection: &mut Collection = &mut self.fonts.collection;
        collection.family_id(family).is_some()
    }

    /// この機械で使えるフォント名。並べ替えて重複を除く
    pub fn families(&mut self) -> Vec<String> {
        let collection: &mut Collection = &mut self.fonts.collection;
        let mut names: Vec<String> = collection.family_names().map(str::to_string).collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// 名前が見つからないときに出す、近いもの。
    /// 共有する語の数を第一に、頭からどれだけ一致するかを第二に見る
    /// 候補のうち、この機械にある最初の名前
    pub fn first_family(&mut self, names: &[String]) -> Option<String> {
        names.iter().find(|n| self.family_exists(n)).cloned()
    }

    pub fn nearest(&mut self, wanted: &str) -> Vec<String> {
        let wanted_lower = wanted.to_lowercase();
        let words: Vec<&str> = wanted_lower.split_whitespace().collect();
        let prefix = |name: &str| name.chars().zip(wanted_lower.chars()).take_while(|(a, b)| a == b).count();
        let mut scored: Vec<(usize, usize, String)> = self
            .families()
            .into_iter()
            .filter_map(|name| {
                let lower = name.to_lowercase();
                let hits = words.iter().filter(|w| lower.split_whitespace().any(|part| part == **w)).count();
                (hits > 0).then(|| (hits, prefix(&lower), name))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)).then_with(|| a.2.len().cmp(&b.2.len())));
        scored.into_iter().take(3).map(|(_, _, n)| n).collect()
    }

    /// 画像を読む。同じファイルは 1 度だけ開いて使い回す
    pub fn image(&mut self, path: &std::path::Path) -> Result<vello::peniko::ImageData, String> {
        if let Some(found) = self.images.get(path) {
            return Ok(found.clone());
        }
        let decoded = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.into_rgba8();
        let (width, height) = decoded.dimensions();
        let data = vello::peniko::ImageData {
            data: vello::peniko::Blob::new(std::sync::Arc::new(decoded.into_raw())),
            format: vello::peniko::ImageFormat::Rgba8,
            alpha_type: vello::peniko::ImageAlphaType::Alpha,
            width,
            height,
        };
        self.images.insert(path.to_path_buf(), data.clone());
        Ok(data)
    }

    /// ピクセル単位でレイアウトする。max_width が None なら折り返さない。同じ入力なら前回の結果を返す
    /// このフレームに要った GPU のメモリ
    pub fn bytes(&self) -> Bytes {
        Bytes { shaders: self.shaders.as_ref().map_or(0, |r| r.bytes()), layers: self.layer_bytes }
    }

    pub fn layout(&mut self, text: &str, family: Option<&str>, size_px: f32, max_width: Option<f32>, align: Alignment) -> &Layout<[u8; 4]> {
        let key = LayoutKey {
            text: text.to_string(),
            family: family.map(str::to_string),
            size_bits: size_px.to_bits(),
            width_bits: max_width.map(f32::to_bits),
            align: align as u8,
        };
        if !self.layout_cache.contains_key(&key) {
            let mut builder = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
            let family = match family {
                Some(name) => FontFamily::Single(FontFamilyName::Named(name.into())),
                None => FontFamily::Single(FontFamilyName::Generic(GenericFamily::SansSerif)),
            };
            builder.push_default(StyleProperty::FontFamily(family));
            builder.push_default(StyleProperty::FontSize(size_px));
            let mut layout = builder.build(text);
            layout.break_all_lines(max_width);
            layout.align(align, AlignmentOptions::default());
            self.layout_cache.insert(
                LayoutKey { text: key.text.clone(), family: key.family.clone(), size_bits: key.size_bits, width_bits: key.width_bits, align: key.align },
                layout,
            );
        }
        self.layout_cache.get(&key).expect("inserted above")
    }
}

/// レイアウト済みのテキストを描く。transform はレイアウト座標 (px) から出力座標への変換
/// 文字を描く。stroke があれば、塗りの上から縁取りを重ねる
pub fn draw(scene: &mut Scene, layout: &Layout<[u8; 4]>, transform: Affine, color: Color, stroke: Option<(&Stroke, Color)>) {
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else { continue };
            let run = glyph_run.run();
            let mut x = glyph_run.offset();
            let y = glyph_run.baseline();
            let placed: Vec<vello::Glyph> = glyph_run
                .glyphs()
                .map(|g| {
                    let gx = x + g.x;
                    x += g.advance;
                    vello::Glyph { id: g.id, x: gx, y: y - g.y }
                })
                .collect();
            let mut run_scene = |style: StyleRef, brush: Color| {
                scene
                    .draw_glyphs(run.font())
                    .font_size(run.font_size())
                    .transform(transform)
                    .normalized_coords(run.normalized_coords())
                    .brush(brush)
                    .draw(style, placed.iter().copied());
            };
            run_scene(StyleRef::Fill(Fill::NonZero), color);
            if let Some((width, ink)) = stroke {
                run_scene(StyleRef::Stroke(width), ink);
            }
        }
    }
}

pub fn alignment(name: Option<&str>) -> Alignment {
    match name {
        Some("center") => Alignment::Center,
        Some("right") => Alignment::Right,
        Some("left") => Alignment::Left,
        _ => Alignment::Start,
    }
}
