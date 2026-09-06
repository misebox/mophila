use std::time::{Duration, Instant};

/// 環境変数 MOPHILA_TIMING が設定されていれば段階ごとの所要時間を集計し、終了時に stderr へ出す
pub struct Timing {
    enabled: bool,
    start: Instant,
    phases: Vec<(&'static str, Duration, u32)>,
}

impl Timing {
    pub fn from_env() -> Self {
        Self {
            enabled: std::env::var_os("MOPHILA_TIMING").is_some(),
            start: Instant::now(),
            phases: Vec::new(),
        }
    }

    pub fn measure<T>(&mut self, name: &'static str, f: impl FnOnce() -> T) -> T {
        if !self.enabled {
            return f();
        }
        let t = Instant::now();
        let result = f();
        let elapsed = t.elapsed();
        match self.phases.iter_mut().find(|(n, ..)| *n == name) {
            Some((_, total, count)) => {
                *total += elapsed;
                *count += 1;
            }
            None => self.phases.push((name, elapsed, 1)),
        }
        result
    }

    pub fn report(&self) {
        if !self.enabled {
            return;
        }
        for (name, total, count) in &self.phases {
            let avg = total.as_secs_f64() * 1000.0 / f64::from(*count);
            eprintln!("{name:<10} {:>8.3}s  x{count:<5} avg {avg:.2}ms", total.as_secs_f64());
        }
        eprintln!("{:<10} {:>8.3}s", "total", self.start.elapsed().as_secs_f64());
    }
}
