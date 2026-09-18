//! 読み上げ。書いた文を音声にして、音声トラックに混ぜる。
//!
//! engine は行ごとに選べる。いま動くのは、その機械に入っている音声合成を呼ぶもの。
//! 作った音声はビルドの中間物としてキャッシュに置き、同じ文と同じ声なら作り直さない。

use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::lang::eval::{Attr, opt};
use crate::lang::value::Value;
use crate::render::media::{AudioClip, Cue, Media};

/// 作った音声の置き場所。読み直せるものなので、リポジトリではなくキャッシュに置く
pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("mophila").join("voice")
}

/// 音声を作るもの。engine ごとに受け取る設定が違うので、型ごと分ける。
/// 増やすときは struct を 1 つ作って ENGINES に足す
pub trait VoiceEngine: Sync {
    /// スクリプトに書く型の名前 (Say など)
    fn name(&self) -> &'static str;
    /// 要る実行ファイル。PATH に無ければ、その engine は使えない
    fn program(&self) -> &'static str;
    /// この型が持つ属性。ここが唯一の定義で、型の検査も補完もこれを引く
    fn attrs(&self) -> &'static [Attr];
    /// 走らせるコマンド
    fn argv(&self, text: &str, settings: &Settings, out: &Path) -> Vec<String>;
    /// 書き出す形式 (拡張子)
    fn format(&self) -> &'static str {
        "wav"
    }
    /// 一覧に出す説明
    fn doc(&self) -> &'static str;
    /// 一覧に出す書き方
    fn make(&self) -> &'static str;
}

/// スクリプトに書かれた engine の設定。属性をそのまま持つ
#[derive(Clone, Default)]
pub struct Settings(pub Vec<(String, Value)>);

impl Settings {
    fn text(&self, name: &str) -> Option<String> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, v)| match v {
            Value::Str(s) => s.clone(),
            // 150.0 ではなく 150 で渡す。外のプログラムは整数しか受けないことがある
            Value::Number(n, _) if n.fract() == 0.0 => format!("{n:.0}"),
            other => other.to_string(),
        })
    }

    /// キャッシュの鍵に使う形 (書いた順に依らないよう並べ替える)
    fn id(&self) -> String {
        let mut parts: Vec<String> = self.0.iter().map(|(n, v)| format!("{n}={v}")).collect();
        parts.sort();
        parts.join(" ")
    }
}

/// macOS の say
struct SayVoiceEngine;

impl VoiceEngine for SayVoiceEngine {
    fn name(&self) -> &'static str {
        "SayVoiceEngine"
    }
    fn program(&self) -> &'static str {
        "say"
    }
    fn doc(&self) -> &'static str {
        "macOS の say で読み上げる。Timeline.place の voice に渡す"
    }
    fn make(&self) -> &'static str {
        "SayVoiceEngine(voice = \"Kyoko\")"
    }
    fn attrs(&self) -> &'static [Attr] {
        const ATTRS: &[Attr] = &[opt("voice", "String"), opt("rate", "Number")];
        ATTRS
    }
    fn argv(&self, text: &str, settings: &Settings, out: &Path) -> Vec<String> {
        let mut argv = vec!["say".to_string()];
        if let Some(v) = settings.text("voice") {
            argv.push("-v".to_string());
            argv.push(v);
        }
        if let Some(r) = settings.text("rate") {
            argv.push("-r".to_string());
            argv.push(r);
        }
        // 拡張子だけでは AIFF になるので、形式を明示して wav を書かせる
        argv.push("--data-format=LEF32@22050".to_string());
        argv.push("-o".to_string());
        argv.push(out.to_string_lossy().to_string());
        argv.push(text.to_string());
        argv
    }
}

/// espeak-ng
struct EspeakVoiceEngine;

impl VoiceEngine for EspeakVoiceEngine {
    fn name(&self) -> &'static str {
        "EspeakVoiceEngine"
    }
    fn program(&self) -> &'static str {
        "espeak-ng"
    }
    fn doc(&self) -> &'static str {
        "espeak-ng で読み上げる。Timeline.place の voice に渡す"
    }
    fn make(&self) -> &'static str {
        "EspeakVoiceEngine(voice = \"ja\")"
    }
    fn attrs(&self) -> &'static [Attr] {
        const ATTRS: &[Attr] =
            &[opt("voice", "String"), opt("speed", "Number"), opt("pitch", "Number"), opt("gap", "Number")];
        ATTRS
    }
    fn argv(&self, text: &str, settings: &Settings, out: &Path) -> Vec<String> {
        let mut argv = vec!["espeak-ng".to_string()];
        for (name, flag) in [("voice", "-v"), ("speed", "-s"), ("pitch", "-p"), ("gap", "-g")] {
            if let Some(v) = settings.text(name) {
                argv.push(flag.to_string());
                argv.push(v);
            }
        }
        argv.push("-w".to_string());
        argv.push(out.to_string_lossy().to_string());
        argv.push("--".to_string());
        argv.push(text.to_string());
        argv
    }
}

/// 使える engine。増やすときはここに 1 つ足す
pub const ENGINES: &[&dyn VoiceEngine] = &[&SayVoiceEngine, &EspeakVoiceEngine];

/// 型の名前から engine を引く
pub fn by_name(name: &str) -> Option<&'static dyn VoiceEngine> {
    ENGINES.iter().find(|e| e.name() == name).copied()
}

/// voice を書かなかったときの engine。この機械に入っているものを上から探す
pub fn default_engine() -> Result<&'static dyn VoiceEngine, Box<dyn Error>> {
    match ENGINES.iter().find(|e| exists(e.program())) {
        Some(engine) => Ok(*engine),
        None => Err(format!(
            "no voice engine on this machine. install one of {}, or set voice = on the Narration",
            ENGINES.iter().map(|e| e.program()).collect::<Vec<_>>().join(" ")
        )
        .into()),
    }
}

/// engine の型の名前を全部
pub fn names() -> Vec<&'static str> {
    ENGINES.iter().map(|e| e.name()).collect()
}

/// その実行ファイルが PATH にあるか。走らせずに探す
fn exists(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|dir| dir.join(program).is_file())
}

/// 1 つ喋らせて、音声のパスと長さ (秒) を返す。同じものは作り直さない
fn say(engine: &dyn VoiceEngine, text: &str, settings: &Settings, cache: &Path) -> Result<(PathBuf, f64), Box<dyn Error>> {
    if !exists(engine.program()) {
        return Err(format!("{} needs {}, which is not on this machine", engine.name(), engine.program()).into());
    }
    let name = format!("{}.{}", key(engine.name(), text, &settings.id()), engine.format());
    let path = cache.join(&name);
    if path.is_file() {
        let length = audio_length(&path)?;
        return Ok((path, length));
    }
    // 書きかけは別のディレクトリに置く。engine には拡張子がそのままの名前を渡す
    let part_dir = cache.join(".part");
    std::fs::create_dir_all(&part_dir)?;
    let part = part_dir.join(&name);
    let argv = engine.argv(text, settings, &part);
    let status = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .status()
        .map_err(|e| format!("cannot run {}: {e}", argv[0]))?;
    if !status.success() {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{} failed for \"{}\"", engine.name(), short(text)).into());
    }
    // 置き場所に入れる前に読めることを確かめる。
    // 終了コードが 0 でも中身が空や壊れていることがあり、そのまま入れると以後ずっとそれを使う
    let Ok(length) = audio_length(&part) else {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{} wrote a file ffprobe cannot read for \"{}\"", engine.name(), short(text)).into());
    };
    std::fs::create_dir_all(cache)?;
    std::fs::rename(&part, &path)?;
    let _ = std::fs::remove_dir(&part_dir);
    Ok((path, length))
}

/// 同じ engine・同じ文・同じ設定なら同じ名前になる
fn key(engine: &str, text: &str, settings: &str) -> String {
    let mut h = DefaultHasher::new();
    engine.hash(&mut h);
    text.hash(&mut h);
    settings.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn short(text: &str) -> String {
    text.chars().take(20).collect()
}

/// 長さを ffprobe で調べる (音声ファイルの読み込みと同じ)
fn audio_length(path: &Path) -> Result<f64, Box<dyn Error>> {
    let output = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1"])
        .arg(path)
        .output()
        .map_err(|e| format!("ffprobe is needed to measure the voice: {e}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse::<f64>().map_err(|_| format!("cannot read the length of {}", path.display()).into())
}

/// 字幕が切り替わったと分かるように空ける間隔 (秒)
const GAP: f64 = 0.2;

/// 読み上げを字幕にする。
///
/// **区間は台本に書いた at と duration そのもの。** 合成した音声の実測長で縮めたり、
/// 前後に余裕を足したりしない。開始を動かすと timeline が言う時刻と食い違う。
/// 音声は at に置いて duration で切るので、声は必ずこの区間の中にある。
///
/// 隣と GAP 未満しか空かないときだけ、前の字幕の終わりを縮めて GAP 空ける。
/// 縮めるのは終わりだけで、開始は動かさない
pub fn set_cues(media: &mut Media, _cache: &Path) {
    let mut cues: Vec<Cue> =
        media.narrations.iter().map(|line| Cue { at: line.at, length: line.length, text: line.text.clone() }).collect();
    cues.sort_by(|a, b| a.at.total_cmp(&b.at));
    for i in 1..cues.len() {
        let limit = cues[i].at - GAP;
        let prev = &mut cues[i - 1];
        if prev.at + prev.length > limit {
            prev.length = (limit - prev.at).max(0.0);
        }
    }
    media.cues = cues;
}

pub fn mix_in(media: &mut Media, cache: &Path) -> Result<(), Box<dyn Error>> {
    for line in std::mem::take(&mut media.narrations) {
        let engine = match &line.engine {
            Some(name) => by_name(name).ok_or_else(|| format!("no voice engine \"{name}\""))?,
            None => default_engine()?,
        };
        let (path, length) = say(engine, &line.text, &line.settings, cache)?;
        if length > line.length + 0.05 {
            eprintln!("warning: the voice for \"{}\" is {length:.1}s but duration is {:.1}s; it is cut", short(&line.text), line.length);
        }
        media.clips.push(AudioClip {
            path,
            name: format!("voice: {}", short(&line.text)),
            at: line.at,
            offset: 0.0,
            length: length.min(line.length),
            fade_in: 0.0,
            fade_out: 0.0,
            volume: line.volume,
            looping: false,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::media::Narration;

    fn media(lines: &[(f64, f64)]) -> Media {
        let narrations = lines
            .iter()
            .map(|(at, length)| Narration {
                at: *at,
                length: *length,
                text: format!("{at}"),
                engine: None,
                settings: Settings::default(),
                volume: 1.0,
            })
            .collect();
        Media { narrations, ..Media::default() }
    }

    /// timeline が言う時刻と字幕の時刻は同じでなければならない。
    /// 実測で縮めたり前後に余裕を足したりして、ここが食い違ったことがある
    #[test]
    fn cues_are_the_written_intervals() {
        let mut m = media(&[(0.5, 3.0), (4.0, 3.0), (7.5, 2.0)]);
        set_cues(&mut m, Path::new("/nonexistent"));
        let got: Vec<(f64, f64)> = m.cues.iter().map(|c| (c.at, c.length)).collect();
        assert_eq!(got, vec![(0.5, 3.0), (4.0, 3.0), (7.5, 2.0)]);
    }

    /// 詰まっているときは、前の字幕の終わりだけを縮めて間隔を空ける。開始は動かさない
    #[test]
    fn a_crowded_cue_gives_up_its_tail_not_its_start() {
        let mut m = media(&[(0.0, 3.0), (3.0, 3.0), (6.1, 3.0)]);
        set_cues(&mut m, Path::new("/nonexistent"));
        let got: Vec<(f64, f64)> = m.cues.iter().map(|c| (c.at, (c.length * 1000.0).round() / 1000.0)).collect();
        assert_eq!(got, vec![(0.0, 2.8), (3.0, 2.9), (6.1, 3.0)]);
    }
}
