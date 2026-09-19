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
