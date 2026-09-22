use std::error::Error;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};

use crate::render::media::Media;

pub struct Settings<'a> {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub codec: &'a str,
    pub pix_fmt: &'a str,
    /// 音声と字幕と、動画の長さ (秒)。画像出力では None
    pub media: Option<(&'a Media, f64)>,
    /// 拡張子ごとの追加の指定 (GIF のパレットなど)
    pub filter: Option<&'a str>,
}

/// 出力の形式。拡張子から決まる
pub struct Format {
    /// 1 枚の画像。--at が要る
    pub image: bool,
    pub codec: &'static str,
    pub pix_fmt: &'static str,
    /// 映像に掛けるフィルタ。音声と混ぜないものだけ
    pub filter: Option<&'static str>,
    /// 音声と字幕を入れられるか
    pub media: bool,
}

/// 出力ファイルの拡張子から形式を決める。知らない拡張子は None
pub fn format_of(path: &str) -> Option<Format> {
    let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let image = |codec, pix_fmt| Format { image: true, codec, pix_fmt, filter: None, media: false };
    let video = |codec, pix_fmt| Format { image: false, codec, pix_fmt, filter: None, media: true };
    Some(match ext.as_str() {
        "png" => image("png", "rgba"),
        "jpg" | "jpeg" => image("mjpeg", "yuvj420p"),
        "webp" => image("webp", "yuv420p"),
        "tif" | "tiff" => image("tiff", "rgba"),
        "mp4" | "m4v" => video("libx264", "yuv420p"),
        "mov" => video("libx264", "yuv420p"),
        "mkv" => video("libx264", "yuv420p"),
        "webm" => video("libvpx-vp9", "yuv420p"),
        // GIF は 256 色。先に使う色を決めてから割り当てないと汚くなる
        "gif" => Format {
            image: false,
            codec: "gif",
            pix_fmt: "rgb8",
            filter: Some("split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=3"),
            media: false,
        },
        "apng" => Format { image: false, codec: "apng", pix_fmt: "rgba", filter: None, media: false },
        _ => return None,
    })
}

/// 書き込みを待たせるフレームの数。ffmpeg が詰まっている間も、この分だけは描き続けられる
const QUEUE: usize = 3;

/// ffmpeg を子プロセスとして起動し、stdin に RGBA フレームを流し込む。
/// 書き込みは別スレッド。ffmpeg (x264) が 1 フレームを噛んでいる間に次のフレームを描けるように
pub struct Ffmpeg {
    child: Child,
    /// 書き込みスレッドへフレームを渡す口。ffmpeg が先に落ちたら閉じる
    tx: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    /// 書き込みスレッド。全部書けたら true
    writer: Option<std::thread::JoinHandle<bool>>,
    /// エラーに出す。使えないコーデックを指した場合に分かるように
    codec: String,
    /// ffmpeg が stderr に言ったこと。進捗の行が書き直しで上書きしてしまうので、こちらで拾っておく
    said: std::sync::Arc<std::sync::Mutex<String>>,
    /// 終わったら消す一時ファイル (字幕)
    temp_files: Vec<PathBuf>,
}

impl Drop for Ffmpeg {
    fn drop(&mut self) {
        for f in &self.temp_files {
            let _ = std::fs::remove_file(f);
        }
    }
}

impl Ffmpeg {
    pub fn spawn(output: &str, s: Settings) -> Result<Self, Box<dyn Error>> {
        let size = format!("{}x{}", s.width, s.height);
        let fps = s.fps.to_string();
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-y", "-loglevel", "error"]).args(["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &size, "-r", &fps, "-i", "-"]);
        let mut temp_files = Vec::new();
        match s.media {
            Some((media, duration)) if !media.is_empty() => temp_files.extend(add_media(&mut cmd, output, media, duration)?),
            _ => {
                cmd.args(["-map", "0:v"]);
            }
        }
        if let Some(filter) = s.filter {
            cmd.args(["-vf", filter]);
        }
        // 後から「これは mophila で作った」と分かるように残す (入れ物が対応していなければ無視される)
        let stamp = format!("comment=mophila {}", env!("CARGO_PKG_VERSION"));
        let mut child = cmd
            .args(["-metadata", &stamp])
            .args(["-c:v", s.codec, "-pix_fmt", s.pix_fmt, output])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start ffmpeg: {e}"))?;
        let said = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        if let Some(mut err) = child.stderr.take() {
            let into = std::sync::Arc::clone(&said);
            std::thread::spawn(move || {
                let mut all = String::new();
                if std::io::Read::read_to_string(&mut err, &mut all).is_ok() {
                    // 長いときは終わりだけ。理由は最後に出る
                    let tail: Vec<&str> = all.lines().rev().take(8).collect();
                    *into.lock().expect("stderr") = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
                }
            });
        }
        let mut stdin: ChildStdin = child.stdin.take().ok_or("cannot take the stdin of ffmpeg")?;
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(QUEUE);
        // 送り手が閉じたら for が終わり、stdin が落ちて ffmpeg に終わりが伝わる
        let writer = std::thread::spawn(move || {
            for frame in rx {
                if stdin.write_all(&frame).is_err() {
                    return false;
                }
            }
            true
        });
        Ok(Self { child, tx: Some(tx), writer: Some(writer), codec: s.codec.to_string(), said, temp_files })
    }

    /// 列に積む。列が一杯なら空くまで待つ (ffmpeg の速さに合わせる)
    pub fn write_frame(&mut self, rgba: &[u8]) -> Result<(), Box<dyn Error>> {
        let Some(tx) = &self.tx else { return Err("ffmpeg has already exited".into()) };
        if tx.send(rgba.to_vec()).is_err() {
            // 書き込みスレッドが終わっている = ffmpeg が先に落ちた。理由は ffmpeg が stderr に出している
            self.tx = None;
            let _ = self.child.wait();
            return Err(self.failed());
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), Box<dyn Error>> {
        self.tx = None;
        let written = self.writer.take().is_none_or(|w| w.join().unwrap_or(false));
        if !self.child.wait()?.success() || !written {
            return Err(self.failed());
        }
        Ok(())
    }

    /// ffmpeg が言ったことをそのまま添える。何も言っていなければ、よくある原因を挙げる
    fn failed(&self) -> Box<dyn Error> {
        let said = self.said.lock().map(|s| s.clone()).unwrap_or_default();
        let why = match said.trim().is_empty() {
            false => format!("\n{}", said.trim()),
            true => " (it said nothing; the disk filling up or the file system's size limit are the usual reasons)".to_string(),
        };
        format!("ffmpeg failed (-c:v {}){why}", self.codec).into()
    }
}

/// 音声を入力に足して filter_complex で 1 本に混ぜ、字幕は SRT の一時ファイルを入力にする。
/// 音声は動画の長さに合わせて切る・伸ばす (無音)。戻り値は一時ファイル
fn add_media(cmd: &mut Command, output: &str, media: &Media, duration: f64) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let ext = std::path::Path::new(output).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let (audio_codec, subtitle_codec) = match ext.as_str() {
        "mp4" | "m4v" | "mov" => ("aac", Some("mov_text")),
        "webm" => ("libopus", Some("webvtt")),
        "mkv" => ("aac", Some("srt")),
        _ => ("aac", None),
    };
    let mut temp_files = Vec::new();
    for clip in &media.clips {
        if clip.looping {
            cmd.args(["-stream_loop", "-1"]);
        }
        cmd.arg("-i").arg(&clip.path);
    }
    // 入力は出力の指定より前に全部並べる
    let subtitle_input = match (media.cues.is_empty(), subtitle_codec) {
        (true, _) => None,
        (false, Some(codec)) => {
            let path = std::env::temp_dir().join(format!("mophila-{}.srt", std::process::id()));
            std::fs::write(&path, crate::render::media::srt(&media.cues))?;
            cmd.args(["-f", "srt", "-i"]).arg(&path);
            temp_files.push(path);
            Some(((media.clips.len() + 1).to_string(), codec))
        }
        (false, None) => {
            eprintln!("warning: subtitles are written only to .mp4 / .mov / .mkv / .webm; skipped for {output}");
            None
        }
    };
    let mut filters: Vec<String> = Vec::new();
    for (i, clip) in media.clips.iter().enumerate() {
        let mut chain = vec![
            format!("atrim=start={}:duration={}", clip.offset, clip.length),
            "asetpts=PTS-STARTPTS".to_string(),
            "aformat=sample_rates=48000:channel_layouts=stereo".to_string(),
        ];
        if clip.volume != 1.0 {
            chain.push(format!("volume={}", clip.volume));
        }
        if clip.fade_in > 0.0 {
            chain.push(format!("afade=t=in:st=0:d={}", clip.fade_in));
        }
        if clip.fade_out > 0.0 {
            chain.push(format!("afade=t=out:st={}:d={}", (clip.length - clip.fade_out).max(0.0), clip.fade_out));
        }
        if clip.at > 0.0 {
            chain.push(format!("adelay={}:all=1", (clip.at * 1000.0).round() as u64));
        }
        filters.push(format!("[{}:a]{}[a{i}]", i + 1, chain.join(",")));
    }
    let labels: String = (0..media.clips.len()).map(|i| format!("[a{i}]")).collect();
    let mix = if media.clips.len() > 1 { format!("amix=inputs={}:normalize=0:duration=longest,", media.clips.len()) } else { String::new() };
    cmd.args(["-map", "0:v"]);
    if !media.clips.is_empty() {
        filters.push(format!("{labels}{mix}apad,atrim=duration={duration}[aout]"));
        cmd.args(["-filter_complex", &filters.join(";"), "-map", "[aout]", "-c:a", audio_codec, "-b:a", "192k"]);
    }
    if let Some((index, codec)) = subtitle_input {
        cmd.args(["-map", &index, "-c:s", codec]);
    }
    Ok(temp_files)
}
