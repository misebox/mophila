use std::time::{Duration, Instant};

/// 環境変数 MOPHILA_TIMING が設定されていれば段階ごとの所要時間を集計し、終了時に stderr へ出す。
/// 平均だけだと「たまに詰まる」が見えないので、いちばん遅かったコマとその時刻も覚える
pub struct Timing {
    enabled: bool,
    start: Instant,
    /// いま組んでいるコマの時刻 (秒)。いちばん遅かったコマがどこかを言うために持つ
    now: f64,
    phases: Vec<Phase>,
}

struct Phase {
    name: &'static str,
    total: Duration,
    count: u32,
    /// いちばん遅かった 1 回と、そのときのコマの時刻
    worst: Duration,
    worst_at: f64,
}

impl Timing {
    pub fn from_env() -> Self {
        Self { enabled: std::env::var_os("MOPHILA_TIMING").is_some(), start: Instant::now(), now: 0.0, phases: Vec::new() }
    }

    pub fn on(&self) -> bool {
        self.enabled
    }

    /// これから組むコマの時刻
    pub fn at(&mut self, t: f64) {
        self.now = t;
    }

    pub fn measure<T>(&mut self, name: &'static str, f: impl FnOnce() -> T) -> T {
        if !self.enabled {
            return f();
        }
        let t = Instant::now();
        let result = f();
        self.add(name, t.elapsed());
        result
    }

    /// 自分で測った時間を足す (閉包で囲めないところ用)
    pub fn add(&mut self, name: &'static str, elapsed: Duration) {
        if !self.enabled {
            return;
        }
        let at = self.now;
        match self.phases.iter_mut().find(|p| p.name == name) {
            Some(p) => {
                p.total += elapsed;
                p.count += 1;
                if elapsed > p.worst {
                    (p.worst, p.worst_at) = (elapsed, at);
                }
            }
            None => self.phases.push(Phase { name, total: elapsed, count: 1, worst: elapsed, worst_at: at }),
        }
    }

    pub fn report(&self) {
        if !self.enabled {
            return;
        }
        for p in &self.phases {
            let avg = p.total.as_secs_f64() * 1000.0 / f64::from(p.count);
            let worst = p.worst.as_secs_f64() * 1000.0;
            eprintln!("{:<10} {:>8.3}s  x{:<5} avg {avg:>7.2}ms  worst {worst:>8.2}ms at {:.2}s", p.name, p.total.as_secs_f64(), p.count, p.worst_at);
        }
        eprintln!("{:<10} {:>8.3}s", "total", self.start.elapsed().as_secs_f64());
    }
}
