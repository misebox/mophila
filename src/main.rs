mod bundle;
mod docs;
mod lang;
mod lsp;
mod project;
mod render;
mod report;
mod stdlib;
mod timing;

use std::error::Error;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Config file. Without it, mophila.yaml in the current directory, or in the folder given instead of a script
    #[arg(short = 'f', long, global = true)]
    config: Option<String>,
    /// Override a config value as name=value (repeatable). MOPHILA_<NAME> does the same
    #[arg(long = "set", global = true, value_name = "name=value")]
    set: Vec<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render a script into a video or image file
    Render {
        /// Script (.moph). Falls back to the entry in mophila.yaml
        script: Option<String>,
        #[command(flatten)]
        out: OutputArgs,
    },
    /// Run a script without drawing (to check log output)
    Run {
        /// Script (.moph). Falls back to the entry in mophila.yaml
        script: Option<String>,
    },
    /// Open a window and play in real time
    Preview {
        /// Script (.moph). Falls back to the entry in mophila.yaml
        script: Option<String>,
        /// Window size
        #[arg(long, default_value = "960x540", value_parser = parse_size)]
        size: (u32, u32),
        /// Start over when it reaches the end
        #[arg(long)]
        r#loop: bool,
        /// Open paused at this time (e.g. 1.5s, 01:23)
        #[arg(long, value_parser = parse_duration)]
        at: Option<f64>,
        /// Show only a part of the box: height (fill the height), width, or a fraction like 0.25,1 / 25%,100%
        #[arg(long, value_parser = parse_crop)]
        crop: Option<render::scene::Crop>,
        /// Where the cropped part sits in what is left over: 0%..100% per axis, or left / center / right / top / bottom / topLeft ...
        #[arg(long, value_parser = parse_align)]
        align: Option<(f64, f64)>,
        /// Color of the bands when the picture does not fill the frame (default white)
        #[arg(long, value_parser = parse_pad)]
        pad: Option<String>,
    },
    /// List what changes when, in time order. --filter kind=TextArea attr=opacity text=... from=10s to=20s
    Timeline {
        /// Script (.moph). Falls back to the entry in mophila.yaml
        script: Option<String>,
        /// Narrow the list (key=value, repeatable)
        #[arg(long)]
        filter: Vec<String>,
    },
    /// Put one frame per moment into a single grid image
    Sheet {
        /// Script (.moph). Falls back to the entry in mophila.yaml
        script: Option<String>,
        /// Image to write (.png)
        #[arg(short, long, default_value = "sheet.png")]
        output: String,
        /// Take a frame every this long
        #[arg(long, default_value = "10s", value_parser = parse_duration)]
        every: f64,
        /// Times to take, comma separated (wins over --every)
        #[arg(long, value_delimiter = ',', value_parser = parse_duration)]
        times: Vec<f64>,
        /// Size of one cell
        #[arg(long, default_value = "320x180", value_parser = parse_size)]
        cell: (u32, u32),
        /// Cells per row
        #[arg(long, default_value_t = 6)]
        cols: u32,
    },
    /// Language server over stdio. Editors start this
    Lsp {
        /// Flag some clients pass (vscode-languageclient). Ignored; stdio is the only transport
        #[arg(long)]
        stdio: bool,
    },
    /// List the font names this machine can use (what TextArea.font accepts)
    Fonts,
    /// Print the builtin types, functions and methods as JSON (read by scripts/docgen.py)
    Doc,
    /// Write the subtitles as a file, without rendering the video
    Subs {
        /// Script (.moph). Falls back to the entry in mophila.yaml
        script: Option<String>,
        /// File to write. .srt or .vtt picks the format. Without it, prints to stdout
        #[arg(short, long)]
        output: Option<String>,
        /// Write only this span, as in render
        #[arg(long, value_parser = parse_trim)]
        trim: Option<Trim>,
    },
    /// Build an executable with the script inside
    Bundle {
        /// Script (.moph). Falls back to the entry in mophila.yaml
        script: Option<String>,
        /// Executable to write
        #[arg(short, long)]
        output: String,
    },
}

/// Render and output options. A bundled executable takes only these,
/// and plays in a window when -o is not given.
//
// (この doc コメントは、埋め込み済みバイナリの --help にそのまま出る)
#[derive(Parser)]
struct OutputArgs {
    /// File to write. The extension picks the format:
    /// video mp4 mov mkv webm gif apng / image png jpg webp tiff (needs --at).
    /// render defaults to output.mp4 (output.png with --at)
    #[arg(short, long)]
    output: Option<String>,
    /// Frames per second
    #[arg(long, default_value_t = 10)]
    fps: u32,
    /// Frame size as WIDTHxHEIGHT, or a name: 360p 480p 720p|hd 1080p|fhd 1440p|wqhd 2160p|4k|uhd (16:9), vga svga xga (4:3)
    #[arg(long, default_value = "960x540", value_parser = parse_size)]
    size: (u32, u32),
    /// ffmpeg video codec (-c:v). Taken from the extension if omitted
    #[arg(long)]
    codec: Option<String>,
    /// ffmpeg pixel format (-pix_fmt). Taken from the extension if omitted
    #[arg(long)]
    pix_fmt: Option<String>,
    /// For an image, the frame at this time; for a window, open paused there (e.g. 1.5s, 500ms, 01:23)
    #[arg(long, value_parser = parse_duration)]
    at: Option<f64>,
    /// In a window, start over when it reaches the end
    #[arg(long)]
    r#loop: bool,
    /// Write only this span: 00:15..00:30 (15s to 30s), 00:15 (from 15s on), ..01:30 (up to 1m30s)
    #[arg(long, value_parser = parse_trim)]
    trim: Option<Trim>,
    /// Threads that build frames (default 1). 0 picks a number from the CPU count.
    /// Only helps when building costs more than drawing. A Shader always uses one
    #[arg(long, default_value_t = 1)]
    jobs: usize,
    /// Warn about frames that need more GPU memory than this (e.g. 512MB, 2GB)
    #[arg(long, value_parser = parse_bytes)]
    gpu_budget: Option<u64>,
    /// Show only a part of the box: height (fill the height), width, or a fraction like 0.25,1 / 25%,100%
    #[arg(long, value_parser = parse_crop)]
    crop: Option<render::scene::Crop>,
    /// Where the cropped part sits in what is left over: 0%..100% per axis, or left / center / right / top / bottom / topLeft ...
    #[arg(long, value_parser = parse_align)]
    align: Option<(f64, f64)>,
    /// Color of the bands when the picture does not fill the frame (default white)
    #[arg(long, value_parser = parse_pad)]
    pad: Option<String>,
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

/// "512MB" "2GB" "1500000" をバイト数にする
fn parse_bytes(s: &str) -> Result<u64, String> {
    let lower = s.trim().to_ascii_lowercase();
    let (number, unit) = match lower.find(|c: char| c.is_alphabetic()) {
        Some(i) => lower.split_at(i),
        None => (lower.as_str(), ""),
    };
    let value: f64 = number.trim().parse().map_err(|_| format!("invalid size: {s}"))?;
    let scale = match unit.trim() {
        "" | "b" => 1.0,
        "k" | "kb" => 1024.0,
        "m" | "mb" => 1024.0 * 1024.0,
        "g" | "gb" => 1024.0 * 1024.0 * 1024.0,
        other => return Err(format!("unknown unit \"{other}\" (use MB or GB)")),
    };
    Ok((value * scale) as u64)
}

/// 切り取る大きさ。height / width か、箱に対する割合 (0.25,1 でも 25%,100% でも)
fn parse_crop(s: &str) -> Result<render::scene::Crop, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "height" => return Ok(render::scene::Crop::Height),
        "width" => return Ok(render::scene::Crop::Width),
        _ => {}
    }
    let (w, h) = s.split_once(',').ok_or_else(|| format!("crop must be height, width, or W,H (e.g. 0.25,1): {s}"))?;
    Ok(render::scene::Crop::Size(parse_ratio(w, "crop")?, parse_ratio(h, "crop")?))
}

/// 余りのどこに寄せるか。名前か、軸ごとの割合
fn parse_align(s: &str) -> Result<(f64, f64), String> {
    let named = |x: f64, y: f64| Ok((x, y));
    match s.trim() {
        "center" => return named(0.5, 0.5),
        "left" => return named(0.0, 0.5),
        "right" => return named(1.0, 0.5),
        "top" => return named(0.5, 0.0),
        "bottom" => return named(0.5, 1.0),
        "topLeft" => return named(0.0, 0.0),
        "topRight" => return named(1.0, 0.0),
        "bottomLeft" => return named(0.0, 1.0),
        "bottomRight" => return named(1.0, 1.0),
        _ => {}
    }
    let (x, y) = s.split_once(',').ok_or_else(|| format!("align must be a name (left, center, topRight ...) or X,Y: {s}"))?;
    Ok((parse_ratio(x, "align")?, parse_ratio(y, "align")?))
}

/// 0..1 の割合。"0.25" でも "25%" でも
fn parse_ratio(s: &str, whose: &str) -> Result<f64, String> {
    let text = s.trim();
    let (number, percent) = match text.strip_suffix('%') {
        Some(rest) => (rest, true),
        None => (text, false),
    };
    let value: f64 = number.trim().parse().map_err(|_| format!("{whose} takes numbers like 0.25 or 25%, found {s}"))?;
    Ok(if percent { value / 100.0 } else { value })
}

/// 帯の色。#rgb #rgba #rrggbb #rrggbbaa
fn parse_pad(s: &str) -> Result<String, String> {
    let hex = s.trim().strip_prefix('#').ok_or_else(|| format!("pad takes a color like #000000, found {s}"))?;
    match matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        true => Ok(hex.to_string()),
        false => Err(format!("pad takes a color like #000000, found {s}")),
    }
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let lower = s.to_ascii_lowercase();
    if let Some((_, size)) = SIZE_NAMES.iter().find(|(name, _)| *name == lower) {
        return Ok(*size);
    }
    let Some((w, h)) = s.split_once('x') else {
        return Err("size must be WIDTHxHEIGHT (e.g. 1920x1080)".into());
    };
    Ok((
        w.parse().map_err(|_| format!("width is not a whole number: {w}"))?,
        h.parse().map_err(|_| format!("height is not a whole number: {h}"))?,
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
        let result = render(&src, bundle::root_of(&sources.main), Some(&sources), &None, name, OutputArgs::parse());
        sources.cleanup();
        return result;
    }
    let cli = Cli::parse();
    // -f が無くても、台本の代わりにフォルダか .yaml を渡されたら、そこの設定を読む
    let named = cli.config.clone().map(PathBuf::from).or_else(|| script_of(&cli.command).and_then(config_at));
    let project = project::Project::load(named.as_deref().and_then(Path::to_str), &cli.set)?.map(Rc::new);
    if let Some(p) = &project {
        p.announce();
    }
    // 走らせているスクリプト。エラーにファイル名を付けるのに使う
    let running = std::cell::RefCell::new(String::new());
    // スクリプトを書かなければ mophila.yaml の entry。どちらも無ければエラー。
    // フォルダや .yaml は台本ではなく設定の指定なので、entry に回す
    let entry = |script: Option<String>| -> Result<String, Box<dyn Error>> {
        let given = match script {
            Some(s) if config_at(&s).is_some() => None,
            Some(s) if Path::new(&s).is_dir() => {
                return Err(format!("{s} has no {} (name a script, or put one there)", project::FILE_NAME).into());
            }
            other => other,
        };
        let path = match (given, project.as_ref().and_then(|p| p.entry.clone())) {
            (Some(s), _) => s,
            (None, Some(e)) => e.to_string_lossy().into_owned(),
            (None, None) => return Err(format!("name a script, or put an entry in {}", project::FILE_NAME).into()),
        };
        *running.borrow_mut() = path.clone();
        Ok(path)
    };
    // 途中の ? でここを飛び越えないように、いったん閉じてから受ける
    let result = (|| -> Result<(), Box<dyn Error>> {
    match cli.command {
        Command::Render { script, mut out } => {
            let script = entry(script)?;
            // -o が無ければ output.mp4 に、--at 付きなら output.png に書く。ウィンドウは preview で
            if out.output.is_none() {
                out.output = Some(if out.at.is_some() { "output.png" } else { "output.mp4" }.to_string());
            }
            render(&read_script(&script)?, base_dir(&script), None, &project, file_name(&script), out)
        }
        Command::Run { script } => {
            let script = entry(script)?;
            let stmts = lang::parser::parse(&read_script(&script)?)?;
            let mut interp = lang::eval::Interp::new();
            interp.base_dir = base_dir(&script);
            if let Some(p) = &project {
                interp.set_project(p.clone());
            }
            interp.run(&stmts)?;
            Ok(())
        }
        Command::Preview { script, size, r#loop, at, crop, align, pad } => {
            let script = entry(script)?;
            let (interp, view, duration) = load(&read_script(&script)?, base_dir(&script), None, &project)?;
            let media = render::media::prepare(&view, duration, 0.0, duration, true, &render::voice::cache_dir())?;
            let shot = render::scene::Shot {
                width: f64::from(size.0),
                height: f64::from(size.1),
                crop,
                align: align.unwrap_or((0.5, 0.5)),
                pad: pad_color(pad.as_deref()),
            };
            render::preview::run(file_name(&script), interp, view, duration, shot, r#loop, at, &media)
        }
        Command::Timeline { script, filter } => {
            let script = entry(script)?;
            let (mut interp, view, duration) = load(&read_script(&script)?, base_dir(&script), None, &project)?;
            let filter = report::Filter::parse(&filter)?;
            let events = report::collect(&mut interp, &view);
            println!("duration: {duration:.2}s\n");
            println!("# events: start  length  object  attribute  change\n{}", report::format_events(&events, &filter));
            println!("\n# text visibility: from – to  length  text\n{}", report::text_visibility(&events, &filter));
            // 動画に入るのと同じものを出す (音声は作らない)
            let media = render::media::prepare(&view, duration, 0.0, duration, false, &render::voice::cache_dir())?;
            if !media.is_empty() {
                println!("\n# subtitles and audio: from – to  length  content\n{}", report::media_report(&media, &filter));
            }
            Ok(())
        }
        Command::Sheet { script, output, every, times, cell, cols } => {
            let script = entry(script)?;
            sheet(&read_script(&script)?, base_dir(&script), &project, &output, every, times, cell, cols)
        }
        Command::Subs { script, output, trim } => {
            let script = entry(script)?;
            let (_, view, duration) = load(&read_script(&script)?, base_dir(&script), None, &project)?;
            let trim = trim.unwrap_or(Trim { from: None, to: None });
            let from = trim.from.unwrap_or(0.0).min(duration);
            let to = trim.to.unwrap_or(duration).min(duration);
            // 動画に入るのと同じものを、同じ道で組む (音声は作らない)
            let media = render::media::prepare(&view, duration, from, to, false, &render::voice::cache_dir())?;
            let cues = &media.cues;
            // 形式は拡張子で決める (render と同じ規則)。書かなければ SRT
            let vtt = output.as_deref().is_some_and(|o| o.to_ascii_lowercase().ends_with(".vtt"));
            let text = match vtt {
                true => render::media::vtt(cues),
                false => render::media::srt(cues),
            };
            match output {
                Some(path) => {
                    std::fs::write(&path, text)?;
                    eprintln!("{} subtitles -> {path}", cues.len());
                }
                None => print!("{text}"),
            }
            Ok(())
        }
        Command::Lsp { .. } => lsp::run(),
        Command::Fonts => {
            let mut cache = render::text::RenderCache::new();
            for name in cache.families() {
                println!("{name}");
            }
            Ok(())
        }
        Command::Doc => {
            println!("{}", serde_json::to_string_pretty(&docs::json())?);
            Ok(())
        }
        Command::Bundle { script, output } => bundle::write(std::path::Path::new(&entry(script)?), &output),
    }
    })();
    // import 先で起きたエラーには "in ./x.moph:" が付くが、書いたファイル自身には付かない。ここで足す
    in_script(&running.borrow(), result)
}

/// スクリプトの実行で起きたエラーに、そのファイル名を足す
fn in_script<T>(script: &str, result: Result<T, Box<dyn Error>>) -> Result<T, Box<dyn Error>> {
    if script.is_empty() {
        return result;
    }
    result.map_err(|e| match e.downcast::<lang::error::MophError>() {
        Ok(m) => Box::new(lang::error::MophError::new(m.kind, format!("in {script}: {}", m.message))) as Box<dyn Error>,
        Err(other) => other,
    })
}

/// タイトルバーに出す名前 (パスと拡張子を除いたファイル名)
fn file_name(script: &str) -> String {
    std::path::Path::new(script).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| script.to_string())
}

/// 帯の色。書かなければ白
fn pad_color(hex: Option<&str>) -> vello::peniko::Color {
    match hex {
        Some(hex) => {
            let [r, g, b, a] = lang::lexer::parse_color(hex);
            vello::peniko::Color::new([r, g, b, a])
        }
        None => vello::peniko::Color::WHITE,
    }
}

/// そのコマンドが受け取ったスクリプトの位置 (無いコマンドもある)
fn script_of(command: &Command) -> Option<&str> {
    match command {
        Command::Render { script, .. }
        | Command::Run { script }
        | Command::Preview { script, .. }
        | Command::Timeline { script, .. }
        | Command::Sheet { script, .. }
        | Command::Subs { script, .. }
        | Command::Bundle { script, .. } => script.as_deref(),
        Command::Lsp { .. } | Command::Fonts | Command::Doc => None,
    }
}

/// そこが設定を指しているなら、その mophila.yaml。
/// フォルダならその中の 1 つ、.yaml / .yml ならそれ自身 (無ければ読むときに言う)
fn config_at(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    if path.is_dir() {
        let file = path.join(project::FILE_NAME);
        return file.exists().then_some(file);
    }
    let yaml = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml"));
    yaml.then(|| path.to_path_buf())
}

/// スクリプトを読む。読めなければ、どのファイルかを言う
fn read_script(path: &str) -> Result<String, Box<dyn Error>> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read \"{path}\": {e}").into())
}

/// スクリプトのあるディレクトリ。import "file" の基準
fn base_dir(script: &str) -> std::path::PathBuf {
    std::path::Path::new(script).parent().map(|d| d.to_path_buf()).unwrap_or_default()
}

/// スクリプトを実行し、出力する View と動画の長さを返す
fn load(src: &str, base_dir: std::path::PathBuf, sources: Option<&bundle::Sources>, project: &Option<Rc<project::Project>>) -> Result<(lang::eval::Interp, lang::value::ObjRef, f64), Box<dyn Error>> {
    let stmts = lang::parser::parse(src)?;
    let mut interp = lang::eval::Interp::new();
    interp.base_dir = base_dir;
    if let Some(p) = project {
        interp.set_project(p.clone());
    }
    if let Some(s) = sources {
        interp.sources = s.files.clone();
        interp.assets = s.assets.clone();
    }
    interp.run(&stmts)?;
    let view = interp.output.clone().ok_or("no output: add \"output <view>\" to the script")?;
    let duration = lang::eval::all_tracks(&view).iter().map(|p| p.end()).fold(0.0, f64::max);
    interp.snapshot(&view);
    Ok((interp, view, duration))
}

/// 置かれた読み上げを音声にして混ぜる。voice: を書いていなければ、書いてくれと言う
fn render(src: &str, base_dir: std::path::PathBuf, sources: Option<&bundle::Sources>, project: &Option<Rc<project::Project>>, name: String, args: OutputArgs) -> Result<(), Box<dyn Error>> {
    let (width, height) = args.size;
    // 箱のどこを、どう寄せて出すか
    let shot = render::scene::Shot {
        width: f64::from(width),
        height: f64::from(height),
        crop: args.crop,
        align: args.align.unwrap_or((0.5, 0.5)),
        pad: pad_color(args.pad.as_deref()),
    };
    let (mut interp, view, duration) = load(src, base_dir, sources, project)?;
    let cache = render::voice::cache_dir();
    let Some(output) = args.output else {
        let media = render::media::prepare(&view, duration, 0.0, duration, true, &cache)?;
        return render::preview::run(name, interp, view, duration, shot, args.r#loop, args.at, &media);
    };

    let mut timing = timing::Timing::from_env();
    let mut renderer = timing.measure("startup", || render::gpu::HeadlessRenderer::new(width, height, shot.pad))?;
    interp.cache_mut().shaders = Some(renderer.shader_runner());
    // 拡張子で出力の形式を決める。--codec / --pix-fmt を書けばそれが勝つ
    let Some(format) = render::encode::format_of(&output) else {
        return Err(format!("cannot write \"{output}\": use .mp4 .mov .mkv .webm .gif .apng, or .png .jpg .webp .tiff with --at").into());
    };
    let is_image = format.image;
    // --trim の区間。映像はこの時刻から描き、音声と字幕もこの区間に合わせてずらして切る
    let trim = args.trim.unwrap_or(Trim { from: None, to: None });
    let from = trim.from.unwrap_or(0.0).min(duration);
    let to = trim.to.unwrap_or(duration).min(duration);
    if !is_image && to <= from {
        return Err(format!("--trim starts at {from}s but the video ends at {duration}s").into());
    }
    let media = render::media::prepare(&view, duration, from, to, true, &cache)?;
    let mut ffmpeg = render::encode::Ffmpeg::spawn(
        &output,
        render::encode::Settings {
            width,
            height,
            fps: args.fps,
            codec: args.codec.as_deref().unwrap_or(format.codec),
            pix_fmt: args.pix_fmt.as_deref().unwrap_or(format.pix_fmt),
            media: if format.media { Some((&media, to - from)) } else { None },
            filter: format.filter,
        },
    )?;

    let mut pixels = Vec::new();
    let times: Vec<f64> = if is_image {
        vec![args.at.ok_or("--at is required for image output")?]
    } else {
        let frames = ((to - from) * f64::from(args.fps)).round() as u32;
        (0..frames).map(|f| from + f64::from(f) / f64::from(args.fps)).collect()
    };
    // GPU には 2 フレームまで投入しておき、次を投入する前に古い方を読み戻す。
    // 待ちの間に次のフレームが GPU に入っているので、読み戻しで止まらない。
    // ただし Shader を使う絵では、compute と描画が GPU を取り合って遅くなるので 1 枚ずつにする
    let depth = if render::scene::uses_shader(&view) { 1 } else { 2 };
    let mut pending: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    // 置いたものは描いている間に増えないので、1 度集めて使い回す
    let tracks = lang::eval::all_tracks(&view);
    let mut progress = render::progress::Progress::new(&name, times.len());
    // Shader の塗りは描画命令を組む時点で GPU を使うので、別スレッドでは組めない
    let workers = match render::scene::uses_shader(&view) {
        true => 1,
        false => match args.jobs {
            0 => std::thread::available_parallelism().map_or(1, |n| n.get().min(4)).max(1),
            n => n.max(1),
        },
    };
    let mut frames = (workers > 1 && times.len() > 1).then(|| {
        render::frames::Frames::start(
            &render::frames::Job {
                src: src.to_string(),
                base_dir: interp.base_dir.clone(),
                sources: sources.cloned(),
                project: project.as_ref().map(|p| (**p).clone()),
                shot,
            },
            &times,
            workers,
        )
    });
    // 読み戻しのバッファ 2 枚は、コマによらず要る
    let readback = 2 * u64::from((width * 4).next_multiple_of(256)) * u64::from(height);
    let mut budget = render::budget::Budget::new(args.gpu_budget, readback);
    for (i, t) in times.iter().copied().enumerate() {
        let (scene, bytes) = match &mut frames {
            // 別スレッドが組んだもの。配った順に届く
            Some(frames) => timing.measure("frame", || -> Result<(vello::Scene, render::text::Bytes), Box<dyn Error>> {
                match frames.recv() {
                    Some(Ok((n, scene, bytes))) if n == i => Ok((scene, bytes)),
                    Some(Ok((n, ..))) => Err(format!("frames came back out of order (got {n} where {i} was expected)").into()),
                    Some(Err(e)) => Err(e.into()),
                    None => Err("a frame worker died".into()),
                }
            })?,
            None => {
                timing.measure("eval", || -> Result<(), Box<dyn Error>> {
                    interp.begin_frame(t);
                    interp.apply_tracks(&tracks, t)?;
                    Ok(())
                })?;
                let scene = timing.measure("scene", || render::scene::build(&view, shot, t, interp.cache_mut()))?;
                (scene, interp.cache_mut().bytes())
            }
        };
        budget.see(t, bytes);
        // 読み戻しは、次のフレームを組んだ後に行う (GPU が描いている間に CPU が組み立てる)
        if pending.len() >= depth {
            let prev = pending.pop_front().expect("a frame is waiting");
            timing.measure("readback", || renderer.read_pixels(prev, &mut pixels))?;
            timing.measure("encode", || ffmpeg.write_frame(&pixels))?;
        }
        pending.push_back(timing.measure("render", || renderer.render(&scene, interp.cache_mut().shaders.as_mut()))?);
        progress.step(i + 1);
    }
    while let Some(prev) = pending.pop_front() {
        timing.measure("readback", || renderer.read_pixels(prev, &mut pixels))?;
        timing.measure("encode", || ffmpeg.write_frame(&pixels))?;
    }
    timing.measure("finish", || ffmpeg.finish())?;
    progress.finish();
    budget.report();

    timing.report();
    Ok(())
}

/// 指定の時刻でフレームを描き、格子に並べて PNG に書く。各コマの左上に時刻を入れる
fn sheet(src: &str, base_dir: std::path::PathBuf, project: &Option<Rc<project::Project>>, output: &str, every: f64, times: Vec<f64>, cell: (u32, u32), cols: u32) -> Result<(), Box<dyn Error>> {
    let (mut interp, view, duration) = load(src, base_dir, None, project)?;
    // 動画と同じ字幕を重ねる (音声は要らないので作らない)
    let media = render::media::prepare(&view, duration, 0.0, duration, false, &render::voice::cache_dir())?;
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

    let mut renderer = render::gpu::HeadlessRenderer::new(cw, ch, vello::peniko::Color::WHITE)?;
    interp.cache_mut().shaders = Some(renderer.shader_runner());
    let mut pixels = Vec::new();
    let mut canvas = vec![0u8; (width * height * 4) as usize];
    let mut progress = render::progress::Progress::new("sheet", times.len());
    for (i, &t) in times.iter().enumerate() {
        interp.begin_frame(t);
        let tracks = lang::eval::all_tracks(&view);
        interp.apply_tracks(&tracks, t)?;
        let cell_shot = render::scene::Shot::whole(f64::from(cw), f64::from(ch));
        let mut scene = render::scene::build(&view, cell_shot, t, interp.cache_mut())?;
        let area = render::scene::picture(&view, cell_shot)?;
        render::scene::overlay_subtitles(&mut scene, interp.cache_mut(), &media.cues, t, area);
        // 時刻のラベル
        let label = format!("{t:.1}s");
        let layout = interp.cache_mut().layout(&label, None, (f64::from(ch) * 0.09) as f32, None, render::text::alignment(None));
        let (lw, lh) = (f64::from(layout.width()) + 8.0, f64::from(layout.height()) + 4.0);
        scene.fill(vello::peniko::Fill::NonZero, vello::kurbo::Affine::IDENTITY, vello::peniko::Color::from_rgba8(0, 0, 0, 160), None, &vello::kurbo::Rect::new(0.0, 0.0, lw, lh));
        render::text::draw(&mut scene, layout, vello::kurbo::Affine::translate((4.0, 2.0)), vello::peniko::Color::WHITE, None);
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
    let mut ffmpeg = render::encode::Ffmpeg::spawn(output, render::encode::Settings { width, height, fps: 1, codec: "png", pix_fmt: "rgba", media: None, filter: None })?;
    ffmpeg.write_frame(&canvas)?;
    ffmpeg.finish()?;
    Ok(())
}
