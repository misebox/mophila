mod ast;
mod audio;
mod bundle;
mod docs;
mod encode;
mod error;
mod eval;
mod gpu;
mod lexer;
mod lsp;
mod media;
mod parser;
mod preview;
mod progress;
mod report;
mod scene;
mod shader;
mod text;
mod timing;
mod value;

use std::error::Error;

use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// スクリプトを描画して動画または画像を出す
    Render {
        /// スクリプト (.moph)
        script: String,
        #[command(flatten)]
        out: OutputArgs,
    },
    /// 描画せずにスクリプトを実行する (log の確認用)
    Run {
        /// スクリプト (.moph)
        script: String,
    },
    /// ウィンドウを開いて実時間で再生する
    Preview {
        /// スクリプト (.moph)
        script: String,
        /// ウィンドウサイズ
        #[arg(long, default_value = "800x600", value_parser = parse_size)]
        size: (u32, u32),
        /// 最後まで再生したら先頭に戻る
        #[arg(long)]
        r#loop: bool,
        /// この時刻の画面を一時停止で出す (例: 1.5s, 01:23)
        #[arg(long, value_parser = parse_duration)]
        at: Option<f64>,
    },
    /// Timeline をテキストで出す (何が、いつ、どう変わるか)。--filter kind=TextArea attr=opacity text=... from=10s to=20s
    Timeline {
        /// スクリプト (.moph)
        script: String,
        /// 絞り込み (key=value を複数可)
        #[arg(long)]
        filter: Vec<String>,
    },
    /// 場面ごとのフレームを 1 枚の格子画像にする
    Sheet {
        /// スクリプト (.moph)
        script: String,
        /// 出力する画像 (.png)
        #[arg(short, long, default_value = "sheet.png")]
        output: String,
        /// この間隔でフレームを取る
        #[arg(long, default_value = "10s", value_parser = parse_duration)]
        every: f64,
        /// 取る時刻を直接指定 (カンマ区切り。--every より優先)
        #[arg(long, value_delimiter = ',', value_parser = parse_duration)]
        times: Vec<f64>,
        /// 1 コマの大きさ
        #[arg(long, default_value = "320x180", value_parser = parse_size)]
        cell: (u32, u32),
        /// 1 行のコマ数
        #[arg(long, default_value_t = 6)]
        cols: u32,
    },
    /// Language Server (stdio)。エディタから起動する
    Lsp {
        /// クライアント (vscode-languageclient など) が付ける印。stdio しかないので無視する
        #[arg(long)]
        stdio: bool,
    },
    /// 組み込みの説明を JSON で出す (scripts/docgen.py が読む)
    Doc,
    /// スクリプトを埋め込んだ実行ファイルを作る
    Bundle {
        /// スクリプト (.moph)
        script: String,
        /// 出力する実行ファイル
        #[arg(short, long)]
        output: String,
    },
}

/// 描画と出力のオプション。埋め込み済みバイナリではこれだけを受け取り、-o が無ければウィンドウで再生する
#[derive(Parser)]
struct OutputArgs {
    /// 出力する動画 (.mp4 など) または画像 (.png)。render では省略時 output.mp4 (--at 付きなら output.png)
    #[arg(short, long)]
    output: Option<String>,
    /// フレームレート
    #[arg(long, default_value_t = 10)]
    fps: u32,
    /// 画面サイズ。幅x高さ、または名前: 360p 480p 720p|hd 1080p|fhd 1440p|wqhd 2160p|4k|uhd (16:9), vga svga xga (4:3)
    #[arg(long, default_value = "800x600", value_parser = parse_size)]
    size: (u32, u32),
    /// ffmpeg の映像コーデック (-c:v)
    #[arg(long, default_value = "libx264")]
    codec: String,
    /// ffmpeg のピクセルフォーマット (-pix_fmt)
    #[arg(long, default_value = "yuv420p")]
    pix_fmt: String,
    /// 画像出力ならその時刻のフレーム、ウィンドウ再生ならその時刻で一時停止して開く (例: 1.5s, 500ms, 01:23)
    #[arg(long, value_parser = parse_duration)]
    at: Option<f64>,
    /// ウィンドウ再生のとき、最後まで再生したら先頭に戻る
    #[arg(long)]
    r#loop: bool,
    /// 動画のこの区間だけを出す。00:15..00:30 (15 秒から 30 秒)、00:15 (15 秒以降)、..01:30 (最初から 1 分 30 秒)
    #[arg(long, value_parser = parse_trim)]
    trim: Option<Trim>,
}

/// --trim の区間。None は端まで
#[derive(Clone, Copy)]
struct Trim {
    from: Option<f64>,
    to: Option<f64>,
}

fn parse_trim(s: &str) -> Result<Trim, String> {
    let side = |part: &str| -> Result<Option<f64>, String> { if part.is_empty() { Ok(None) } else { parse_duration(part).map(Some) } };
    let trim = match s.split_once("..") {
        Some((a, b)) => Trim { from: side(a)?, to: side(b)? },
        None => Trim { from: side(s)?, to: None },
    };
    if let (Some(a), Some(b)) = (trim.from, trim.to) {
        if b <= a {
            return Err(format!("trim end must be after start: {s}"));
        }
    }
    Ok(trim)
}

/// Duration リテラルを秒に変換する
fn parse_duration(s: &str) -> Result<f64, String> {
    if let Some(ms) = s.strip_suffix("ms") {
        return ms.parse::<f64>().map(|v| v / 1000.0).map_err(|_| format!("invalid duration: {s}"));
    }
    if let Some(sec) = s.strip_suffix('s') {
        return sec.parse().map_err(|_| format!("invalid duration: {s}"));
    }
    let parts: Vec<&str> = s.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return Err(format!("invalid duration: {s} (use 1.5s, 500ms, mm:ss or hh:mm:ss)"));
    }
    parts.iter().try_fold(0.0, |acc, part| {
        part.parse::<f64>().map(|v| acc * 60.0 + v).map_err(|_| format!("invalid duration: {s}"))
    })
}

const SIZE_NAMES: &[(&str, (u32, u32))] = &[
    // 16:9
    ("360p", (640, 360)),
    ("480p", (854, 480)),
    ("720p", (1280, 720)),
    ("hd", (1280, 720)),
    ("1080p", (1920, 1080)),
    ("fhd", (1920, 1080)),
    ("1440p", (2560, 1440)),
    ("wqhd", (2560, 1440)),
    ("2160p", (3840, 2160)),
    ("4k", (3840, 2160)),
    ("uhd", (3840, 2160)),
    // 4:3
    ("vga", (640, 480)),
    ("svga", (800, 600)),
    ("xga", (1024, 768)),
];

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let lower = s.to_ascii_lowercase();
    if let Some((_, size)) = SIZE_NAMES.iter().find(|(name, _)| *name == lower) {
        return Ok(*size);
    }
    let Some((w, h)) = s.split_once('x') else {
        return Err("幅x高さ の形式で指定する (例: 1920x1080)".into());
    };
    Ok((
        w.parse().map_err(|_| format!("幅が整数でない: {w}"))?,
        h.parse().map_err(|_| format!("高さが整数でない: {h}"))?,
    ))
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    if let Some(sources) = bundle::embedded()? {
        let src = sources.files[&sources.main].clone();
        let name = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| sources.main.clone());
        let result = render(&src, bundle::root_of(&sources.main), Some(&sources), name, OutputArgs::parse());
        sources.cleanup();
        return result;
    }
    match Cli::parse().command {
        Command::Render { script, mut out } => {
            // -o が無ければ output.mp4 に、--at 付きなら output.png に書く。ウィンドウは preview で
            if out.output.is_none() {
                out.output = Some(if out.at.is_some() { "output.png" } else { "output.mp4" }.to_string());
            }
            render(&std::fs::read_to_string(&script)?, base_dir(&script), None, file_name(&script), out)
        }
        Command::Run { script } => {
            let stmts = parser::parse(&std::fs::read_to_string(&script)?)?;
            let mut interp = eval::Interp::new();
            interp.base_dir = base_dir(&script);
            interp.run(&stmts)?;
            Ok(())
        }
        Command::Preview { script, size, r#loop, at } => {
            let (interp, view, duration) = load(&std::fs::read_to_string(&script)?, base_dir(&script), None)?;
            let media = media::collect(&view, duration);
            preview::run(file_name(&script), interp, view, duration, size, r#loop, at, &media)
        }
        Command::Timeline { script, filter } => {
            let (mut interp, view, duration) = load(&std::fs::read_to_string(&script)?, base_dir(&script), None)?;
            let filter = report::Filter::parse(&filter)?;
            let events = report::collect(&mut interp, &view);
            println!("duration: {duration:.2}s\n");
            println!("# events: start  length  object  attribute  change\n{}", report::format_events(&events, &filter));
            println!("\n# text visibility: from – to  length  text\n{}", report::text_visibility(&events, &filter));
            let media = media::collect(&view, duration);
            if !media.is_empty() {
                println!("\n# subtitles and audio: from – to  length  content\n{}", report::media_report(&media, &filter));
            }
            Ok(())
        }
        Command::Sheet { script, output, every, times, cell, cols } => sheet(&std::fs::read_to_string(&script)?, base_dir(&script), &output, every, times, cell, cols),
        Command::Lsp { .. } => lsp::run(),
        Command::Doc => {
            println!("{}", serde_json::to_string_pretty(&docs::json())?);
            Ok(())
        }
        Command::Bundle { script, output } => bundle::write(std::path::Path::new(&script), &output),
    }
}

/// タイトルバーに出す名前 (パスと拡張子を除いたファイル名)
fn file_name(script: &str) -> String {
    std::path::Path::new(script).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| script.to_string())
}

/// スクリプトのあるディレクトリ。import "file" の基準
fn base_dir(script: &str) -> std::path::PathBuf {
    std::path::Path::new(script).parent().map(|d| d.to_path_buf()).unwrap_or_default()
}

/// スクリプトを実行し、出力する View と動画の長さを返す
fn load(src: &str, base_dir: std::path::PathBuf, sources: Option<&bundle::Sources>) -> Result<(eval::Interp, value::ObjRef, f64), Box<dyn Error>> {
    let stmts = parser::parse(src)?;
    let mut interp = eval::Interp::new();
    interp.base_dir = base_dir;
    if let Some(s) = sources {
        interp.sources = s.files.clone();
        interp.assets = s.assets.clone();
    }
    interp.run(&stmts)?;
    let view = interp.output.clone().ok_or("no output: add \"output <view>\" to the script")?;
    let duration = view.borrow().tracks.iter().map(|p| p.end()).fold(0.0, f64::max);
    interp.snapshot(&view);
    Ok((interp, view, duration))
}

fn render(src: &str, base_dir: std::path::PathBuf, sources: Option<&bundle::Sources>, name: String, args: OutputArgs) -> Result<(), Box<dyn Error>> {
    let (width, height) = args.size;
    let (mut interp, view, duration) = load(src, base_dir, sources)?;
    let media = media::collect(&view, duration);
    let Some(output) = args.output else {
        return preview::run(name, interp, view, duration, args.size, args.r#loop, args.at, &media);
    };

    let mut timing = timing::Timing::from_env();
    let mut renderer = timing.measure("startup", || gpu::HeadlessRenderer::new(width, height))?;
    interp.cache_mut().shaders = Some(renderer.shader_runner());
    let is_image = std::path::Path::new(&output).extension().is_some_and(|e| e == "png");
    // --trim の区間。映像はこの時刻から描き、音声と字幕もこの区間に合わせてずらして切る
    let trim = args.trim.unwrap_or(Trim { from: None, to: None });
    let from = trim.from.unwrap_or(0.0).min(duration);
    let to = trim.to.unwrap_or(duration).min(duration);
    if !is_image && to <= from {
        return Err(format!("--trim starts at {from}s but the video ends at {duration}s").into());
    }
    let media = media.window(from, to);
    let mut ffmpeg = encode::Ffmpeg::spawn(
        &output,
        encode::Settings {
            width,
            height,
            fps: args.fps,
            codec: if is_image { "png" } else { &args.codec },
            pix_fmt: if is_image { "rgba" } else { &args.pix_fmt },
            media: if is_image { None } else { Some((&media, to - from)) },
        },
    )?;

    let mut pixels = Vec::new();
    let times: Vec<f64> = if is_image {
        vec![args.at.ok_or("--at is required for image output")?]
    } else {
        let frames = ((to - from) * f64::from(args.fps)).round() as u32;
        (0..frames).map(|f| from + f64::from(f) / f64::from(args.fps)).collect()
    };
    // フレーム N を GPU に投入したら、その完了を待つ前にフレーム N+1 の eval と scene を進める。
    // N の読み戻しは N+1 を投入した後に行う (GPU が N を描いている間に CPU が N+1 を組み立てる)
    let mut pending: Option<usize> = None;
    let mut progress = progress::Progress::new(&name, times.len());
    for (i, t) in times.into_iter().enumerate() {
        timing.measure("eval", || -> Result<(), Box<dyn Error>> {
            interp.begin_frame(t);
            let tracks = view.borrow().tracks.clone();
            for placed in &tracks {
                interp.apply_track(placed, t)?;
            }
            Ok(())
        })?;
        let scene = timing.measure("scene", || scene::build(&view, f64::from(width), f64::from(height), t, interp.cache_mut()))?;
        if let Some(prev) = pending.take() {
            timing.measure("readback", || renderer.read_pixels(prev, &mut pixels))?;
            timing.measure("encode", || ffmpeg.write_frame(&pixels))?;
        }
        pending = Some(timing.measure("render", || renderer.render(&scene, interp.cache_mut().shaders.as_mut()))?);
        progress.step(i + 1);
    }
    if let Some(prev) = pending {
        timing.measure("readback", || renderer.read_pixels(prev, &mut pixels))?;
        timing.measure("encode", || ffmpeg.write_frame(&pixels))?;
    }
    timing.measure("finish", || ffmpeg.finish())?;
    progress.finish();

    timing.report();
    Ok(())
}

/// 指定の時刻でフレームを描き、格子に並べて PNG に書く。各コマの左上に時刻を入れる
fn sheet(src: &str, base_dir: std::path::PathBuf, output: &str, every: f64, times: Vec<f64>, cell: (u32, u32), cols: u32) -> Result<(), Box<dyn Error>> {
    let (mut interp, view, duration) = load(src, base_dir, None)?;
    let media = media::collect(&view, duration);
    let times: Vec<f64> = if times.is_empty() {
        let n = (duration / every).floor() as u32 + 1;
        (0..n).map(|i| f64::from(i) * every).collect()
    } else {
        times
    };
    let (cw, ch) = cell;
    let cols = cols.max(1);
    let rows = (times.len() as u32).div_ceil(cols);
    let (width, height) = (cw * cols, ch * rows);

    let mut renderer = gpu::HeadlessRenderer::new(cw, ch)?;
    interp.cache_mut().shaders = Some(renderer.shader_runner());
    let mut pixels = Vec::new();
    let mut canvas = vec![0u8; (width * height * 4) as usize];
    let mut progress = progress::Progress::new("sheet", times.len());
    for (i, &t) in times.iter().enumerate() {
        interp.begin_frame(t);
        let tracks = view.borrow().tracks.clone();
        for placed in &tracks {
            interp.apply_track(placed, t)?;
        }
        let mut scene = scene::build(&view, f64::from(cw), f64::from(ch), t, interp.cache_mut())?;
        scene::overlay_subtitles(&mut scene, interp.cache_mut(), &media.cues, t, f64::from(cw), f64::from(ch));
        // 時刻のラベル
        let label = format!("{t:.1}s");
        let layout = interp.cache_mut().layout(&label, None, (f64::from(ch) * 0.09) as f32, None, text::alignment(None));
        let (lw, lh) = (f64::from(layout.width()) + 8.0, f64::from(layout.height()) + 4.0);
        scene.fill(vello::peniko::Fill::NonZero, vello::kurbo::Affine::IDENTITY, vello::peniko::Color::from_rgba8(0, 0, 0, 160), None, &vello::kurbo::Rect::new(0.0, 0.0, lw, lh));
        text::draw(&mut scene, layout, vello::kurbo::Affine::translate((4.0, 2.0)), vello::peniko::Color::WHITE);
        let slot = renderer.render(&scene, interp.cache_mut().shaders.as_mut())?;
        renderer.read_pixels(slot, &mut pixels)?;
        let (col, row) = (i as u32 % cols, i as u32 / cols);
        for y in 0..ch {
            let src_off = (y * cw * 4) as usize;
            let dst_off = (((row * ch + y) * width + col * cw) * 4) as usize;
            canvas[dst_off..dst_off + (cw * 4) as usize].copy_from_slice(&pixels[src_off..src_off + (cw * 4) as usize]);
        }
        progress.step(i + 1);
    }
    progress.finish();
    let mut ffmpeg = encode::Ffmpeg::spawn(output, encode::Settings { width, height, fps: 1, codec: "png", pix_fmt: "rgba", media: None })?;
    ffmpeg.write_frame(&canvas)?;
    ffmpeg.finish()?;
    Ok(())
}
