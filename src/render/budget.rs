//! 1 コマに要る GPU のメモリを数えて、重いところを知らせる。
//!
//! 数えるのは、Shader の塗りのテクスチャとズームの帯 (フレームをまたいで持つ)、
//! 透ける View のレイヤー (そのコマだけ)、読み戻しのバッファ (ずっと同じ)。
//! 足りなくなると wgpu が Out of Memory で落ちるだけなので、その前に、
//! どのコマが何に使っているかを出す。

use std::sync::atomic::{AtomicU64, Ordering};

use crate::render::text::Bytes;

/// 直前のコマに要った量。GPU から「足りない」と言われたときに添える
static LAST: AtomicU64 = AtomicU64::new(0);

/// 直前のコマの合計 (バイト)
pub fn last() -> u64 {
    LAST.load(Ordering::Relaxed)
}

/// GPU が「足りない」と言ってきたときに、何をしていたかを添えて止める。
/// wgpu の既定は種別だけ出して panic するので、切り分けられない
pub fn on_gpu_error(e: &wgpu::Error) {
    if !matches!(e, wgpu::Error::OutOfMemory { .. }) {
        return;
    }
    eprintln!(
        "Error: RuntimeError.OutOfMemory: the GPU ran out of memory while drawing. \
         The frame before this one needed {}. Lower --size, make the Shader fills smaller, \
         or split the scene. Run with --gpu-budget to see which frames are heavy.",
        size(last())
    );
    std::process::exit(1);
}

/// 書き出しの間、いちばん要ったコマを覚えておく
pub struct Budget {
    /// 超えたら知らせる量。書かなければ知らせず、最後の 1 行だけ出す
    limit: Option<u64>,
    /// コマによらず要るもの (読み戻しのバッファ)
    fixed: u64,
    /// (合計, その時刻, 内訳)
    peak: (u64, f64, Bytes),
    over: usize,
}

impl Budget {
    pub fn new(limit: Option<u64>, fixed: u64) -> Self {
        Budget { limit, fixed, peak: (0, 0.0, Bytes::default()), over: 0 }
    }

    /// 1 コマぶん。予算を超えたコマは、最初の 1 つだけ内訳を出す
    pub fn see(&mut self, t: f64, bytes: Bytes) {
        let total = bytes.total() + self.fixed;
        LAST.store(total, Ordering::Relaxed);
        if total > self.peak.0 {
            self.peak = (total, t, bytes);
        }
        let Some(limit) = self.limit else { return };
        if total <= limit {
            return;
        }
        self.over += 1;
        if self.over == 1 {
            eprintln!("warning: {} needs {}, over the {} budget ({})", at(t), size(total), size(limit), breakdown(bytes, self.fixed));
        }
    }

    /// 書き出しの終わりに 1 行。いちばん要ったコマと、その内訳
    pub fn report(&self) {
        let (total, t, bytes) = self.peak;
        if total == 0 {
            return;
        }
        let over = match (self.limit, self.over) {
            (Some(limit), n) if n > 0 => format!(", {n} frames over the {} budget", size(limit)),
            _ => String::new(),
        };
        eprintln!("gpu: peak {} at {} ({}){over}", size(total), at(t), breakdown(bytes, self.fixed));
    }
}

fn breakdown(bytes: Bytes, fixed: u64) -> String {
    format!("shader {} / layers {} / readback {}", size(bytes.shaders), size(bytes.layers), size(fixed))
}

fn at(t: f64) -> String {
    format!("{t:.2}s")
}

pub fn size(bytes: u64) -> String {
    let n = bytes as f64;
    match bytes {
        0..=1_048_575 => format!("{:.0}KB", n / 1024.0),
        1_048_576..=1_073_741_823 => format!("{:.1}MB", n / 1_048_576.0),
        _ => format!("{:.2}GB", n / 1_073_741_824.0),
    }
}
