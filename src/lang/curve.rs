//! 変化の曲線と、`ease(t, t0, t1, v0, v1)` の仲間。

use crate::lang::error::{Kind, Result, err};
use crate::lang::eval::interpolate;
use crate::lang::value::Value;

/// 曲線の名前。`progress` の値の隣に書く名前と、補間関数の名前は同じ
pub const CURVES: [&str; 4] = ["linear", "ease_in", "ease_out", "ease"];

/// 進み具合 k (0..1) を、曲線に通した進み具合にする
pub fn shape(name: Option<&str>, k: f64) -> f64 {
    match name {
        Some("ease_in") => k * k,
        Some("ease_out") => 1.0 - (1.0 - k) * (1.0 - k),
        Some("ease") => {
            if k < 0.5 { 2.0 * k * k } else { 1.0 - 2.0 * (1.0 - k) * (1.0 - k) }
        }
        _ => k,
    }
}

fn seconds(v: &Value, what: &str) -> Result<f64> {
    match v {
        Value::Duration(d) => Ok(*d),
        v => err(Kind::ArgumentType, format!("ease: {what} expects Duration, found {}", v.type_name())),
    }
}

/// `ease(t, t0, t1, v0, v1)`。t が t0 から t1 の間にあるとき、v0 から v1 へ補間する。区間の外は端の値
pub fn call(curve: &str, values: &[Value]) -> Result<Value> {
    let [t, t0, t1, v0, v1] = values else {
        return err(Kind::ArityMismatch, format!("{curve} takes 5 arguments (t, t0, t1, v0, v1), {} given", values.len()));
    };
    let (t, t0, t1) = (seconds(t, "t")?, seconds(t0, "t0")?, seconds(t1, "t1")?);
    if t1 <= t0 {
        return err(Kind::OutOfRange, format!("{curve}: t1 must be after t0, found {t0}s to {t1}s"));
    }
    let k = (t - t0) / (t1 - t0);
    // 端は計算せずに返す (補間の丸め誤差で端の値がずれない)
    if k <= 0.0 {
        return Ok(v0.clone());
    }
    if k >= 1.0 {
        return Ok(v1.clone());
    }
    Ok(interpolate(v0, v1, shape(Some(curve), k)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: f64) -> Value {
        Value::Duration(s)
    }

    fn run(curve: &str, t: f64) -> f64 {
        match call(curve, &[d(t), d(1.0), d(3.0), Value::num(10.0), Value::num(20.0)]).expect("ease") {
            Value::Number(n, _) => n,
            v => panic!("not a number: {v:?}"),
        }
    }

    #[test]
    fn stays_at_the_ends_outside_the_range() {
        assert_eq!(run("linear", 0.0), 10.0);
        assert_eq!(run("ease", 9.0), 20.0);
    }

    #[test]
    fn linear_goes_straight() {
        assert_eq!(run("linear", 2.0), 15.0);
    }

    #[test]
    fn ease_in_is_slow_at_first() {
        assert!(run("ease_in", 1.5) < run("linear", 1.5));
        assert!(run("ease_out", 1.5) > run("linear", 1.5));
    }

    #[test]
    fn ease_is_symmetric() {
        assert_eq!(run("ease", 2.0), 15.0);
    }

    #[test]
    fn an_empty_range_is_an_error() {
        assert!(call("ease", &[d(1.0), d(2.0), d(2.0), Value::num(0.0), Value::num(1.0)]).is_err());
    }
}
