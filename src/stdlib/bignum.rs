//! 多倍長の固定小数点と、それを使うマンデルブロの下ごしらえ。
//!
//! 深いズームでは中心の座標が f64 に入らない。文字列で受け取って、必要な桁で基準軌道を作る。
//! 使うのは加算・減算・乗算だけで、値の範囲も小さいので、汎用の多倍長は持たずに自前で足りる。
//! 読み込みのときに 1 度作って全フレームで使い回すので、速さは要らない。
//!
//! 反復を飛ばす表 (BLA) と、ミニチュアの中心を探すニュートン法もここ。どれも読み込み時の計算

use std::collections::HashMap;

use crate::lang::error::{Kind, MophError, Result, err};
use crate::lang::value::{Module, Value};

/// 符号と大きさで持つ固定小数点。
/// 値は (-1)^neg * (limbs[n-1] + Σ_{i<n-1} limbs[i] * 2^(-64*(n-1-i)))。
/// いちばん上の limb が整数部、残りが小数部
#[derive(Clone, Debug, PartialEq, Eq)]
struct Fix {
    neg: bool,
    limbs: Vec<u64>,
}

const LIMB: f64 = 18_446_744_073_709_551_616.0;

impl Fix {
    fn zero(n: usize) -> Fix {
        Fix { neg: false, limbs: vec![0; n] }
    }

    fn len(&self) -> usize {
        self.limbs.len()
    }

    fn is_zero(&self) -> bool {
        self.limbs.iter().all(|l| *l == 0)
    }

    /// 大きさだけを比べる
    fn cmp_abs(&self, other: &Fix) -> std::cmp::Ordering {
        for i in (0..self.len()).rev() {
            match self.limbs[i].cmp(&other.limbs[i]) {
                std::cmp::Ordering::Equal => {}
                other => return other,
            }
        }
        std::cmp::Ordering::Equal
    }

    /// 大きさの足し算。溢れた分は捨てる (値は小さい範囲にしか使わない)
    fn add_abs(&self, other: &Fix) -> Vec<u64> {
        let mut out = vec![0; self.len()];
        let mut carry = 0u64;
        for i in 0..self.len() {
            let (a, over) = self.limbs[i].overflowing_add(other.limbs[i]);
            let (b, over2) = a.overflowing_add(carry);
            out[i] = b;
            carry = u64::from(over) + u64::from(over2);
        }
        out
    }

    /// 大きさの引き算。self >= other であること
    fn sub_abs(&self, other: &Fix) -> Vec<u64> {
        let mut out = vec![0; self.len()];
        let mut borrow = 0u64;
        for i in 0..self.len() {
            let (a, under) = self.limbs[i].overflowing_sub(other.limbs[i]);
            let (b, under2) = a.overflowing_sub(borrow);
            out[i] = b;
            borrow = u64::from(under) + u64::from(under2);
        }
        out
    }

    fn add(&self, other: &Fix) -> Fix {
        match self.neg == other.neg {
            true => Fix { neg: self.neg, limbs: self.add_abs(other) },
            false => match self.cmp_abs(other) {
                std::cmp::Ordering::Less => Fix { neg: other.neg, limbs: other.sub_abs(self) },
                _ => Fix { neg: self.neg && !self.is_zero(), limbs: self.sub_abs(other) },
            },
        }
    }

    fn sub(&self, other: &Fix) -> Fix {
        self.add(&Fix { neg: !other.neg, limbs: other.limbs.clone() })
    }

    /// 掛け算。小数部の桁がそろうように、下から n-1 limb ぶんを落とす
    fn mul(&self, other: &Fix) -> Fix {
        let n = self.len();
        let mut wide = vec![0u64; n * 2];
        for i in 0..n {
            let mut carry = 0u128;
            for j in 0..n {
                let at = i + j;
                let cur = u128::from(wide[at]) + u128::from(self.limbs[i]) * u128::from(other.limbs[j]) + carry;
                wide[at] = cur as u64;
                carry = cur >> 64;
            }
            // 繰り上がりを上へ。いちばん上から溢れた分は整数部より上なので捨てる
            let mut at = i + n;
            while carry > 0 && at < wide.len() {
                let cur = u128::from(wide[at]) + carry;
                wide[at] = cur as u64;
                carry = cur >> 64;
                at += 1;
            }
        }
        let limbs = wide[n - 1..n * 2 - 1].to_vec();
        let neg = self.neg != other.neg && limbs.iter().any(|l| *l != 0);
        Fix { neg, limbs }
    }

    /// 小さい数で割る (10 進の読み取りに使う)
    fn div_small(&mut self, by: u64) {
        let mut rest = 0u128;
        for i in (0..self.len()).rev() {
            let cur = (rest << 64) | u128::from(self.limbs[i]);
            self.limbs[i] = (cur / u128::from(by)) as u64;
            rest = cur % u128::from(by);
        }
    }

    /// 小さい数を掛ける (10 進の書き出しに使う)
    fn mul_small(&mut self, by: u64) {
        let mut carry = 0u128;
        for i in 0..self.len() {
            let cur = u128::from(self.limbs[i]) * u128::from(by) + carry;
            self.limbs[i] = cur as u64;
            carry = cur >> 64;
        }
    }

    /// いちばん上の 0 でない limb から 2 つを取って f64 にする。小さい値でも桁を落とさない
    /// (f64 に入らないほど小さければ 0)
    fn to_f64(&self) -> f64 {
        let n = self.len();
        let Some(top) = (0..n).rev().find(|i| self.limbs[*i] != 0) else { return 0.0 };
        let hi = self.limbs[top] as f64;
        let lo = if top > 0 { self.limbs[top - 1] as f64 } else { 0.0 };
        // 2^e を 2 回に分けて掛ける。1 回だと e < -1074 で 0 になり、小さい値が消える
        let e = 64 * (top as i32 - (n as i32 - 1));
        let out = (hi + lo / LIMB) * 2f64.powi(e / 2) * 2f64.powi(e - e / 2);
        match self.neg {
            true => -out,
            false => out,
        }
    }

    /// f64 から。細かさは limb の数まで、それより下の bit は落ちる
    fn from_f64(x: f64, n: usize) -> Fix {
        let mut out = Fix::zero(n);
        let a = x.abs();
        if a == 0.0 || !a.is_finite() {
            return out;
        }
        let bits = a.to_bits();
        let raw = ((bits >> 52) & 0x7ff) as i64;
        let frac = bits & ((1u64 << 52) - 1);
        let (mant, exp) = match raw {
            0 => (frac, -1074),
            _ => (frac | (1u64 << 52), raw - 1075),
        };
        // 値は mant * 2^exp。固定小数点の中で mant の最下位 bit が来る位置 (下から数えて)
        let pos = 64 * (n as i64 - 1) + exp;
        let wide = u128::from(mant);
        if pos < 0 {
            if pos > -53 {
                out.limbs[0] = (wide >> (-pos) as u32) as u64;
            }
        } else {
            let idx = (pos / 64) as usize;
            let v = wide << (pos % 64) as u32;
            if idx < n {
                out.limbs[idx] = v as u64;
            }
            if idx + 1 < n {
                out.limbs[idx + 1] = (v >> 64) as u64;
            }
        }
        out.neg = x < 0.0;
        out
    }

    /// "-0.7436438870371587" のような 10 進の文字列から。指数表記は受けない
    fn parse(text: &str, n: usize) -> Result<Fix> {
        let text = text.trim();
        let (neg, rest) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        let (int_part, frac_part) = match rest.split_once('.') {
            Some((a, b)) => (a, b),
            None => (rest, ""),
        };
        let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if int_part.is_empty() || !digits(int_part) || !digits(frac_part) {
            return err(Kind::ArgumentType, format!("\"{text}\" is not a decimal number like -0.743643887"));
        }
        let whole: u64 = int_part.parse().map_err(|_| MophError::new(Kind::OutOfRange, format!("{int_part} is too large")))?;
        // 小数部は後ろから。acc = (acc + 桁) / 10 を繰り返す
        let mut out = Fix::zero(n);
        for d in frac_part.bytes().rev() {
            out.limbs[n - 1] += u64::from(d - b'0');
            out.div_small(10);
        }
        out.limbs[n - 1] += whole;
        out.neg = neg;
        Ok(out)
    }

    /// 10 進の文字列。小数点以下 digits 桁 (切り捨て)
    fn to_decimal(&self, digits: usize) -> String {
        let n = self.len();
        // 最後の桁で丸める: 5 × 10^-(digits+1) を足してから切る
        let mut half = Fix::zero(n);
        half.limbs[n - 1] = 5;
        for _ in 0..=digits {
            half.div_small(10);
        }
        let rounded = Fix { neg: false, limbs: self.limbs.clone() }.add(&half);
        let mut text = String::new();
        if self.neg && !self.is_zero() {
            text.push('-');
        }
        text.push_str(&rounded.limbs[n - 1].to_string());
        text.push('.');
        let mut frac = rounded;
        frac.limbs[n - 1] = 0;
        for _ in 0..digits {
            frac.mul_small(10);
            text.push((b'0' + frac.limbs[n - 1] as u8) as char);
            frac.limbs[n - 1] = 0;
        }
        text
    }
}

/// 桁数から limb の数を決める。10 進 1 桁はおよそ 3.33 bit。余裕を 1 limb 持つ
fn limbs_for(digits: usize) -> usize {
    (digits as f64 * 3.33 / 64.0).ceil() as usize + 2
}

/// z ← z² + c を 1 回
fn step(z: (&Fix, &Fix), c: (&Fix, &Fix)) -> (Fix, Fix) {
    let re2 = z.0.mul(z.0);
    let im2 = z.1.mul(z.1);
    let im = z.0.mul(z.1).add(&z.0.mul(z.1)).add(c.1);
    (re2.sub(&im2).add(c.0), im)
}

/// 中心の点の軌道を多倍長で回し、f64 の [re, im, re, im, ...] で返す。
/// 脱出したらそこで止める
fn reference_orbit(re: &str, im: &str, digits: usize, steps: usize) -> Result<Vec<f64>> {
    let n = limbs_for(digits);
    let c = (Fix::parse(re, n)?, Fix::parse(im, n)?);
    let mut z = (Fix::zero(n), Fix::zero(n));
    let mut out = Vec::with_capacity(steps * 2);
    for _ in 0..steps {
        let (zr, zi) = (z.0.to_f64(), z.1.to_f64());
        out.push(zr);
        out.push(zi);
        if zr * zr + zi * zi > 4.0 {
            return Ok(out);
        }
        z = step((&z.0, &z.1), (&c.0, &c.1));
    }
    Ok(out)
}

/// 仮数 2 つと共通の指数で持つ複素数。BLA の係数は倍率の桁を超えるので f64 に収まらない。
/// 値は (re, im) * 2^e。大きいほうの仮数を [0.5, 1) に保つ
#[derive(Clone, Copy, Debug)]
struct Cx {
    re: f64,
    im: f64,
    e: i32,
}

impl Cx {
    fn new(re: f64, im: f64) -> Cx {
        Cx { re, im, e: 0 }.norm()
    }

    fn is_zero(self) -> bool {
        self.re == 0.0 && self.im == 0.0
    }

    fn norm(self) -> Cx {
        let s = self.re.abs().max(self.im.abs());
        if s == 0.0 || !s.is_finite() {
            return Cx { re: self.re, im: self.im, e: 0 };
        }
        let k = s.log2().floor() as i32 + 1;
        let f = 2f64.powi(-k);
        Cx { re: self.re * f, im: self.im * f, e: self.e + k }
    }

    fn mul(self, o: Cx) -> Cx {
        Cx { re: self.re * o.re - self.im * o.im, im: self.re * o.im + self.im * o.re, e: self.e + o.e }.norm()
    }

    fn add(self, o: Cx) -> Cx {
        if self.is_zero() {
            return o;
        }
        if o.is_zero() {
            return self;
        }
        let (big, small) = if self.e >= o.e { (self, o) } else { (o, self) };
        // 指数の差が大きければ小さいほうは消える (2 の負の大きな乗は 0 になる)
        let f = 2f64.powi((small.e - big.e).max(-2000));
        Cx { re: big.re + small.re * f, im: big.im + small.im * f, e: big.e }.norm()
    }

    /// 1 / z
    fn inv(self) -> Cx {
        let d = self.re * self.re + self.im * self.im;
        Cx { re: self.re / d, im: -self.im / d, e: -self.e }.norm()
    }

    /// 2^k
    fn pow2(k: i32) -> Cx {
        Cx { re: 0.5, im: 0.0, e: k + 1 }
    }

    /// |z|
    fn abs(self) -> f64 {
        self.log2_abs().exp2()
    }

    /// log2 |z|。0 なら NEVER
    fn log2_abs(self) -> f64 {
        let s = self.re.hypot(self.im);
        match s == 0.0 {
            true => NEVER,
            false => s.log2() + f64::from(self.e),
        }
    }
}

/// 「有効になることは無い」半径 (log2)。-inf は f32 にすると扱いに困るので、十分小さい数
const NEVER: f64 = -1.0e30;

/// 反復を l 回まとめて飛ばす線形近似。d' = A d + B c。
/// 使えるのは |d| < 2^p - 2^s·|c| のとき。半径を c の 1 次の下界で持つので、
/// 画素の差 c が倍率で変わっても表は 1 本で済む (焼き込むと倍率ごとに作り直しになる)
#[derive(Clone, Copy)]
struct Bla {
    a: Cx,
    b: Cx,
    /// c = 0 のときの半径 (log2)
    p: f64,
    /// c に掛かる傾き (log2)。NEVER なら 0
    s: f64,
}

/// log2(2^a + 2^b)
fn log2_add(a: f64, b: f64) -> f64 {
    let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
    match lo <= NEVER {
        true => hi,
        false => hi + (1.0 + 2f64.powf(lo - hi)).log2(),
    }
}

/// 1 回ぶん。d² を落とすので、|d| が |2 Z d| に対して ε 以下に小さいときだけ
fn single(z: (f64, f64), eps: f64) -> Bla {
    let zabs = z.0.hypot(z.1);
    let p = match zabs == 0.0 {
        true => NEVER,
        false => (eps * zabs).log2(),
    };
    Bla { a: Cx::new(2.0 * z.0, 2.0 * z.1), b: Cx::new(1.0, 0.0), p, s: NEVER }
}

/// x の後に y を続けたもの。x を掛けた後の d が y の半径に収まる範囲まで:
///   r(c) = min(rx(c), (ry(c) - |Bx| c) / |Ax|)  ≥  min(px, py/|Ax|) - max(sx, (sy + |Bx|)/|Ax|) c
fn merge(x: Bla, y: Bla) -> Bla {
    let a = y.a.mul(x.a);
    let b = y.a.mul(x.b).add(y.b);
    let ax = x.a.log2_abs();
    if ax <= NEVER || x.p <= NEVER || y.p <= NEVER {
        return Bla { a, b, p: NEVER, s: NEVER };
    }
    let p = x.p.min(y.p - ax);
    let s = x.s.max(log2_add(y.s, x.b.log2_abs()) - ax);
    Bla { a, b, p, s }
}

/// 基準軌道から BLA の表を作り、shader が読む 1 本の配列にまとめる。
/// 並び: [M, L, off_0 … off_{L-1}, 軌道 (re, im) × M, 表 (A.re A.im A.e B.re B.im B.e p s) × …]。
/// 段 k の表は 2^k 回ぶんを飛ばすもので、i 番目が反復 i·2^k から。off_k はその段の先頭
fn bla_table(orbit: &[f64], eps: f64) -> Vec<f64> {
    let m = orbit.len() / 2;
    let mut levels: Vec<Vec<Bla>> = vec![(0..m).map(|i| single((orbit[2 * i], orbit[2 * i + 1]), eps)).collect()];
    while levels.last().is_some_and(|l| l.len() >= 2) {
        let prev = levels.last().expect("just checked");
        let next: Vec<Bla> = (0..prev.len() / 2).map(|i| merge(prev[2 * i], prev[2 * i + 1])).collect();
        levels.push(next);
    }
    let head = 2 + levels.len();
    let mut out = vec![0.0; head];
    out[0] = m as f64;
    out[1] = levels.len() as f64;
    out.extend_from_slice(orbit);
    for (k, level) in levels.iter().enumerate() {
        out[2 + k] = out.len() as f64;
        for b in level {
            out.extend([b.a.re, b.a.im, f64::from(b.a.e), b.b.re, b.b.im, f64::from(b.b.e), b.p, b.s]);
        }
    }
    out
}

/// 見えている範囲に入っているミニチュアの周期。c を中心にした半径 radius の球を回し、
/// 球が原点を含んだ最初の反復がそれ
fn ball_period(c: (&Fix, &Fix), radius: f64, max_period: usize) -> Option<usize> {
    let n = c.0.len();
    let mut z = (Fix::zero(n), Fix::zero(n));
    let mut r = 0.0f64;
    for p in 1..=max_period {
        let before = z.0.to_f64().hypot(z.1.to_f64());
        r = 2.0 * before * r + r * r + radius;
        z = step((&z.0, &z.1), c);
        let after = z.0.to_f64().hypot(z.1.to_f64());
        if after < r {
            return Some(p);
        }
        if after > 4.0 {
            return None;
        }
    }
    None
}

/// 周期 period の点 f^p(c) = 0 に、ニュートン法で寄せる。
/// 補正 z / dz は f64 で足りる (小さいが相対の桁は 16 で十分)。足すところだけ多倍長
fn newton(c: (Fix, Fix), period: usize, digits: usize, rounds: usize) -> Result<(Fix, Fix)> {
    let n = c.0.len();
    let (mut cre, mut cim) = c;
    let close = 10f64.powi(-(digits as i32) - 2);
    for _ in 0..rounds {
        let mut z = (Fix::zero(n), Fix::zero(n));
        let (mut dre, mut dim) = (0.0f64, 0.0f64);
        for _ in 0..period {
            let (zr, zi) = (z.0.to_f64(), z.1.to_f64());
            (dre, dim) = (2.0 * (zr * dre - zi * dim) + 1.0, 2.0 * (zr * dim + zi * dre));
            z = step((&z.0, &z.1), (&cre, &cim));
        }
        let (zr, zi) = (z.0.to_f64(), z.1.to_f64());
        let den = dre * dre + dim * dim;
        if den == 0.0 || !den.is_finite() {
            return err(Kind::OutOfRange, "find_center: the derivative overflowed; this point is deeper than f64 can hold");
        }
        let (sr, si) = ((zr * dre + zi * dim) / den, (zi * dre - zr * dim) / den);
        if !sr.is_finite() || !si.is_finite() {
            return err(Kind::OutOfRange, "find_center: Newton's method diverged");
        }
        cre = cre.sub(&Fix::from_f64(sr, n));
        cim = cim.sub(&Fix::from_f64(si, n));
        if sr.hypot(si) < close {
            break;
        }
    }
    Ok((cre, cim))
}

/// 中心 c、周期 p のミニチュアの大きさ (心臓形の幅。全体の集合なら 1)。
/// f^p を 0 の近くで 2 次式に見立てたときの縮尺 1 / (a·d)。a は z の 2 階微分の半分、d は c の 1 階微分で、
/// どちらも軌道の積 L_k = z_1 … z_k で書ける: a·d = L² Σ_{k=1..p} 2^(2p-1-k) / L_(k-1)
fn size_of(c: (&Fix, &Fix), period: usize) -> Cx {
    let n = c.0.len();
    let mut z = (Fix::zero(n), Fix::zero(n));
    let (mut l, mut s) = (Cx::new(1.0, 0.0), Cx::new(0.0, 0.0));
    for k in 1..=period {
        s = s.add(Cx::pow2((2 * period - 1 - k) as i32).mul(l.inv()));
        if k == period {
            break;
        }
        z = step((&z.0, &z.1), c);
        l = l.mul(Cx::new(z.0.to_f64(), z.1.to_f64()));
    }
    l.mul(l).mul(s).inv()
}

fn find_center(re: &str, im: &str, radius: f64, digits: usize, max_period: usize) -> Result<(String, String, usize, f64)> {
    let n = limbs_for(digits);
    let c = (Fix::parse(re, n)?, Fix::parse(im, n)?);
    let Some(period) = ball_period((&c.0, &c.1), radius, max_period) else {
        return err(Kind::OutOfRange, format!("find_center: no minibrot of period up to {max_period} within {radius} of that point"));
    };
    let (cre, cim) = newton(c, period, digits, 40)?;
    let size = size_of((&cre, &cim), period).abs();
    Ok((cre.to_decimal(digits), cim.to_decimal(digits), period, size))
}

/// 中心が周期点 (ミニチュアの中心) なら、その周期。軌道が最初に 0 に戻る m。戻らなければ 0
fn nucleus_period(orbit: &[f64]) -> usize {
    (1..orbit.len() / 2).find(|&m| orbit[2 * m].hypot(orbit[2 * m + 1]) < 1e-30).unwrap_or(0)
}

pub const DOCS: &[crate::docs::Entry] = &[
    crate::docs::Entry {
        name: "nucleus_period",
        signature: "bignum.nucleus_period(orbit: Array)",
        returns: "Number",
        doc: "基準軌道の中心が周期点 (ミニチュアの中心) なら、その周期。軌道が最初に 0 に戻る反復。戻らなければ 0",
    },
    crate::docs::Entry {
        name: "reference_orbit",
        signature: "bignum.reference_orbit(re: String, im: String, digits: Number, steps: Number)",
        returns: "Array",
        doc: "中心の点の軌道を、指定した 10 進の桁数で回して [re, im, re, im, …] の Array で返す。深いズームの基準軌道。脱出したらそこで止まる",
    },
    crate::docs::Entry {
        name: "bla_table",
        signature: "bignum.bla_table(orbit: Array, eps: Number)",
        returns: "Array",
        doc: "基準軌道から、反復をまとめて飛ばす表 (BLA) を作る。eps は許す誤差 (f32 なら 2^-24)。軌道と表を 1 本にした Array を返す (Shader の args に渡す)",
    },
    crate::docs::Entry {
        name: "find_center",
        signature: "bignum.find_center(re: String, im: String, radius: Number, digits: Number, max_period: Number)",
        returns: "(String, String, Number, Number)",
        doc: "その点から radius の中にあるミニチュアの中心を、周期を球で探してからニュートン法で求める。(re, im, 周期, 大きさ) を返す。re と im は桁数ぶんの文字列。大きさは心臓形の幅の目安 (全体の集合なら 1) で、span をその 4 倍にするとミニチュアが枝まで収まる",
    },
];

/// `import bignum` で束縛されるもの
pub fn module() -> Module {
    let mut items = HashMap::new();
    for f in ["reference_orbit", "bla_table", "find_center", "nucleus_period"] {
        items.insert(f.into(), Value::Builtin(name_of(f)));
    }
    Module { name: "bignum".into(), items }
}

fn name_of(f: &str) -> &'static str {
    match f {
        "reference_orbit" => "bignum.reference_orbit",
        "bla_table" => "bignum.bla_table",
        "nucleus_period" => "bignum.nucleus_period",
        "find_center" => "bignum.find_center",
        other => panic!("bignum に {other} は無い"),
    }
}

fn array(items: Vec<f64>) -> Value {
    Value::Array(std::rc::Rc::new(crate::lang::value::Array::new(items)))
}

/// 基準軌道の長さの上限。軌道と BLA 表を合わせた配列の添字が f32 で正確に表せる 2^24 に収まる長さ
const MAX_STEPS: f64 = 900_000.0;

/// `bignum.…` の呼び出し。`f` は前置きを外したもの
pub fn call(f: &str, values: &[Value]) -> Result<Value> {
    let name = &format!("bignum.{f}")[..];
    let whole = |what: &str, v: f64, top: f64| match v >= 1.0 && v.fract() == 0.0 && v <= top {
        true => Ok(v as usize),
        false => err(Kind::OutOfRange, format!("{name}: {what} must be a whole number between 1 and {top}, found {v}")),
    };
    match (f, values) {
        ("reference_orbit", [Value::Str(re), Value::Str(im), Value::Number(digits, _), Value::Number(steps, _)]) => {
            Ok(array(reference_orbit(re, im, whole("digits", *digits, 5000.0)?, whole("steps", *steps, MAX_STEPS)?)?))
        }
        ("reference_orbit", _) => err(Kind::ArgumentType, format!("{name} takes (re: String, im: String, digits: Number, steps: Number)")),
        ("bla_table", [orbit @ (Value::Array(_) | Value::List(_)), Value::Number(eps, _)]) => {
            if *eps <= 0.0 {
                return err(Kind::OutOfRange, format!("{name}: eps must be positive"));
            }
            let table = match orbit {
                Value::Array(a) => bla_table(&a.nums, *eps),
                Value::List(items) => {
                    let orbit = items
                        .borrow()
                        .iter()
                        .map(|v| match v {
                            Value::Number(x, _) => Ok(*x),
                            v => err(Kind::ArgumentType, format!("{name}: orbit must hold Numbers, found {}", v.type_name())),
                        })
                        .collect::<Result<Vec<f64>>>()?;
                    bla_table(&orbit, *eps)
                }
                _ => unreachable!(),
            };
            Ok(array(table))
        }
        ("bla_table", _) => err(Kind::ArgumentType, format!("{name} takes (orbit: Array, eps: Number)")),
        ("nucleus_period", [Value::Array(orbit)]) => Ok(Value::num(nucleus_period(&orbit.nums) as f64)),
        ("nucleus_period", _) => err(Kind::ArgumentType, format!("{name} takes (orbit: Array)")),
        ("find_center", [Value::Str(re), Value::Str(im), Value::Number(radius, _), Value::Number(digits, _), Value::Number(max_period, _)]) => {
            if *radius <= 0.0 {
                return err(Kind::OutOfRange, format!("{name}: radius must be positive"));
            }
            let (re, im, period, size) = find_center(re, im, *radius, whole("digits", *digits, 5000.0)?, whole("max_period", *max_period, 10_000_000.0)?)?;
            Ok(Value::Tuple(vec![Value::Str(re), Value::Str(im), Value::num(period as f64), Value::num(size)]))
        }
        ("find_center", _) => {
            err(Kind::ArgumentType, format!("{name} takes (re: String, im: String, radius: Number, digits: Number, max_period: Number)"))
        }
        (other, _) => err(Kind::UndefinedVariable, format!("bignum has no \"{other}\"")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(text: &str) -> Fix {
        Fix::parse(text, 4).expect("parses")
    }

    #[test]
    fn reads_and_writes_decimals() {
        assert!((fix("0.5").to_f64() - 0.5).abs() < 1e-18);
        assert!((fix("-1.25").to_f64() + 1.25).abs() < 1e-18);
        assert!((fix("2").to_f64() - 2.0).abs() < 1e-18);
        assert!((fix("-0.743643887037158704752191506114774").to_f64() + 0.7436438870371587).abs() < 1e-15);
        assert_eq!(fix("-0.743643887037158704752191506114774").to_decimal(30), "-0.743643887037158704752191506115");
        assert_eq!(fix("3.5").to_decimal(3), "3.500");
    }

    #[test]
    fn adds_and_multiplies_like_f64() {
        let cases = [("0.25", "0.5"), ("-0.75", "0.125"), ("1.5", "-0.5"), ("-0.3", "-0.7")];
        for (a, b) in cases {
            let (x, y) = (fix(a), fix(b));
            let (fa, fb) = (a.parse::<f64>().expect("f64"), b.parse::<f64>().expect("f64"));
            assert!((x.add(&y).to_f64() - (fa + fb)).abs() < 1e-15, "{a} + {b}");
            assert!((x.sub(&y).to_f64() - (fa - fb)).abs() < 1e-15, "{a} - {b}");
            assert!((x.mul(&y).to_f64() - fa * fb).abs() < 1e-15, "{a} * {b}");
        }
    }

    #[test]
    fn goes_through_f64_and_back() {
        for x in [0.0, 1.0, -1.0, 0.5, 1e-10, -3.25e-20, 1234.5, 1e-300] {
            let back = Fix::from_f64(x, 20).to_f64();
            assert!((back - x).abs() <= x.abs() * 1e-15, "{x} -> {back}");
        }
        // 小さい値も limb の数だけ細かく持てる
        let tiny = Fix::from_f64(1e-40, 6);
        assert!(!tiny.is_zero());
        assert!((tiny.to_f64() - 1e-40).abs() < 1e-55);
    }

    /// 浅い中心なら、f64 でそのまま回した軌道と一致する
    #[test]
    fn matches_a_plain_f64_orbit() {
        let (cre, cim) = (-0.5, 0.5);
        let orbit = reference_orbit("-0.5", "0.5", 30, 20).expect("orbit");
        let (mut zre, mut zim) = (0.0, 0.0);
        for n in 0..orbit.len() / 2 {
            assert!((orbit[n * 2] - zre).abs() < 1e-12, "re at {n}");
            assert!((orbit[n * 2 + 1] - zim).abs() < 1e-12, "im at {n}");
            (zre, zim) = (zre * zre - zim * zim + cre, 2.0 * zre * zim + cim);
        }
    }

    /// 脱出する中心は、そこで止まる
    #[test]
    fn stops_when_it_escapes() {
        let orbit = reference_orbit("1", "1", 20, 100).expect("orbit");
        assert!(orbit.len() / 2 < 10, "escapes early, got {}", orbit.len() / 2);
    }

    /// BLA で 16 回まとめて飛ばした結果が、1 回ずつ回したものと合う
    #[test]
    fn bla_skips_like_stepping() {
        let orbit = reference_orbit("-1.2", "0.05", 20, 64).expect("orbit");
        let table = bla_table(&orbit, 2f64.powi(-24));
        let l = table[1] as usize;
        assert!(l >= 5, "levels {l}");
        // 段 4 (16 回) の 1 番目 (反復 16 から)。0 番目は Z_0 = 0 を含むので使えない
        let at = table[2 + 4] as usize + 8;
        let a = Cx { re: table[at], im: table[at + 1], e: table[at + 2] as i32 };
        let b = Cx { re: table[at + 3], im: table[at + 4], e: table[at + 5] as i32 };
        let (p, s) = (table[at + 6], table[at + 7]);
        let (d0, c): ((f64, f64), (f64, f64)) = ((1e-22, 2e-22), (3e-23, -1e-23));
        // 有効半径 2^p - 2^s |c| の中にあること
        let radius = 2f64.powf(p) - 2f64.powf(s) * c.0.hypot(c.1);
        assert!(d0.0.hypot(d0.1) < radius, "d must be inside the radius {radius}");
        // 1 回ずつ: d' = 2 Z d + d² + c
        let (mut dr, mut di) = d0;
        for n in 16..32 {
            let (zr, zi) = (orbit[2 * n], orbit[2 * n + 1]);
            (dr, di) = (2.0 * (zr * dr - zi * di) + (dr * dr - di * di) + c.0, 2.0 * (zr * di + zi * dr) + 2.0 * dr * di + c.1);
        }
        let skipped = a.mul(Cx::new(d0.0, d0.1)).add(b.mul(Cx::new(c.0, c.1)));
        let f = 2f64.powi(skipped.e);
        let (sr, si) = (skipped.re * f, skipped.im * f);
        let scale = dr.hypot(di);
        assert!((sr - dr).abs() < scale * 1e-6 && (si - di).abs() < scale * 1e-6, "stepped ({dr}, {di}) skipped ({sr}, {si})");
    }

    /// shader と同じ手順 (仮数と指数、表の段を上から試す、rebase) を f64 で回し、
    /// 1 歩ずつ回した脱出回数と合うか。表の当て方の間違いはここで出る
    #[test]
    fn deep_kernel_matches_stepping() {
        compare_kernels("-0.7436438870371587", "0.1318259042053120", 20, 3000, 3000, &[(1e-8, 2e-8), (-3e-9, 1e-8), (5e-12, -5e-12), (2e-10, 0.0)]);
    }

    /// 深いところ (1e15 相当) でも合うこと
    #[test]
    fn deep_kernel_matches_stepping_far_in() {
        let (re, im) = ("-0.743643887037158704752191506114774", "0.131825904205311970493132056385139");
        compare_kernels(re, im, 40, 100_000, 20_000, &[(6e-18, 3e-18), (-2e-18, 5e-18), (1e-19, 0.0), (4e-18, -4e-18)]);
    }

    fn compare_kernels(re: &str, im: &str, digits: usize, steps: usize, limit: usize, cs: &[(f64, f64)]) {
        let orbit = reference_orbit(re, im, digits, steps).expect("orbit");
        let table = bla_table(&orbit, 2f64.powi(-24));
        let m_len = table[0] as usize;
        let levels = table[1] as usize;
        let ob = 2 + levels;
        let z_at = |m: usize| (table[ob + 2 * m], table[ob + 2 * m + 1]);
        let escape = 256.0;
        // 1 歩ずつ (rebase あり)
        let plain = |c: (f64, f64)| -> Option<usize> {
            let (mut dr, mut di) = (0.0, 0.0);
            let mut m = 0;
            for n in 0..limit {
                let (zr, zi) = z_at(m);
                let (xr, xi) = (zr + dr, zi + di);
                let r2 = xr * xr + xi * xi;
                if r2 > escape {
                    return Some(n);
                }
                if r2 < dr * dr + di * di || m + 1 >= m_len {
                    (dr, di, m) = (xr, xi, 0);
                }
                let (zr, zi) = z_at(m);
                (dr, di) = (2.0 * (zr * dr - zi * di) + (dr * dr - di * di) + c.0, 2.0 * (zr * di + zi * dr) + 2.0 * dr * di + c.1);
                m += 1;
            }
            None
        };
        // shader の手順
        let deep = |c: (f64, f64)| -> Option<usize> {
            let cx = Cx::new(c.0, c.1);
            let lc = cx.log2_abs();
            let mut d = Cx { re: 0.0, im: 0.0, e: 0 };
            let (mut m, mut n) = (0usize, 0usize);
            while n < limit {
                let (zr, zi) = z_at(m);
                let f = 2f64.powi(d.e);
                let (xr, xi) = (zr + d.re * f, zi + d.im * f);
                let r2 = xr * xr + xi * xi;
                if r2 > escape {
                    return Some(n);
                }
                let mut ld = d.log2_abs();
                if 0.5 * r2.log2() < ld || m + 1 >= m_len {
                    d = Cx::new(xr, xi);
                    m = 0;
                    ld = d.log2_abs();
                }
                let mut took = false;
                for k in (0..levels).rev() {
                    let hop = 1usize << k;
                    if m % hop != 0 || m + hop >= m_len {
                        continue;
                    }
                    let at = table[2 + k] as usize + 8 * (m / hop);
                    let (p, s) = (table[at + 6], table[at + 7]);
                    let bias = s + lc - p;
                    if bias >= 0.0 {
                        continue;
                    }
                    if ld < p + (1.0 - 2f64.powf(bias)).log2() {
                        let a = Cx { re: table[at], im: table[at + 1], e: table[at + 2] as i32 };
                        let b = Cx { re: table[at + 3], im: table[at + 4], e: table[at + 5] as i32 };
                        d = a.mul(d).add(b.mul(cx));
                        m += hop;
                        n += hop;
                        took = true;
                        break;
                    }
                }
                if !took {
                    let (zr, zi) = z_at(m);
                    let z = Cx::new(2.0 * zr, 2.0 * zi);
                    d = z.mul(d).add(d.mul(d)).add(cx);
                    m += 1;
                    n += 1;
                }
            }
            None
        };
        for &c in cs {
            let (a, b) = (plain(c), deep(c));
            let close = match (a, b) {
                (Some(x), Some(y)) => x.abs_diff(y) <= 2.max(x / 100),
                (None, None) => true,
                _ => false,
            };
            assert!(close, "c = {c:?}: stepping {a:?}, skipping {b:?}");
        }
    }

    /// 表を当てる手順の f64 版 (Cx で毎回正規化する)
    fn deep_cx(table: &[f64], c: (f64, f64), limit: usize, escape: f64) -> Option<usize> {
        deep_cx_trace(table, c, limit, escape, &mut Vec::new())
    }

    fn deep_cx_trace(table: &[f64], c: (f64, f64), limit: usize, escape: f64, trace: &mut Vec<(f64, f64, f64, f64)>) -> Option<usize> {
        let m_len = table[0] as usize;
        let levels = table[1] as usize;
        let ob = 2 + levels;
        let z_at = |m: usize| (table[ob + 2 * m], table[ob + 2 * m + 1]);
        let cx = Cx::new(c.0, c.1);
        let lc = cx.log2_abs();
        let mut d = Cx { re: 0.0, im: 0.0, e: 0 };
        let (mut m, mut n) = (0usize, 0usize);
        while n < limit {
            let (zr, zi) = z_at(m);
            let f = 2f64.powi(d.e);
            let (xr, xi) = (zr + d.re * f, zi + d.im * f);
            let r2 = xr * xr + xi * xi;
            if r2 > escape {
                return Some(n);
            }
            let mut ld = d.log2_abs();
            trace.push((n as f64, m as f64, ld, r2));
            if 0.5 * r2.log2() < ld || m + 1 >= m_len {
                d = Cx::new(xr, xi);
                m = 0;
                ld = d.log2_abs();
            }
            let mut took = false;
            for k in (0..levels).rev() {
                let hop = 1usize << k;
                if m % hop != 0 || m + hop >= m_len {
                    continue;
                }
                let at = table[2 + k] as usize + 8 * (m / hop);
                let (p, s) = (table[at + 6], table[at + 7]);
                let bias = s + lc - p;
                if bias >= 0.0 {
                    continue;
                }
                if ld < p + (1.0 - 2f64.powf(bias)).log2() {
                    let a = Cx { re: table[at], im: table[at + 1], e: table[at + 2] as i32 };
                    let b = Cx { re: table[at + 3], im: table[at + 4], e: table[at + 5] as i32 };
                    d = a.mul(d).add(b.mul(cx));
                    m += hop;
                    n += hop;
                    took = true;
                    break;
                }
            }
            if !took {
                let (zr, zi) = z_at(m);
                let z = Cx::new(2.0 * zr, 2.0 * zi);
                d = z.mul(d).add(d.mul(d)).add(cx);
                m += 1;
                n += 1;
            }
        }
        None
    }

    /// 1 歩ずつ (BLA も内側判定も無し) の f64 で、1e30 の見え方を ASCII で出す。GPU の絵の答え合わせ用
    #[test]
    #[ignore]
    fn ascii_1e30() {
        let (re, im) = ("-0.743643887037158704752191506114774", "0.131825904205311970493132056385139");
        let orbit = reference_orbit(re, im, 40, 100_000).expect("orbit");
        let m_len = orbit.len() / 2;
        let z_at = |m: usize| (orbit[2 * m], orbit[2 * m + 1]);
        let plain = |c: (f64, f64), limit: usize| -> Option<usize> {
            let (mut dr, mut di) = (0.0, 0.0);
            let mut m = 0;
            for n in 0..limit {
                let (zr, zi) = z_at(m);
                let (xr, xi) = (zr + dr, zi + di);
                let r2 = xr * xr + xi * xi;
                if r2 > 256.0 {
                    return Some(n);
                }
                if r2 < dr * dr + di * di || m + 1 >= m_len {
                    (dr, di, m) = (xr, xi, 0);
                }
                let (zr, zi) = z_at(m);
                (dr, di) = (2.0 * (zr * dr - zi * di) + (dr * dr - di * di) + c.0, 2.0 * (zr * di + zi * dr) + 2.0 * dr * di + c.1);
                m += 1;
            }
            None
        };
        let width = 4.0 / 10f64.powf(29.5);
        let (cols, rows) = (64, 36);
        for row in 0..rows {
            let mut line = String::new();
            for col in 0..cols {
                let c = ((col as f64 / cols as f64 - 0.5) * width, -(row as f64 / rows as f64 - 0.5) * width * 9.0 / 16.0);
                line.push(match plain(c, 100_000) {
                    None => '#',
                    Some(n) if n < 20_000 => '.',
                    Some(_) => ':',
                });
            }
            eprintln!("{line}");
        }
    }

    /// 1e30 の左半分 (θ ≈ 180°) の画素。GPU では黒くなる
    #[test]
    #[ignore]
    fn probe_left_1e30() {
        let (re, im) = ("-0.743643887037158704752191506114774", "0.131825904205311970493132056385139");
        let orbit = reference_orbit(re, im, 40, 100_000).expect("orbit");
        let table64 = bla_table(&orbit, 2f64.powi(-24));
        let table32: Vec<f32> = table64.iter().map(|x| *x as f32).collect();
        for c in [(-6.0e-30f64, 0.0f64), (-4.0e-30, 2.0e-30), (-4.0e-30, -2.0e-30), (-1.0e-30, 3.0e-30), (6.0e-30, 0.0), (0.0, 3.0e-30)] {
            let a = deep_cx(&table64, c, 40_000, 256.0);
            let b = deep_f32(&table32, (c.0 as f32, c.1 as f32), 40_000, 256.0);
            eprintln!("c = {c:?}: f64 {a:?}  f32 {b:?}");
        }
    }

    #[test]
    #[ignore]
    fn probe_depth_1e30() {
        let (re, im) = ("-0.743643887037158704752191506114774", "0.131825904205311970493132056385139");
        let orbit = reference_orbit(re, im, 40, 100_000).expect("orbit");
        let table64 = bla_table(&orbit, 2f64.powi(-24));
        for c in [(2.0e-30f64, 1.0e-30f64), (-1.5e-30, 0.5e-30), (0.0, 2.0e-30), (2.0e-27, 1.0e-27), (2.0e-28, 0.0)] {
            let a = deep_cx(&table64, c, 20_000, 256.0);
            let b = deep_cx(&table64, c, 400_000, 256.0);
            eprintln!("c = {c:?}: limit 20000 -> {a:?}, limit 400000 -> {b:?}");
        }
    }

    #[test]
    #[ignore]
    fn trace_divergence() {
        let (re, im) = ("-0.743643887037158704752191506114774", "0.131825904205311970493132056385139");
        let orbit = reference_orbit(re, im, 40, 100_000).expect("orbit");
        let table64 = bla_table(&orbit, 2f64.powi(-24));
        let table32: Vec<f32> = table64.iter().map(|x| *x as f32).collect();
        let c = (-6.0e-30f64, 0.0f64);
        let (mut t64, mut t32) = (Vec::new(), Vec::new());
        let n64 = deep_cx_trace(&table64, c, 20_000, 256.0, &mut t64);
        let n32 = deep_f32_trace(&table32, (c.0 as f32, c.1 as f32), 20_000, 256.0, 0, &mut t32);
        eprintln!("f64 {n64:?} ({} rows)   f32 {n32:?} ({} rows)", t64.len(), t32.len());
        let mut shown = 0;
        for i in 0..t64.len().min(t32.len()) {
            let (a, b) = (t64[i], t32[i]);
            let differs = a.0 != f64::from(b.0) || a.1 != f64::from(b.1) || (a.2 - f64::from(b.2)).abs() > 0.5;
            if differs || i < 3 {
                eprintln!("{i:>5}: f64 n {:>7} m {:>7} log2|d| {:>9.2} |z|² {:.4}   f32 n {:>7} m {:>7} log2|d| {:>9.2} |z|² {:.4}", a.0, a.1, a.2, a.3, b.0, b.1, b.2, b.3);
                shown += 1;
                if shown > 12 { break; }
            }
        }
    }

    /// WGSL を f32 で 1 行ずつ写したもの。GPU で起きることはここでも起きる
    fn deep_f32(table: &[f32], c: (f32, f32), limit: usize, escape: f32) -> Option<usize> {
        deep_f32_trace(table, c, limit, escape, 0, &mut Vec::new())
    }

    /// period は中心の周期 (0 なら中の点の打ち切りをしない)。打ち切ったときも None
    fn deep_f32_trace(table: &[f32], c: (f32, f32), limit: usize, escape: f32, period: usize, trace: &mut Vec<(f32, f32, f32, f32)>) -> Option<usize> {
        let never = -1e30f32;
        let log2_len = |v: (f32, f32)| {
            let a = v.0.abs().max(v.1.abs());
            if a == 0.0 {
                never
            } else {
                let b = v.0.abs().min(v.1.abs()) / a;
                a.log2() + 0.5 * (1.0 + b * b).log2()
            }
        };
        let cmul = |a: (f32, f32), b: (f32, f32)| (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0);
        let m_len = table[0];
        let levels = table[1] as usize;
        let ob = 2.0 + table[1];
        let at = |x: f32| table[x as u32 as usize];
        let (window, window_inv) = (2f32.powi(24), 2f32.powi(-24));
        // 画素の差を (仮数, 指数) に
        let lc = c.0.hypot(c.1).log2();
        let ce = lc.floor();
        let cm = (c.0 * (-ce).exp2(), c.1 * (-ce).exp2());
        let (mut dm, mut de) = ((0.0f32, 0.0f32), 0.0f32);
        let (mut s1, mut s2) = (1.0f32, ce.exp2());
        let (mut m, mut n, mut skip) = (0.0f32, 0.0f32, 0u32);
        let period = period as f32;
        let (mut pm, mut pe, mut ldiff, mut hits) = ((0.0f32, 0.0f32), 0.0f32, never, 0);
        for _ in 0..limit {
            if n >= limit as f32 {
                break;
            }
            let mut zz = (at(ob + 2.0 * m), at(ob + 2.0 * m + 1.0));
            let z = (zz.0 + dm.0 * s1, zz.1 + dm.1 * s1);
            let r2 = z.0 * z.0 + z.1 * z.1;
            if r2 > escape {
                return Some(n as usize);
            }
            trace.push((n, m, de + log2_len(dm), r2));
            if r2 < (dm.0 * dm.0 + dm.1 * dm.1) * s1 * s1 || m + 1.0 >= m_len {
                dm = z;
                de = 0.0;
                m = 0.0;
                skip = 0;
                zz = (at(ob), at(ob + 1.0));
                if dm.0 != 0.0 || dm.1 != 0.0 {
                    let shift = log2_len(dm).floor();
                    dm = (dm.0 * (-shift).exp2(), dm.1 * (-shift).exp2());
                    de = shift;
                }
                s1 = de.exp2();
                s2 = (ce - de).exp2();
            }
            let mut best = None;
            let mut hop = 1.0f32;
            if skip > 0 {
                skip -= 1;
            } else {
                let ld = if dm.0 == 0.0 && dm.1 == 0.0 { never } else { de + 0.5 * (dm.0 * dm.0 + dm.1 * dm.1).log2() };
                for k in 0..levels {
                    if (m / hop).floor() * hop != m || m + hop >= m_len {
                        break;
                    }
                    let a = at(2.0 + k as f32) + 8.0 * (m / hop).floor();
                    let p = at(a + 6.0);
                    if ld >= p {
                        break;
                    }
                    let bias = at(a + 7.0) + lc - p;
                    if bias >= 0.0 || (bias <= -1.0 && ld >= p - 1.0) || (bias > -1.0 && ld >= p + (1.0 - bias.exp2()).log2()) {
                        break;
                    }
                    best = Some((k as f32, hop));
                    hop *= 2.0;
                }
                if best.is_none() {
                    skip = SCAN_REST;
                }
            }
            match best {
                Some((k, hop)) => {
                    let a = at(2.0 + k) + 8.0 * (m / hop).floor();
                    let e2 = at(a + 5.0) + ce;
                    let t2 = cmul((at(a + 3.0), at(a + 4.0)), cm);
                    if dm.0 == 0.0 && dm.1 == 0.0 {
                        (dm, de) = (t2, e2);
                    } else {
                        let e1 = at(a + 2.0) + de;
                        let big = e1.max(e2);
                        let t1 = cmul((at(a), at(a + 1.0)), dm);
                        let (f1, f2) = ((e1 - big).exp2(), (e2 - big).exp2());
                        dm = (t1.0 * f1 + t2.0 * f2, t1.1 * f1 + t2.1 * f2);
                        de = big;
                    }
                    m += hop;
                    n += hop;
                    s1 = de.exp2();
                    s2 = (ce - de).exp2();
                }
                None => {
                    if dm.0 == 0.0 && dm.1 == 0.0 {
                        (dm, de) = (cm, ce);
                        s1 = ce.exp2();
                        s2 = 1.0;
                    } else {
                        let (t1, t2) = (cmul(zz, dm), cmul(dm, dm));
                        dm = (2.0 * t1.0 + t2.0 * s1 + cm.0 * s2, 2.0 * t1.1 + t2.1 * s1 + cm.1 * s2);
                    }
                    m += 1.0;
                    n += 1.0;
                    // 周期点の次の歩。前の周期の同じ歩との差が 4 回続けて 0.5 bit 以上縮むなら、周期点に引き込まれている
                    if period > 0.0 && m - period * (m / period).floor() == 1.0 {
                        let e = de.max(pe);
                        let (fd, fp) = ((de - e).exp2(), (pe - e).exp2());
                        let diff = (dm.0 * fd - pm.0 * fp, dm.1 * fd - pm.1 * fp);
                        let ld2 = e + log2_len(diff);
                        hits = if ld2 < ldiff - 0.5 { hits + 1 } else { 0 };
                        if hits >= 4 {
                            return None;
                        }
                        (ldiff, pm, pe) = (ld2, dm, de);
                    }
                }
            }
            // 仮数が窓を外れたら指数を動かす
            let a = dm.0.abs().max(dm.1.abs());
            if a > window || (a < window_inv && a > 0.0) {
                let shift = a.log2().floor();
                dm = (dm.0 * (-shift).exp2(), dm.1 * (-shift).exp2());
                de += shift;
                s1 = de.exp2();
                s2 = (ce - de).exp2();
            }
        }
        None
    }

    #[test]
    #[ignore]
    fn trace_f32_kernel() {
        let (re, im) = ("-0.743643887037158704752191506114774", "0.131825904205311970493132056385139");
        let orbit = reference_orbit(re, im, 40, 100_000).expect("orbit");
        let table: Vec<f32> = bla_table(&orbit, 2f64.powi(-24)).into_iter().map(|x| x as f32).collect();
        let mut trace = Vec::new();
        let n = deep_f32_trace(&table, (1e-19, 0.0), 20_000, 256.0, 0, &mut trace);
        eprintln!("escaped: {n:?}, steps taken: {}, orbit len {}", trace.len(), orbit.len() / 2);
        for (i, row) in trace.iter().enumerate() {
            if i < 40 || i % 500 == 0 || i + 20 > trace.len() {
                eprintln!("{i:>6}: n {:>8} m {:>8} log2|d| {:>10.2} |z|^2 {:.4}", row.0, row.1, row.2, row.3);
            }
        }
    }

    #[test]
    fn f32_kernel_escapes_like_f64() {
        let (re, im) = ("-0.743643887037158704752191506114774", "0.131825904205311970493132056385139");
        let orbit = reference_orbit(re, im, 40, 100_000).expect("orbit");
        let table: Vec<f32> = bla_table(&orbit, 2f64.powi(-24)).into_iter().map(|x| x as f32).collect();
        // 1e15 のコマの画素 (|c| ≈ 1e-15)。f64 の手順が脱出するものは f32 も脱出すること
        let table64 = bla_table(&orbit, 2f64.powi(-24));
        for c in [(1.0e-15f32, 0.5e-15f32), (-2.0e-15, 1.0e-15), (1.5e-15, -1.5e-15), (0.0, 1.8e-15), (-1.0e-15, -0.3e-15)] {
            let n32 = deep_f32(&table, c, 20_000, 256.0);
            let n64 = deep_cx(&table64, (f64::from(c.0), f64::from(c.1)), 20_000, 256.0);
            eprintln!("c = {c:?}: f64 {n64:?}  f32 {n32:?}");
            assert_eq!(n32.is_some(), n64.is_some(), "c = {c:?}: f64 {n64:?}, f32 {n32:?}");
        }
    }

    /// fractal.moph の SCAN_REST と同じ。段 0 で駄目だったあと判定を休む反復の数
    const SCAN_REST: u32 = 32;

    /// fractal.moph の浅い側 (差分を f32 のまま持ち、表で飛ぶ) を f32 で 1 行ずつ写したもの
    fn shallow_f32(table: &[f32], c: (f32, f32), limit: usize, escape: f32) -> Option<usize> {
        let m_len = table[0] as usize;
        let levels = table[1] as usize;
        let ob = 2 + levels;
        let at = |i: usize| table[i];
        let cmul = |a: (f32, f32), b: (f32, f32)| (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0);
        let lc = c.0.hypot(c.1).log2();
        let (mut d, mut m, mut n, mut skip) = ((0.0f32, 0.0f32), 0usize, 0usize, 0u32);
        while n < limit {
            let mut z_ref = (at(ob + 2 * m), at(ob + 2 * m + 1));
            let z = (z_ref.0 + d.0, z_ref.1 + d.1);
            let r2 = z.0 * z.0 + z.1 * z.1;
            if r2 > escape {
                return Some(n);
            }
            let mut d2 = d.0 * d.0 + d.1 * d.1;
            if r2 < d2 || m + 1 >= m_len {
                d = z;
                d2 = r2;
                m = 0;
                skip = 0;
                z_ref = (at(ob), at(ob + 1));
            }
            let mut best = None;
            let mut hop = 1usize;
            if skip > 0 {
                skip -= 1;
            } else {
                let ld = if d2 == 0.0 { -1e30 } else { 0.5 * d2.log2() };
                for k in 0..levels {
                    if m % hop != 0 || m + hop >= m_len {
                        break;
                    }
                    let e = at(2 + k) as usize + 8 * (m / hop);
                    let p = at(e + 6);
                    if ld >= p {
                        break;
                    }
                    let bias = at(e + 7) + lc - p;
                    if bias >= 0.0 || (bias <= -1.0 && ld >= p - 1.0) || (bias > -1.0 && ld >= p + (1.0 - bias.exp2()).log2()) {
                        break;
                    }
                    best = Some((k, hop));
                    hop *= 2;
                }
                if best.is_none() {
                    skip = SCAN_REST;
                }
            }
            match best {
                Some((k, hop)) => {
                    let e = at(2 + k) as usize + 8 * (m / hop);
                    let a = ((at(e) * at(e + 2).exp2()), (at(e + 1) * at(e + 2).exp2()));
                    let b = ((at(e + 3) * at(e + 5).exp2()), (at(e + 4) * at(e + 5).exp2()));
                    let (ad, bc) = (cmul(a, d), cmul(b, c));
                    d = (ad.0 + bc.0, ad.1 + bc.1);
                    m += hop;
                    n += hop;
                }
                None => {
                    let (zd, dd) = (cmul(z_ref, d), cmul(d, d));
                    d = (2.0 * zd.0 + dd.0 + c.0, 2.0 * zd.1 + dd.1 + c.1);
                    m += 1;
                    n += 1;
                }
            }
        }
        None
    }

    /// 浅い側の f32 の版が、同じ f32 の表を使う深い側の版と同じ脱出回数になること (1e15 から 1e25 まで)。
    /// f64 の基準軌道とは、敏感な点で数 % ずれる (基準を f32 に丸めた分は両方の版に同じに乗る) ので、脱出の有無だけ比べる
    #[test]
    fn shallow_f32_kernel_matches_deep() {
        let (re, im) = ("-0.743643887037158704752191506114774", "0.131825904205311970493132056385139");
        let orbit = reference_orbit(re, im, 40, 100_000).expect("orbit");
        let table64 = bla_table(&orbit, 2f64.powi(-24));
        let table: Vec<f32> = table64.iter().map(|x| *x as f32).collect();
        for e in [15, 20, 25] {
            let s = 10f64.powi(-e);
            for (x, y) in [(1.0, 0.5), (-2.0, 1.0), (1.5, -1.5), (0.0, 1.8), (-1.0, -0.3), (0.7, 0.7)] {
                let c = (x * s, y * s);
                let c32 = (c.0 as f32, c.1 as f32);
                let n64 = deep_cx(&table64, c, 100_000, 256.0);
                let deep = deep_f32(&table, c32, 100_000, 256.0);
                let shallow = shallow_f32(&table, c32, 100_000, 256.0);
                assert_eq!(n64.is_some(), shallow.is_some(), "c = {c:?}: f64 {n64:?}, shallow {shallow:?}");
                // 敏感な点では飛び方の違いが 1% ほどの差になる
                let close = match (deep, shallow) {
                    (Some(a), Some(b)) => (a as i64 - b as i64).abs() <= 2.max(a as i64 / 50),
                    (None, None) => true,
                    _ => false,
                };
                assert!(close, "c = {c:?}: deep {deep:?}, shallow {shallow:?}");
            }
        }
    }

    /// 中心が周期点なら、ミニチュアの中の点は周期ごとの差分の縮みで数周期のうちに「中」と分かること。
    /// 外の点 (尖りのそば、|ĉ| がわずかに 0.25 を超える) は打ち切られないこと
    #[test]
    fn period_check_finds_the_inside() {
        let (re, im, p, _) = find_center("-0.743643887037158704752191506114774", "0.131825904205311970493132056385139", 2e-30, 60, 20_000).expect("center");
        let orbit = reference_orbit(&re, &im, 40, 100_000).expect("orbit");
        assert_eq!(nucleus_period(&orbit), p);
        let table: Vec<f32> = bla_table(&orbit, 2f64.powi(-24)).into_iter().map(|x| x as f32).collect();
        // 大きさは向きも持つ (複素数) ので、ミニチュアの座標 ĉ を画素の差に写せる
        let n = limbs_for(60);
        let c = (Fix::parse(&re, n).expect("re"), Fix::parse(&im, n).expect("im"));
        let size = size_of((&c.0, &c.1), p);
        let run = |x: f64, y: f64| {
            let f = 2f64.powi(size.e);
            let d = (((x * size.re - y * size.im) * f) as f32, ((x * size.im + y * size.re) * f) as f32);
            let mut trace = Vec::new();
            let escaped = deep_f32_trace(&table, d, 100_000, 256.0, p, &mut trace);
            (escaped, trace.last().map_or(0, |row| row.0 as usize))
        };
        // 心臓形の内側 (乗数 0.7 未満) は数周期で分かる
        for (x, y) in [(0.1, 0.0), (0.0, 0.2), (-0.3, 0.1), (-0.2, -0.3), (0.01, 0.0)] {
            let (escaped, last_n) = run(x, y);
            assert!(escaped.is_none() && last_n < 8 * p, "ĉ = ({x}, {y}): {escaped:?}, reached {last_n}");
        }
        // 尖りのそば、遠く、周期 2 の円のそばの外の点は打ち切られない
        for (x, y) in [(0.26, 0.0), (0.3, 0.05), (1.5, 0.0), (0.0, 1.5), (-0.8, 0.3)] {
            let (escaped, last_n) = run(x, y);
            assert!(escaped.is_some() || last_n >= 100_000 - 8192, "ĉ = ({x}, {y}): {escaped:?}, reached {last_n}");
        }
    }

    /// 1 周期 (8007 歩) を進むのに、カーネルのループが何回回るか。BLA の効き具合
    #[test]
    #[ignore]
    fn kernel_iterations_per_period() {
        let (re, im, p, _) = find_center("-0.743643887037158704752191506114774", "0.131825904205311970493132056385139", 2e-30, 60, 20_000).expect("center");
        let orbit = reference_orbit(&re, &im, 40, 100_000).expect("orbit");
        let table: Vec<f32> = bla_table(&orbit, 2f64.powi(-24)).into_iter().map(|x| x as f32).collect();
        let n = limbs_for(60);
        let c = (Fix::parse(&re, n).expect("re"), Fix::parse(&im, n).expect("im"));
        let size = size_of((&c.0, &c.1), p);
        for (x, y) in [(0.1, 0.0), (-0.3, 0.1), (0.6, 0.0), (0.26, 0.0), (1.0, 0.5)] {
            let f = 2f64.powi(size.e);
            let d = (((x * size.re - y * size.im) * f) as f32, ((x * size.im + y * size.re) * f) as f32);
            let mut trace = Vec::new();
            let escaped = deep_f32_trace(&table, d, 100_000, 256.0, 0, &mut trace);
            let reached = trace.last().map_or(0, |row| row.0 as usize);
            let hist: Vec<usize> = (0..=(reached / p)).map(|k| trace.iter().filter(|row| (row.0 as usize) / p == k).count()).collect();
            eprintln!("ĉ = ({x}, {y}): {escaped:?}, {} loop iterations for {} steps ({:.0}/period), per period {hist:?}", trace.len(), reached, trace.len() as f64 / (reached as f64 / p as f64));
        }
    }

    /// 素の摂動法 (差分を f32 で持つ、表なし) が何桁まで f64 と合うか。切り替える深さを決める材料
    #[test]
    #[ignore]
    fn plain_f32_depth_limit() {
        let (re, im) = ("-0.743643887037158704752191506114774", "0.131825904205311970493132056385139");
        let orbit = reference_orbit(re, im, 40, 100_000).expect("orbit");
        let orbit32: Vec<f32> = orbit.iter().map(|x| *x as f32).collect();
        let m_len = orbit.len() / 2;
        let plain64 = |c: (f64, f64), limit: usize| -> Option<usize> {
            let (mut dr, mut di, mut m) = (0.0f64, 0.0f64, 0);
            for n in 0..limit {
                let (zr, zi) = (orbit[2 * m], orbit[2 * m + 1]);
                let (xr, xi) = (zr + dr, zi + di);
                let r2 = xr * xr + xi * xi;
                if r2 > 256.0 {
                    return Some(n);
                }
                if r2 < dr * dr + di * di || m + 1 >= m_len {
                    (dr, di, m) = (xr, xi, 0);
                }
                let (zr, zi) = (orbit[2 * m], orbit[2 * m + 1]);
                (dr, di) = (2.0 * (zr * dr - zi * di) + (dr * dr - di * di) + c.0, 2.0 * (zr * di + zi * dr) + 2.0 * dr * di + c.1);
                m += 1;
            }
            None
        };
        let plain32 = |c: (f32, f32), limit: usize| -> Option<usize> {
            let (mut dr, mut di, mut m) = (0.0f32, 0.0f32, 0);
            for n in 0..limit {
                let (zr, zi) = (orbit32[2 * m], orbit32[2 * m + 1]);
                let (xr, xi) = (zr + dr, zi + di);
                let r2 = xr * xr + xi * xi;
                if r2 > 256.0 {
                    return Some(n);
                }
                if r2 < dr * dr + di * di || m + 1 >= m_len {
                    (dr, di, m) = (xr, xi, 0);
                }
                let (zr, zi) = (orbit32[2 * m], orbit32[2 * m + 1]);
                (dr, di) = (2.0 * (zr * dr - zi * di) + (dr * dr - di * di) + c.0, 2.0 * (zr * di + zi * dr) + 2.0 * dr * di + c.1);
                m += 1;
            }
            None
        };
        for e in [10, 13, 15, 17, 19, 21, 23, 25, 28, 30, 33] {
            let s = 10f64.powi(-e);
            let mut agree = 0;
            let mut rows = Vec::new();
            let cs = [(1.0, 0.5), (-2.0, 1.0), (1.5, -1.5), (0.0, 1.8), (-1.0, -0.3), (0.7, 0.7), (-1.6, -1.2), (2.0, 0.1)];
            for (x, y) in cs {
                let c = (x * s, y * s);
                let a = plain64(c, 100_000);
                let b = plain32((c.0 as f32, c.1 as f32), 100_000);
                let same = match (a, b) {
                    (Some(a), Some(b)) => (a as i64 - b as i64).abs() <= 1,
                    (None, None) => true,
                    _ => false,
                };
                agree += usize::from(same);
                rows.push(format!("{a:?}/{b:?}"));
            }
            eprintln!("1e-{e:<3} agree {agree}/{}  {}", cs.len(), rows.join(" "));
        }
    }

    /// 周期 2 のミニチュアの中心は -1 ちょうど。周期 3 は既知の値
    #[test]
    fn finds_minibrot_centers() {
        let (re, im, p, _) = find_center("-0.98", "0.02", 0.05, 30, 10).expect("center");
        assert_eq!(p, 2);
        assert_eq!(&re[..12], "-1.000000000");
        assert_eq!(&im[..12], "0.0000000000");
        let (re, im, p, _) = find_center("-0.12", "0.74", 0.02, 30, 10).expect("center");
        assert_eq!(p, 3);
        assert_eq!(&re[..10], "-0.1225611");
        assert_eq!(&im[..10], "0.74486176");
    }

    /// 全体の集合は 1。実軸の周期 3 のミニチュアは、尖りが -1.75 で中心が -1.7549 なので幅 0.0196 ほど
    #[test]
    fn estimates_minibrot_size() {
        let (_, _, p, size) = find_center("0.01", "0.01", 0.1, 30, 10).expect("center");
        assert_eq!(p, 1);
        assert!((size - 1.0).abs() < 1e-9, "{size}");
        let (_, _, p, size) = find_center("-1.755", "0", 0.002, 30, 10).expect("center");
        assert_eq!(p, 3);
        assert!(size > 0.018 && size < 0.021, "{size}");
    }

    /// 周期 8007 のミニチュアの中心を基準に、幅 4 × size の見え方を f64 (1 歩ずつ) で ASCII に出す。
    /// 中央にミニチュアが見えるはず。外の点も周期ごとに 1 歩ずつしか離れないので、反復は周期の何十倍も要る。
    /// f32 の表の版も何点か比べる
    #[test]
    #[ignore]
    fn ascii_nucleus() {
        let (re, im, p, size) = find_center("-0.743643887037158704752191506114774", "0.131825904205311970493132056385139", 2e-30, 60, 20_000).expect("center");
        eprintln!("period {p} size {size:e}");
        let limit = 300_000;
        // 軌道が尽きると差分を z で持ち直して精度が落ちるので、反復の上限まで用意する
        let orbit = reference_orbit(&re, &im, 40, limit).expect("orbit");
        let m_len = orbit.len() / 2;
        let z_at = |m: usize| (orbit[2 * m], orbit[2 * m + 1]);
        let plain = |c: (f64, f64), limit: usize| -> Option<usize> {
            let (mut dr, mut di) = (0.0, 0.0);
            let mut m = 0;
            for n in 0..limit {
                let (zr, zi) = z_at(m);
                let (xr, xi) = (zr + dr, zi + di);
                let r2 = xr * xr + xi * xi;
                if r2 > 256.0 {
                    return Some(n);
                }
                if r2 < dr * dr + di * di || m + 1 >= m_len {
                    (dr, di, m) = (xr, xi, 0);
                }
                let (zr, zi) = z_at(m);
                (dr, di) = (2.0 * (zr * dr - zi * di) + (dr * dr - di * di) + c.0, 2.0 * (zr * di + zi * dr) + 2.0 * dr * di + c.1);
                m += 1;
            }
            None
        };
        let width = 4.0 * size;
        let (cols, rows) = (64, 36);
        let (mut lo, mut hi) = (usize::MAX, 0);
        for row in 0..rows {
            let mut line = String::new();
            for col in 0..cols {
                let c = ((col as f64 / cols as f64 - 0.5) * width, -(row as f64 / rows as f64 - 0.5) * width * 9.0 / 16.0);
                line.push(match plain(c, limit) {
                    None => '#',
                    Some(n) => {
                        (lo, hi) = (lo.min(n), hi.max(n));
                        char::from(b'0' + ((n / p) % 10) as u8)
                    }
                });
            }
            eprintln!("{line}");
        }
        eprintln!("escaped between {lo} and {hi}");
        let s = size;
        compare_kernels(&re, &im, 40, limit, limit, &[(s, 0.0), (-s, 0.0), (0.0, s), (0.3 * s, 0.2 * s), (1.4 * s, -0.5 * s)]);
    }
}
