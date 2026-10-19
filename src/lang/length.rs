//! 単位つきの長さ。`20vy` のように書き、描く先の箱が決まってから数にする。
//!
//! 単位ごとの係数を持つ和なので、足し引きと数との掛け割りは、解決を待たずにできる。

use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Length {
    /// 単位なし。箱の単位
    pub bare: f64,
    pub vx: f64,
    pub vy: f64,
    pub vmin: f64,
    pub vmax: f64,
    pub px: f64,
}

/// 長さを数にするときの基準。描く先の箱と、出力のピクセル
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Basis {
    pub width: f64,
    pub height: f64,
    /// 出力のピクセル 1 つが、箱の単位でいくつか
    pub pixel: f64,
}

impl Basis {
    /// 箱がそのまま 1 単位 1 ピクセルの基準 (単位つきの長さが現れない場面用)
    pub const UNIT: Basis = Basis { width: 100.0, height: 100.0, pixel: 1.0 };
}

thread_local! {
    static BASIS: Cell<Basis> = const { Cell::new(Basis::UNIT) };
}

/// 描く間の基準。箱を描き始めるときに入れ、描き終えたら返す
pub fn set_basis(basis: Basis) -> Basis {
    BASIS.with(|b| b.replace(basis))
}

pub fn basis() -> Basis {
    BASIS.with(Cell::get)
}

impl Length {
    pub fn bare(v: f64) -> Self {
        Length { bare: v, ..Length::default() }
    }

    /// 単位の名前から作る。単位でなければ None
    pub fn with_unit(unit: &str, v: f64) -> Option<Self> {
        let mut l = Length::default();
        match unit {
            "vx" => l.vx = v,
            "vy" => l.vy = v,
            "vmin" => l.vmin = v,
            "vmax" => l.vmax = v,
            "px" => l.px = v,
            _ => return None,
        }
        Some(l)
    }

    pub fn plus(self, o: Length) -> Length {
        Length { bare: self.bare + o.bare, vx: self.vx + o.vx, vy: self.vy + o.vy, vmin: self.vmin + o.vmin, vmax: self.vmax + o.vmax, px: self.px + o.px }
    }

    pub fn scaled(self, k: f64) -> Length {
        Length { bare: self.bare * k, vx: self.vx * k, vy: self.vy * k, vmin: self.vmin * k, vmax: self.vmax * k, px: self.px * k }
    }

    pub fn minus(self, o: Length) -> Length {
        self.plus(o.scaled(-1.0))
    }

    /// 単位なしの数だけなら、その数
    pub fn as_bare(self) -> Option<f64> {
        (self.vx == 0.0 && self.vy == 0.0 && self.vmin == 0.0 && self.vmax == 0.0 && self.px == 0.0).then_some(self.bare)
    }

    /// 基準に対する数。vx は箱の幅の 1/100、vy は高さの 1/100
    pub fn resolve(self, b: Basis) -> f64 {
        self.bare
            + self.vx * b.width / 100.0
            + self.vy * b.height / 100.0
            + self.vmin * b.width.min(b.height) / 100.0
            + self.vmax * b.width.max(b.height) / 100.0
            + self.px * b.pixel
    }

    /// 描いている箱に対する数
    pub fn value(self) -> f64 {
        self.resolve(basis())
    }
}

impl std::fmt::Display for Length {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let parts = [(self.bare, ""), (self.vx, "vx"), (self.vy, "vy"), (self.vmin, "vmin"), (self.vmax, "vmax"), (self.px, "px")];
        let mut first = true;
        for (v, unit) in parts.into_iter().filter(|(v, _)| *v != 0.0) {
            if !first {
                write!(f, "{}", if v < 0.0 { " - " } else { " + " })?;
                write!(f, "{}{unit}", v.abs())?;
            } else {
                write!(f, "{v}{unit}")?;
            }
            first = false;
        }
        if first {
            write!(f, "0")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Basis = Basis { width: 16.0, height: 9.0, pixel: 0.0125 };

    #[test]
    fn vy_is_a_hundredth_of_the_height() {
        assert!((Length::with_unit("vy", 20.0).unwrap().resolve(BOX) - 1.8).abs() < 1e-12);
    }

    #[test]
    fn vmin_and_vmax_pick_a_side() {
        assert!((Length::with_unit("vmin", 100.0).unwrap().resolve(BOX) - 9.0).abs() < 1e-12);
        assert!((Length::with_unit("vmax", 100.0).unwrap().resolve(BOX) - 16.0).abs() < 1e-12);
    }

    #[test]
    fn a_sum_resolves_term_by_term() {
        let l = Length::with_unit("vx", 50.0).unwrap().plus(Length::bare(1.0)).minus(Length::with_unit("px", 80.0).unwrap());
        assert!((l.resolve(BOX) - (8.0 + 1.0 - 1.0)).abs() < 1e-12);
    }

    #[test]
    fn bare_only_is_bare() {
        assert_eq!(Length::bare(3.0).as_bare(), Some(3.0));
        assert_eq!(Length::with_unit("vx", 3.0).unwrap().as_bare(), None);
    }
}
