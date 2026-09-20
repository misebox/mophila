//! フレームの組み立てを別スレッドに分ける。
//!
//! 1 フレームの状態は「スクリプト直後の状態 + その時刻までに始まった Timeline を置いた順に適用」で決まり、
//! 前のフレームに依存しない。なのでフレームごとに独立して組める。
//! スレッドは自分のインタプリタとオブジェクトの木を持ち (`Rc` を跨がせない)、描画命令だけを返す。
//!
//! GPU が要る Shader の塗りだけは、この道を通せない (描画命令を組む時点で GPU を使うため)。

use std::error::Error;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

use vello::Scene;

use crate::lang::value::ObjRef;

/// スクリプトを実行し直すのに要るもの。スレッドへ渡すので Rc を含まない
#[derive(Clone)]
pub struct Job {
    pub src: String,
    pub base_dir: PathBuf,
    pub sources: Option<crate::bundle::Sources>,
    pub project: Option<crate::project::Project>,
    pub width: f64,
    pub height: f64,
}

/// 組み立てた 1 フレーム (番号, 描画命令, 要った GPU のメモリ)
pub type Frame = Result<(usize, Scene, crate::render::text::Bytes), String>;

/// フレームを順に受け取る。裏で n 本のスレッドが先回りして組む
pub struct Frames {
    /// スレッドごとの受け口。フレームを配った順に読むと、並びが戻る
    channels: Vec<Receiver<Frame>>,
    next: usize,
    handles: Vec<std::thread::JoinHandle<()>>,
}

impl Frames {
    /// times をスレッドに順繰りに配る。各スレッドは先に 2 フレームまで組んで待つ
    pub fn start(job: &Job, times: &[f64], workers: usize) -> Self {
        let mut channels = Vec::new();
        let mut handles = Vec::new();
        for w in 0..workers {
            let (tx, rx): (SyncSender<Frame>, Receiver<Frame>) = sync_channel(2);
            let mine: Vec<(usize, f64)> = times.iter().enumerate().skip(w).step_by(workers).map(|(i, t)| (i, *t)).collect();
            let job = job.clone();
            handles.push(std::thread::spawn(move || build(&job, &mine, &tx)));
            channels.push(rx);
        }
        Frames { channels, next: 0, handles }
    }

    /// 次のフレーム。配った順に読むので、並びは times のまま
    pub fn recv(&mut self) -> Option<Frame> {
        let got = self.channels[self.next % self.channels.len()].recv().ok();
        self.next += 1;
        got
    }
}

impl Drop for Frames {
    fn drop(&mut self) {
        // 受け口を閉じるとスレッド側の send が失敗して終わる
        self.channels.clear();
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

/// スレッド 1 本の仕事。自分のインタプリタでスクリプトを実行し、割り当てられた時刻を組む
fn build(job: &Job, mine: &[(usize, f64)], tx: &SyncSender<Frame>) {
    let (mut interp, view) = match prepare(job) {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.send(Err(e.to_string()));
            return;
        }
    };
    let tracks = crate::lang::eval::all_tracks(&view);
    for &(i, t) in mine {
        let frame = (|| -> Result<(Scene, crate::render::text::Bytes), Box<dyn Error>> {
            interp.begin_frame(t);
            interp.apply_tracks(&tracks, t)?;
            let scene = crate::render::scene::build(&view, job.width, job.height, t, interp.cache_mut())?;
            Ok((scene, interp.cache_mut().bytes()))
        })();
        let message = match frame {
            Ok((scene, bytes)) => Ok((i, scene, bytes)),
            Err(e) => Err(e.to_string()),
        };
        if tx.send(message).is_err() {
            return;
        }
    }
}

/// このスレッドのインタプリタとオブジェクトの木
fn prepare(job: &Job) -> Result<(crate::lang::eval::Interp, ObjRef), Box<dyn Error>> {
    let stmts = crate::lang::parser::parse(&job.src)?;
    let mut interp = crate::lang::eval::Interp::new();
    interp.base_dir = job.base_dir.clone();
    if let Some(p) = &job.project {
        interp.set_project(std::rc::Rc::new(p.clone()));
    }
    if let Some(s) = &job.sources {
        interp.sources = s.files.clone();
        interp.assets = s.assets.clone();
    }
    interp.run(&stmts)?;
    let view = interp.output.clone().ok_or("no output")?;
    interp.snapshot(&view);
    Ok((interp, view))
}
