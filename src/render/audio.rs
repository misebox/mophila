//! preview の音。音声ファイルを ffmpeg で PCM にしてメモリに持ち、cpal で出力デバイスに流す。
//! 再生位置と一時停止は映像側が決め、ここはそれに合わせる

use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use cpal::SampleFormat;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::render::media::Media;

pub struct Output {
    _stream: cpal::Stream,
    state: Arc<Mutex<Mixer>>,
}

struct Mixer {
    clips: Vec<Clip>,
    /// 動画の時間軸での再生位置 (秒)
    position: f64,
    playing: bool,
    rate: f64,
    channels: usize,
}

struct Clip {
    /// デバイスのサンプルレートとチャンネル数に合わせた interleaved PCM
    pcm: Arc<Vec<f32>>,
    frames: usize,
    at: f64,
    length: f64,
    fade_in: f64,
    fade_out: f64,
    volume: f32,
    looping: bool,
}

impl Output {
    pub fn open(media: &Media) -> Result<Self, Box<dyn Error>> {
        let device = cpal::default_host().default_output_device().ok_or("no audio output device")?;
        let supported = device.default_output_config()?;
        let config = supported.config();
        let (rate, channels) = (config.sample_rate, usize::from(config.channels));

        let mut decoded: HashMap<PathBuf, Arc<Vec<f32>>> = HashMap::new();
        let mut clips = Vec::new();
        for c in &media.clips {
            let pcm = match decoded.get(&c.path) {
                Some(p) => p.clone(),
                None => {
                    let p = Arc::new(decode(&c.path, rate, channels)?);
                    decoded.insert(c.path.clone(), p.clone());
                    p
                }
            };
            clips.push(Clip {
                frames: pcm.len() / channels,
                pcm,
                at: c.at,
                length: c.length,
                fade_in: c.fade_in,
                fade_out: c.fade_out,
                volume: c.volume as f32,
                looping: c.looping,
            });
        }
        let state = Arc::new(Mutex::new(Mixer { clips, position: 0.0, playing: false, rate: f64::from(rate), channels }));
        let err_fn = |e| eprintln!("audio: {e}");
        let stream = match supported.sample_format() {
            SampleFormat::F32 => {
                let s = state.clone();
                device.build_output_stream(config, move |data: &mut [f32], _| fill(&s, data), err_fn, None)?
            }
            SampleFormat::I16 => {
                let s = state.clone();
                let mut scratch = Vec::new();
                device.build_output_stream(
                    config,
                    move |data: &mut [i16], _| {
                        scratch.resize(data.len(), 0.0);
                        fill(&s, &mut scratch);
                        for (d, v) in data.iter_mut().zip(&scratch) {
                            *d = (v * f32::from(i16::MAX)) as i16;
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            other => return Err(format!("unsupported audio sample format {other}").into()),
        };
        stream.play()?;
        Ok(Self { _stream: stream, state })
    }

    /// 映像の位置に合わせる。force でなければ、ずれが小さいうちは音を途切れさせない
    pub fn sync(&self, position: f64, playing: bool, force: bool) {
        let mut m = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if force || (m.position - position).abs() > 0.1 {
            m.position = position;
        }
        m.playing = playing;
    }
}

/// 出力バッファを埋める。フレームごとに、その時刻に鳴っている音声を音量とフェードを掛けて足す
fn fill(state: &Mutex<Mixer>, out: &mut [f32]) {
    let mut m = state.lock().unwrap_or_else(PoisonError::into_inner);
    if !m.playing {
        out.fill(0.0);
        return;
    }
    let (rate, channels) = (m.rate, m.channels);
    let mut position = m.position;
    for frame in out.chunks_mut(channels) {
        frame.fill(0.0);
        for c in &m.clips {
            let local = position - c.at;
            if local < 0.0 || local >= c.length {
                continue;
            }
            let mut gain = c.volume;
            if c.fade_in > 0.0 && local < c.fade_in {
                gain *= (local / c.fade_in) as f32;
            }
            if c.fade_out > 0.0 && local > c.length - c.fade_out {
                gain *= ((c.length - local) / c.fade_out) as f32;
            }
            // 位置はフレーム番号に丸めてから繰り返す (浮動小数の剰余で 1 つずれるのを避ける)
            let index = (local * rate).round() as usize;
            let index = if c.looping { index % c.frames } else { index };
            if index >= c.frames {
                continue;
            }
            for (k, s) in frame.iter_mut().enumerate() {
                *s += c.pcm[index * channels + k] * gain;
            }
        }
        for s in frame.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
        position += 1.0 / rate;
    }
    m.position = position;
}

/// ffmpeg で f32 interleaved の PCM にする
fn decode(path: &Path, rate: u32, channels: usize) -> Result<Vec<f32>, Box<dyn Error>> {
    let output = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", "f32le", "-ac", &channels.to_string(), "-ar", &rate.to_string(), "-"])
        .output()
        .map_err(|e| format!("ffmpeg is needed to play {}: {e}", path.display()))?;
    if !output.status.success() {
        return Err(format!("cannot decode {}: {}", path.display(), String::from_utf8_lossy(&output.stderr).trim()).into());
    }
    Ok(output.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mixer(clips: Vec<Clip>) -> Mutex<Mixer> {
        Mutex::new(Mixer { clips, position: 0.0, playing: true, rate: 10.0, channels: 1 })
    }

    fn clip(pcm: Vec<f32>, at: f64, length: f64, looping: bool) -> Clip {
        Clip { frames: pcm.len(), pcm: Arc::new(pcm), at, length, fade_in: 0.0, fade_out: 0.0, volume: 1.0, looping }
    }

    #[test]
    fn mixes_delayed_clips_and_advances_position() {
        // 10 Hz、1 ch。a は 0.0s から、b は 0.3s から
        let m = mixer(vec![clip(vec![0.5; 5], 0.0, 0.5, false), clip(vec![0.25; 5], 0.3, 0.5, false)]);
        let mut out = vec![0.0; 6];
        fill(&m, &mut out);
        assert_eq!(out, vec![0.5, 0.5, 0.5, 0.75, 0.75, 0.25]);
        assert!((m.lock().unwrap().position - 0.6).abs() < 1e-9);
    }

    #[test]
    fn loops_short_file_and_fades_out() {
        // 2 フレームのファイルを 1.0s まで繰り返す。最後の 0.5s でフェードアウト
        let mut c = clip(vec![1.0, -1.0], 0.0, 1.0, true);
        c.fade_out = 0.5;
        let m = mixer(vec![c]);
        let mut out = vec![0.0; 10];
        fill(&m, &mut out);
        // 0.5s までは素通し、0.5s を過ぎてから (0.6s〜) 減る
        assert_eq!(&out[..6], &[1.0, -1.0, 1.0, -1.0, 1.0, -1.0]);
        assert!(out[6].abs() < 1.0 && out[9].abs() < out[6].abs());
    }

    #[test]
    fn silent_when_paused_or_outside() {
        let m = mixer(vec![clip(vec![1.0; 5], 2.0, 0.5, false)]);
        let mut out = vec![9.0; 3];
        fill(&m, &mut out);
        assert_eq!(out, vec![0.0; 3]);
        m.lock().unwrap().playing = false;
        let mut out = vec![9.0; 3];
        fill(&m, &mut out);
        assert_eq!(out, vec![0.0; 3]);
    }
}
