//! 長い処理の進捗を stderr の同じ行に書き続ける。割合はフレーム数ではなく時間で出す:
//! 直近のフレームにかかった時間から残りを見積もり、経過 / (経過 + 残り) を割合にする

use std::collections::VecDeque;
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
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


/// 立ち上がりの 1 行。別スレッドが 0.2 秒ごとに書き直すので、長い段階でも止まって見えない。
/// 深いところ (bignum や音声) から段数を知らせられるように、プロセスで 1 つだけ持つ
struct Stage {
    start: Instant,
    /// 段階の名前
    doing: &'static str,
    /// その中でいま回っている仕事と、済み / 全部
    work: Option<(&'static str, usize, usize)>,
    /// 段階そのものの数 (溜めたコマなど)
    count: Option<(usize, usize)>,
    /// 前に書いた時刻。深いループから毎回呼ばれても書き直しすぎない
    wrote: Instant,
    over: bool,
}

impl Stage {
    fn write(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.wrote).as_secs_f64() < 0.1 {
            return;
        }
        self.wrote = now;
        self.show();
    }

    fn show(&self) {
        let (label, gauge) = match (self.work, self.count) {
            (Some((what, done, total)), _) => (what, meter(done, total)),
            (None, Some((done, total))) => (self.doing, meter(done, total)),
            (None, None) => (self.doing, String::new()),
        };
        eprint!("\r{:<72}", format!("preview: {label}{gauge}  {:.1}s", self.start.elapsed().as_secs_f64()));
        let _ = std::io::stderr().flush();
    }
}

/// 進み具合の目盛り。全部が 0 なら数が分からないので空
fn meter(done: usize, total: usize) -> String {
    if total == 0 {
        return String::new();
    }
    const WIDTH: usize = 20;
    let done = done.min(total);
    let filled = done * WIDTH / total;
    format!("  [{}{}] {:>3}%", "#".repeat(filled), "-".repeat(WIDTH - filled), done * 100 / total)
}

/// 立ち上がりの行。preview のときだけ入る
static STAGE: Mutex<Option<Stage>> = Mutex::new(None);
/// STAGE が入っているか。深いループから毎回ロックしないための札
static LIVE: AtomicBool = AtomicBool::new(false);

/// いま何をどこまでやったかを知らせる。回数の分かるループから呼ぶ。
/// preview の立ち上がり以外では札を見るだけで戻る
pub fn working(what: &'static str, done: usize, total: usize) {
    if !LIVE.load(Ordering::Relaxed) {
        return;
    }
    let mut stage = STAGE.lock().expect("stage");
    let Some(stage) = stage.as_mut() else { return };
    if !stage.over {
        stage.work = Some((what, done, total));
        stage.write();
    }
}

/// preview が再生を始めるまでの段階を stderr の同じ行に出す。
/// ウィンドウが開いた時点と、再生が始まった時点で、そこまでの内訳を 1 行残す
pub struct Startup {
    /// -o を書いた render では出さない
    show: bool,
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
        if show {
            *STAGE.lock().expect("stage") = Some(Stage { start, doing: "", work: None, count: None, wrote: start, over: false });
            LIVE.store(true, Ordering::Relaxed);
            std::thread::spawn(|| {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    match STAGE.lock().expect("stage").as_ref() {
                        Some(stage) if !stage.over => stage.show(),
                        _ => return,
                    }
                }
            });
        }
        Self { show, start, stage_at: start, spent: Vec::new(), name: "", opened: false, playing: false }
    }

    /// 次の段階に移る。line は途中に出す文、name は内訳に出す短い名前
    pub fn stage(&mut self, line: &'static str, name: &'static str) {
        if self.playing {
            return;
        }
        self.close();
        self.name = name;
        self.edit(|stage| {
            stage.doing = line;
            (stage.work, stage.count) = (None, None);
        });
    }

    /// 段階そのものの数 (溜めたコマなど)
    pub fn count(&mut self, done: usize, total: usize) {
        if self.playing {
            return;
        }
        self.edit(|stage| {
            stage.work = None;
            stage.count = Some((done, total));
        });
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
        LIVE.store(false, Ordering::Relaxed);
        if let Some(stage) = STAGE.lock().expect("stage").as_mut() {
            stage.over = true;
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

    fn edit(&self, change: impl FnOnce(&mut Stage)) {
        if !self.show {
            return;
        }
        if let Some(stage) = STAGE.lock().expect("stage").as_mut() {
            change(stage);
            stage.show();
        }
    }

    /// 途中の行を消して、残る 1 行を出す
    fn note(&self, text: &str) {
        if !self.show {
            return;
        }
        let _hold = STAGE.lock().expect("stage");
        eprintln!("\r{:<72}", format!("preview: {text}"));
    }
}
