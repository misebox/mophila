//! 長い処理の進捗を stderr の同じ行に書き続ける。割合はフレーム数ではなく時間で出す:
//! 直近のフレームにかかった時間から残りを見積もり、経過 / (経過 + 残り) を割合にする

use std::collections::VecDeque;
use std::io::Write;
use std::time::Instant;

pub struct Progress {
    label: String,
    total: usize,
    start: Instant,
    last_frame: Instant,
    last_print: Option<Instant>,
    /// 直近のフレームの所要秒。残り時間の見積もりに使う
    recent: VecDeque<f64>,
}

impl Progress {
    pub fn new(label: &str, total: usize) -> Self {
        let now = Instant::now();
        Self { label: label.to_string(), total, start: now, last_frame: now, last_print: None, recent: VecDeque::new() }
    }

    /// 1 フレーム終わるごとに呼ぶ。done は終わった数。表示は 0.2 秒に 1 回まで
    pub fn step(&mut self, done: usize) {
        let now = Instant::now();
        self.recent.push_back(now.duration_since(self.last_frame).as_secs_f64());
        if self.recent.len() > 30 {
            self.recent.pop_front();
        }
        self.last_frame = now;
        if self.last_print.is_some_and(|t| now.duration_since(t).as_secs_f64() < 0.2) && done < self.total {
            return;
        }
        self.last_print = Some(now);
        let elapsed = now.duration_since(self.start).as_secs_f64();
        let per_frame = self.recent.iter().sum::<f64>() / self.recent.len() as f64;
        let remaining = per_frame * self.total.saturating_sub(done) as f64;
        let percent = if elapsed + remaining > 0.0 { (elapsed / (elapsed + remaining) * 100.0) as usize } else { 100 };
        let line = format!("{} {done}/{} frames  {percent:>3}%  {} elapsed  {} left", self.label, self.total, clock(elapsed), clock(remaining));
        eprint!("\r{line:<72}");
        let _ = std::io::stderr().flush();
    }

    pub fn finish(&self) {
        let line = format!("{} done: {} frames in {}", self.label, self.total, clock(self.start.elapsed().as_secs_f64()));
        eprintln!("\r{line:<72}");
    }
}

fn clock(seconds: f64) -> String {
    let s = seconds.round() as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{:02}:{:02}", s / 60, s % 60) }
}
