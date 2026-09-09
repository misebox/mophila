//! 標準ライブラリ。`import name` で読めるもの。
//! Rust で書いたもの (math) と、本体に埋め込んだ .moph (fractal) がある

pub mod math;

use crate::lang::value::Module;

pub enum Lib {
    /// Rust の実装
    Native(Module),
    /// 埋め込んだ .moph のソース。呼ぶ側が別のスコープで実行する
    Script(&'static str),
}

/// 埋め込む .moph。(名前, ソース)。基本的なものから順
pub const SCRIPTS: &[(&str, &str)] = &[
    ("color", include_str!("color.moph")),
    ("shape", include_str!("shape.moph")),
    ("layout", include_str!("layout.moph")),
    ("transition", include_str!("transition.moph")),
    ("fractal", include_str!("fractal.moph")),
];

pub fn find(name: &str) -> Option<Lib> {
    if name == "math" {
        return Some(Lib::Native(math::module()));
    }
    SCRIPTS.iter().find(|(n, _)| *n == name).map(|(_, src)| Lib::Script(src))
}

/// 名前の一覧 (補完や文書に)
pub fn names() -> Vec<&'static str> {
    std::iter::once("math").chain(SCRIPTS.iter().map(|(n, _)| *n)).collect()
}
