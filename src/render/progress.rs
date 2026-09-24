//! 長い処理の進捗を stderr の同じ行に書き続ける。割合はフレーム数ではなく時間で出す:
//! 直近のフレームにかかった時間から残りを見積もり、経過 / (経過 + 残り) を割合にする

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex};
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

/// いま出している行。別スレッドが 0.2 秒ごとに経過を書き直すので、
/// 長い段階でも止まって見えない
struct Stage {
    start: Instant,
    doing: &'static str,
    count: Option<(usize, usize)>,
    over: bool,
}

impl Stage {
    fn write(&self) {
        let of = match self.count {
            Some((done, total)) => format!(" {done}/{total}"),
            None => String::new(),
        };
        eprint!("\r{:<72}", format!("preview: {}{of}  {:.1}s", self.doing, self.start.elapsed().as_secs_f64()));
        let _ = std::io::stderr().flush();
    }
}

/// preview が再生を始めるまでの段階を stderr の同じ行に出す。
/// ウィンドウが開いた時点と、再生が始まった時点で、そこまでの内訳を 1 行残す
pub struct Startup {
    /// -o を書いた render では出さない
    now: Option<Arc<Mutex<Stage>>>,
    start: Instant,
    stage_at: Instant,
    /// 終わった段階の (名前, 秒)
    spent: Vec<(&'static str, f64)>,
    /// いまの段階の、内訳に出す短い名前
    name: &'static str,
    opened: bool,
    playing: bool,
}

impl Startup {
    pub fn new(show: bool) -> Self {
        let start = Instant::now();
        let now = show.then(|| {
            let stage = Arc::new(Mutex::new(Stage { start, doing: "", count: None, over: false }));
            let ticker = stage.clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    let stage = ticker.lock().expect("stage");
                    if stage.over {
                        return;
                    }
                    stage.write();
                }
            });
            stage
        });
        Self { now, start, stage_at: start, spent: Vec::new(), name: "", opened: false, playing: false }
    }

    /// 次の段階に移る。line は途中に出す文、name は内訳に出す短い名前
    pub fn stage(&mut self, line: &'static str, name: &'static str) {
        if self.playing {
            return;
        }
        self.close();
        self.name = name;
        if let Some(now) = &self.now {
            let mut stage = now.lock().expect("stage");
            (stage.doing, stage.count) = (line, None);
            stage.write();
        }
    }

    /// 同じ段階の途中経過
    pub fn count(&mut self, done: usize, total: usize) {
        if self.playing {
            return;
        }
        if let Some(now) = &self.now {
            let mut stage = now.lock().expect("stage");
            stage.count = Some((done, total));
            stage.write();
        }
    }

    /// ウィンドウが開いたところ。ここまでの内訳を残す
    pub fn opened(&mut self) {
        if self.opened {
            return;
        }
        self.opened = true;
        self.close();
        let parts: Vec<String> = self.spent.iter().map(|(name, s)| format!("{name} {s:.1}s")).collect();
        self.note(&format!("window open after {:.1}s ({})", self.start.elapsed().as_secs_f64(), parts.join(", ")));
        self.spent.clear();
    }

    /// 再生が始まったところ。溜めたコマの数と、そこに掛かった時間を残す
    pub fn playing(&mut self, frames: usize) {
        if self.playing {
            return;
        }
        let waited = self.stage_at.elapsed().as_secs_f64();
        let each = if frames == 1 { "frame" } else { "frames" };
        self.note(&format!("playing after {:.1}s ({frames} {each} in {waited:.1}s)", self.start.elapsed().as_secs_f64()));
        self.stop();
    }

    /// 立ち上がりの途中で終わったときも、書き足すのを止める
    pub fn stop(&mut self) {
        self.playing = true;
        if let Some(now) = &self.now {
            now.lock().expect("stage").over = true;
        }
    }

    /// いまの段階を閉じて、掛かった時間を控える
    fn close(&mut self) {
        let now = Instant::now();
        if !self.name.is_empty() {
            self.spent.push((self.name, now.duration_since(self.stage_at).as_secs_f64()));
        }
        self.stage_at = now;
        self.name = "";
    }

    /// 途中の行を消して、残る 1 行を出す
    fn note(&self, text: &str) {
        let Some(now) = &self.now else { return };
        let _stage = now.lock().expect("stage");
        eprintln!("\r{:<72}", format!("preview: {text}"));
    }
}

