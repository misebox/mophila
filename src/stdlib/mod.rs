//! 標準ライブラリ。`import name` で読めるもの。
//! Rust で書いたもの (math) と、本体に埋め込んだ .moph がある。
//! .moph は include_str! で実行ファイルの中に入るので、別に配るファイルは無い

pub mod math;

use crate::lang::value::Module;

/// 埋め込んだ .moph の置き場を指す、実在しないパスの頭。
/// 中の相対 import (`import .night`) はここからの相対で解く
pub const ROOT: &str = "std:";

pub enum Lib {
    /// Rust の実装
    Native(Module),
    /// 埋め込んだ .moph の (パス, ソース)。呼ぶ側が別のスコープで実行する
    Script(&'static str, &'static str),
}

/// 埋め込む .moph。(import する名前, src/stdlib からのパス, ソース)。基本的なものから順。
/// 名前が空のものは他のファイルから import されるだけで、それ自体は import できない
pub const FILES: &[(&str, &str, &str)] = &[
    ("color", "color.moph", include_str!("color.moph")),
    ("shape", "shape.moph", include_str!("shape.moph")),
    ("layout", "layout.moph", include_str!("layout.moph")),
    ("easing", "easing.moph", include_str!("easing.moph")),
    ("animation", "animation.moph", include_str!("animation.moph")),
    ("chart", "chart.moph", include_str!("chart.moph")),
    ("diagram", "diagram.moph", include_str!("diagram.moph")),
    ("text", "text.moph", include_str!("text.moph")),
    ("ui", "ui.moph", include_str!("ui.moph")),
    ("backdrop", "backdrop.moph", include_str!("backdrop.moph")),
    ("focus", "focus.moph", include_str!("focus.moph")),
    ("media", "media.moph", include_str!("media.moph")),
    ("clock", "clock.moph", include_str!("clock.moph")),
    ("meter", "meter.moph", include_str!("meter.moph")),
    ("fractal", "fractal.moph", include_str!("fractal.moph")),
    ("palette", "materials/palette/index.moph", include_str!("materials/palette/index.moph")),
    ("", "materials/palette/palette.moph", include_str!("materials/palette/palette.moph")),
    ("", "materials/palette/house.moph", include_str!("materials/palette/house.moph")),
    ("", "materials/palette/paper.moph", include_str!("materials/palette/paper.moph")),
    ("", "materials/palette/earth.moph", include_str!("materials/palette/earth.moph")),
    ("", "materials/palette/mono.moph", include_str!("materials/palette/mono.moph")),
    ("", "materials/palette/night.moph", include_str!("materials/palette/night.moph")),
    ("", "materials/palette/neon.moph", include_str!("materials/palette/neon.moph")),
    ("", "materials/palette/ocean.moph", include_str!("materials/palette/ocean.moph")),
    ("", "materials/palette/slate.moph", include_str!("materials/palette/slate.moph")),
    ("", "materials/palette/candy.moph", include_str!("materials/palette/candy.moph")),
    ("", "materials/palette/forest.moph", include_str!("materials/palette/forest.moph")),
    ("", "materials/palette/sunset.moph", include_str!("materials/palette/sunset.moph")),
    ("", "materials/palette/blueprint.moph", include_str!("materials/palette/blueprint.moph")),
    ("pattern", "materials/pattern/index.moph", include_str!("materials/pattern/index.moph")),
    ("", "materials/pattern/stripes.moph", include_str!("materials/pattern/stripes.moph")),
    ("", "materials/pattern/hatch.moph", include_str!("materials/pattern/hatch.moph")),
    ("", "materials/pattern/checker.moph", include_str!("materials/pattern/checker.moph")),
    ("", "materials/pattern/dots.moph", include_str!("materials/pattern/dots.moph")),
    ("", "materials/pattern/grid.moph", include_str!("materials/pattern/grid.moph")),
    ("", "materials/pattern/cross_hatch.moph", include_str!("materials/pattern/cross_hatch.moph")),
    ("", "materials/pattern/waves.moph", include_str!("materials/pattern/waves.moph")),
    ("", "materials/pattern/zigzag.moph", include_str!("materials/pattern/zigzag.moph")),
    ("", "materials/pattern/diamonds.moph", include_str!("materials/pattern/diamonds.moph")),
    ("", "materials/pattern/triangles.moph", include_str!("materials/pattern/triangles.moph")),
    ("", "materials/pattern/bricks.moph", include_str!("materials/pattern/bricks.moph")),
    ("", "materials/pattern/noise.moph", include_str!("materials/pattern/noise.moph")),
    ("icon", "materials/icon/index.moph", include_str!("materials/icon/index.moph")),
    ("", "materials/icon/icon.moph", include_str!("materials/icon/icon.moph")),
    ("", "materials/icon/check.moph", include_str!("materials/icon/check.moph")),
    ("", "materials/icon/cross.moph", include_str!("materials/icon/cross.moph")),
    ("", "materials/icon/plus.moph", include_str!("materials/icon/plus.moph")),
    ("", "materials/icon/minus.moph", include_str!("materials/icon/minus.moph")),
    ("", "materials/icon/circle.moph", include_str!("materials/icon/circle.moph")),
    ("", "materials/icon/triangle.moph", include_str!("materials/icon/triangle.moph")),
    ("", "materials/icon/star.moph", include_str!("materials/icon/star.moph")),
    ("", "materials/icon/arrow.moph", include_str!("materials/icon/arrow.moph")),
    ("", "materials/icon/chevron.moph", include_str!("materials/icon/chevron.moph")),
    ("", "materials/icon/play.moph", include_str!("materials/icon/play.moph")),
    ("", "materials/icon/pause.moph", include_str!("materials/icon/pause.moph")),
    ("", "materials/icon/stop.moph", include_str!("materials/icon/stop.moph")),
    ("", "materials/icon/heart.moph", include_str!("materials/icon/heart.moph")),
    ("", "materials/icon/home.moph", include_str!("materials/icon/home.moph")),
    ("", "materials/icon/bolt.moph", include_str!("materials/icon/bolt.moph")),
    ("", "materials/icon/pin.moph", include_str!("materials/icon/pin.moph")),
    ("", "materials/icon/search.moph", include_str!("materials/icon/search.moph")),
    ("", "materials/icon/flag.moph", include_str!("materials/icon/flag.moph")),
    ("", "materials/icon/cloud.moph", include_str!("materials/icon/cloud.moph")),
    ("", "materials/icon/user.moph", include_str!("materials/icon/user.moph")),
    ("", "materials/icon/mail.moph", include_str!("materials/icon/mail.moph")),
    ("", "materials/icon/folder.moph", include_str!("materials/icon/folder.moph")),
    ("", "materials/icon/file.moph", include_str!("materials/icon/file.moph")),
    ("", "materials/icon/clock.moph", include_str!("materials/icon/clock.moph")),
    ("", "materials/icon/gear.moph", include_str!("materials/icon/gear.moph")),
    ("", "materials/icon/lock.moph", include_str!("materials/icon/lock.moph")),
    ("", "materials/icon/bell.moph", include_str!("materials/icon/bell.moph")),
    ("", "materials/icon/chat.moph", include_str!("materials/icon/chat.moph")),
];

pub fn find(name: &str) -> Option<Lib> {
    if name == "math" {
        return Some(Lib::Native(math::module()));
    }
    FILES.iter().find(|(n, ..)| *n == name && !n.is_empty()).map(|(_, path, src)| Lib::Script(path, src))
}

/// 標準ライブラリの中のファイル。key は ROOT から始まる正規化したパス
pub fn embedded(key: &str) -> Option<&'static str> {
    let path = key.strip_prefix(ROOT)?.trim_start_matches('/');
    FILES.iter().find(|(_, p, _)| *p == path).map(|(.., src)| *src)
}

/// import できる名前の一覧 (補完や文書に)
pub fn names() -> Vec<&'static str> {
    std::iter::once("math").chain(FILES.iter().map(|(n, ..)| *n).filter(|n| !n.is_empty())).collect()
}
