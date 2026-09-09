//! 動画に付ける音声と字幕。View に置かれた Timeline を辿り、絶対時刻で集める

use crate::lang::value::{ObjRef, Placed, Track, Value};

/// 音声ファイルの 1 区間
pub struct AudioClip {
    pub path: std::path::PathBuf,
    pub name: String,
    /// 動画の時間軸での開始
    pub at: f64,
    /// ファイルの何秒目から鳴らすか (--trim で途中から始まるとき)
    pub offset: f64,
    /// 実際に鳴らす長さ。繰り返しは duration: か動画の終わりまで
    pub length: f64,
    pub fade_in: f64,
    pub fade_out: f64,
    pub volume: f64,
    pub looping: bool,
}

/// 字幕 1 つ
#[derive(Clone)]
pub struct Cue {
    pub at: f64,
    pub length: f64,
    pub text: String,
}

#[derive(Default)]
pub struct Media {
    pub clips: Vec<AudioClip>,
    pub cues: Vec<Cue>,
}

impl Media {
    pub fn is_empty(&self) -> bool {
        self.clips.is_empty() && self.cues.is_empty()
    }

    /// from..to の区間だけにする。時刻は from が 0 になるようにずらし、区間の外は切る
    pub fn window(self, from: f64, to: f64) -> Media {
        let span = to - from;
        let clips = self
            .clips
            .into_iter()
            .filter_map(|mut c| {
                let start = c.at - from;
                let end = (start + c.length).min(span);
                if end <= 0.0 || start >= span {
                    return None;
                }
                if start < 0.0 {
                    // 途中から鳴らす。フェードインはもう終わっている
                    c.offset += -start;
                    c.fade_in = (c.fade_in + start).max(0.0);
                }
                c.at = start.max(0.0);
                c.length = end - c.at;
                Some(c)
            })
            .collect();
        let cues = self
            .cues
            .into_iter()
            .filter_map(|mut q| {
                let start = q.at - from;
                let end = (start + q.length).min(span);
                if end <= 0.0 || start >= span {
                    return None;
                }
                q.at = start.max(0.0);
                q.length = end - q.at;
                Some(q)
            })
            .collect();
        Media { clips, cues }
    }
}

/// total は動画の長さ。繰り返す音声はそこまで鳴らす
pub fn collect(view: &ObjRef, total: f64) -> Media {
    let mut media = Media::default();
    let tracks = view.borrow().tracks.clone();
    for placed in &tracks {
        walk(placed, 0.0, total, &mut media);
    }
    media.clips.sort_by(|a, b| a.at.total_cmp(&b.at));
    media.cues.sort_by(|a, b| a.at.total_cmp(&b.at));
    media
}

/// limit は、ここまでに通った入れ物 (duration を指定した Timeline) の終わりの最小。はみ出した分は切る
fn walk(placed: &Placed, origin: f64, limit: f64, out: &mut Media) {
    let start = origin + placed.at;
    match &placed.track {
        Track::Container(obj) => {
            let explicit = match obj.borrow().attrs.get("duration") {
                Some(Value::Duration(d)) => Some(*d),
                _ => None,
            };
            let limit = explicit.map_or(limit, |d| limit.min(start + d));
            let children = obj.borrow().tracks.clone();
            for child in &children {
                walk(child, start, limit, out);
            }
        }
        Track::Timeline(_) => {}
        Track::Audio(audio, clip) => {
            let natural = if clip.looping { clip.cut.unwrap_or(f64::INFINITY) } else { audio.clip_length(clip) };
            let length = (start + natural).min(limit) - start;
            if length <= 0.0 {
                return;
            }
            out.clips.push(AudioClip {
                path: audio.path.clone(),
                name: audio.name.clone(),
                at: start,
                offset: 0.0,
                length,
                fade_in: placed.fade_in,
                fade_out: placed.fade_out,
                volume: clip.volume,
                looping: clip.looping,
            });
        }
        Track::Subtitle(obj) => {
            let o = obj.borrow();
            let text = match o.attrs.get("text") {
                Some(Value::Str(s)) => s.clone(),
                _ => String::new(),
            };
            let length = (start + placed.track.duration()).min(limit) - start;
            if length > 0.0 {
                out.cues.push(Cue { at: start, length, text });
            }
        }
    }
}

/// SRT 形式。ffmpeg が mp4 の字幕トラック (mov_text) などに変換する
pub fn srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (i, cue) in cues.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, timestamp(cue.at), timestamp(cue.at + cue.length), cue.text));
    }
    out
}

fn timestamp(seconds: f64) -> String {
    let ms = (seconds * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}
