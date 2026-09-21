//! 多倍長の固定小数点と、それを使うマンデルブロの下ごしらえ。
//!
//! 深いズームでは中心の座標が f64 に入らない。文字列で受け取って、必要な桁で基準軌道を作る。
//! 使うのは加算・減算・乗算だけで、値の範囲も小さいので、汎用の多倍長は持たずに自前で足りる。
//! 読み込みのときに 1 度作って全フレームで使い回すので、速さは要らない。

use std::collections::HashMap;

use crate::lang::error::{Kind, Result, err};
use crate::lang::value::{Module, Value};

/// 符号と大きさで持つ固定小数点。
/// 値は (-1)^neg * (limbs[n-1] + Σ_{i<n-1} limbs[i] * 2^(-64*(n-1-i)))。
/// いちばん上の limb が整数部、残りが小数部
#[derive(Clone, Debug, PartialEq, Eq)]
struct Fix {
    neg: bool,
    limbs: Vec<u64>,
}

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

    fn to_f64(&self) -> f64 {
        let n = self.len();
        let mut out = self.limbs[n - 1] as f64;
        let mut scale = 1.0 / 18_446_744_073_709_551_616.0;
        for i in (0..n - 1).rev() {
            out += self.limbs[i] as f64 * scale;
            scale /= 18_446_744_073_709_551_616.0;
        }
        match self.neg {
            true => -out,
            false => out,
        }
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
        let whole: u64 = int_part.parse().map_err(|_| crate::lang::error::MophError::new(Kind::OutOfRange, format!("{int_part} is too large")))?;
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
}

/// 桁数から limb の数を決める。10 進 1 桁はおよそ 3.33 bit。余裕を 1 limb 持つ
fn limbs_for(digits: usize) -> usize {
    (digits as f64 * 3.33 / 64.0).ceil() as usize + 2
}

/// 中心の点の軌道を多倍長で回し、f64 の [re, im, re, im, ...] で返す。
/// 脱出したらそこで止める
fn reference_orbit(re: &str, im: &str, digits: usize, steps: usize) -> Result<Vec<f64>> {
    let n = limbs_for(digits);
    let (cre, cim) = (Fix::parse(re, n)?, Fix::parse(im, n)?);
    let (mut zre, mut zim) = (Fix::zero(n), Fix::zero(n));
    let four = Fix { neg: false, limbs: { let mut l = vec![0; n]; l[n - 1] = 4; l } };
    let mut out = Vec::with_capacity(steps * 2);
    for _ in 0..steps {
        out.push(zre.to_f64());
        out.push(zim.to_f64());
        let re2 = zre.mul(&zre);
        let im2 = zim.mul(&zim);
        if re2.add(&im2).cmp_abs(&four) == std::cmp::Ordering::Greater {
            return Ok(out);
        }
        let next_im = zre.mul(&zim).add(&zre.mul(&zim)).add(&cim);
        zre = re2.sub(&im2).add(&cre);
        zim = next_im;
    }
    Ok(out)
}

pub const DOCS: &[crate::docs::Entry] = &[crate::docs::Entry {
    name: "reference_orbit",
    signature: "bignum.reference_orbit(re: String, im: String, digits: Number, steps: Number)",
    returns: "List<Number>",
    doc: "中心の点の軌道を、指定した 10 進の桁数で回して [re, im, re, im, …] で返す。深いズームの基準軌道。脱出したらそこで止まる",
}];

/// `import bignum` で束縛されるもの
pub fn module() -> Module {
    let mut items = HashMap::new();
    items.insert("reference_orbit".into(), Value::Builtin("bignum.reference_orbit"));
    Module { name: "bignum".into(), items }
}

/// `bignum.…` の呼び出し。`name` は前置きを外したもの
pub fn call(f: &str, values: &[Value]) -> Result<Value> {
    let name = &format!("bignum.{f}")[..];
    match (f, values) {
        ("reference_orbit", [Value::Str(re), Value::Str(im), Value::Number(digits, _), Value::Number(steps, _)]) => {
            let whole = |what: &str, v: f64, top: f64| match v >= 1.0 && v.fract() == 0.0 && v <= top {
                true => Ok(v as usize),
                false => err(Kind::OutOfRange, format!("{name}: {what} must be a whole number between 1 and {top}, found {v}")),
            };
            let orbit = reference_orbit(re, im, whole("digits", *digits, 5000.0)?, whole("steps", *steps, 10_000_000.0)?)?;
            Ok(Value::List(std::rc::Rc::new(std::cell::RefCell::new(orbit.into_iter().map(Value::num).collect()))))
        }
        ("reference_orbit", _) => err(Kind::ArgumentType, format!("{name} takes (re: String, im: String, digits: Number, steps: Number)")),
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
}
