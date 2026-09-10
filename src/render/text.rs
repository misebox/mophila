//! テキストのレイアウトと、描画命令のキャッシュ。フォントの列挙は parley (fontique) に任せる

use std::collections::HashMap;

use parley::fontique::Collection;
use parley::layout::{Alignment, AlignmentOptions, Layout, PositionedLayoutItem};
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
    /// 図形のポインタ → (属性の指紋, 描画命令)。指紋が同じならそのまま使う
    pub fragments: HashMap<usize, (u64, Scene)>,
    /// Shader を GPU で走らせるもの。描画する側 (render / preview) が装置を作ってから入れる
    pub shaders: Option<crate::render::shader::ShaderRunner>,
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
        Self { fonts: FontContext::new(), layouts: LayoutContext::new(), layout_cache: HashMap::new(), fragments: HashMap::new(), shaders: None }
    }

    /// システムにそのファミリ名のフォントがあるか
    pub fn family_exists(&mut self, family: &str) -> bool {
        let collection: &mut Collection = &mut self.fonts.collection;
        collection.family_id(family).is_some()
    }

    /// ピクセル単位でレイアウトする。max_width が None なら折り返さない。同じ入力なら前回の結果を返す
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
