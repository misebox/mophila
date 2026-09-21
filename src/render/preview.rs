//! ウィンドウを開いて再生する。コマは少し先まで描いておき、順に出す。
//!
//! 描くのが実時間に間に合わないときは、コマを飛ばさずに**出せる速さで**出す (絵はゆっくりになるが跳ねない)。
//! 音があるときだけは音が親時計なので、遅れたぶんは飛ばして追いつく。
//!
//! 操作: Space で一時停止・再開。← → または h l で 10 秒移動 (一時停止中は 1 秒)。Shift を押しながらで全体の 10%。
//! s でステータスバーの出し入れ、q で終了。
//! 最後まで再生したら最後の場面で止まる (--loop なら先頭に戻る)。
//! --at で時刻を指定すると、その時刻の画面を一時停止で出す。
//! 音声は audio が出力デバイスに流す。字幕とステータスバーは絵の中に重ねる

use std::collections::VecDeque;
use std::error::Error;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Instant;

use vello::util::{RenderContext, RenderSurface};
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, wgpu};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::render::audio;
use crate::lang::eval::Interp;
use crate::render::shader::{self, ShaderRunner};
use crate::render::media::{Cue, Media};
use crate::render::scene;
use crate::lang::value::ObjRef;

/// 先に描いておくコマの数。多いほど重いコマを吸収できるが、キー操作の効きが遅れる
const LOOKAHEAD: usize = 4;
/// 1 回の描画で作り足すコマの数。作りすぎると、いま出すコマがそのぶん遅れる
const PER_TURN: usize = 2;
/// 中身を進める刻みの目安。画面の更新がこれより速ければ、同じコマを何回か出して合わせる
const TARGET_STEP: f64 = 1.0 / 60.0;

pub fn run(name: String, interp: Interp, view: ObjRef, duration: f64, shot: crate::render::scene::Shot, looping: bool, start: Option<f64>, media: &Media) -> Result<(), Box<dyn Error>> {
    let size = (shot.width as u32, shot.height as u32);
    // 音が出せなくても再生はする
    let audio = if media.clips.is_empty() {
        None
    } else {
        audio::Output::open(media).map_err(|e| eprintln!("audio disabled: {e}")).ok()
    };
    let position = start.unwrap_or(0.0).clamp(0.0, duration.max(0.0));
    let mut player = Player {
        shot,
        name,
        interp,
        view,
        duration,
        size,
        looping,
        state: None,
        position,
        next_t: position,
        playing: start.is_none(),
        refresh: TARGET_STEP,
        step: TARGET_STEP,
        holds: 1,
        waited: 0,
        last_tick: Instant::now(),
        last_content: Instant::now(),
        shift: false,
        error: None,
        audio,
        anchor: Instant::now(),
        anchor_t: position,
        cues: media.cues.clone(),
        ready: VecDeque::new(),
        spare: Vec::new(),
        shown: None,
        status: true,
        fps: 0.0,
        last_note: Instant::now(),
        timing: crate::timing::Timing::from_env(),
        starved: 0,
    };
    EventLoop::new()?.run_app(&mut player)?;
    player.timing.report();
    if player.starved > 0 {
        eprintln!(
            "preview: {} frames were not drawn in time, so the picture ran slower than real time (what render writes does not change)",
            player.starved
        );
    }
    match player.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// 描き終わって、まだ出していないコマ
struct Ready {
    /// 使い回すので持っておく。view はここから作ったもの
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// その絵の中身の時刻 (秒)
    t: f64,
}

struct Player<'a> {
    /// タイトルバーに出す名前。スクリプト名か、埋め込みバイナリの名前
    name: String,
    interp: Interp,
    view: ObjRef,
    duration: f64,
    size: (u32, u32),
    /// 箱のどこを出すか (ウィンドウの大きさは毎フレーム入れ替える)
    shot: crate::render::scene::Shot,
    looping: bool,
    state: Option<State<'a>>,
    /// いま出しているコマの時刻 (秒)
    position: f64,
    /// 次に描くコマの時刻 (秒)。出しているコマより先に進んでいる
    next_t: f64,
    playing: bool,
    /// 実測した画面の更新間隔 (秒)
    refresh: f64,
    /// 中身を 1 コマ進める幅 (秒)。refresh * holds
    step: f64,
    /// 1 コマを何回の更新に渡って出すか。120Hz の画面なら 2
    holds: usize,
    /// いまのコマを出してからの更新の回数
    waited: usize,
    /// 前に描いた時刻。画面の更新間隔を測るのに使う
    last_tick: Instant,
    /// 前にコマを入れ替えた時刻。fps を出すのに使う
    last_content: Instant,
    shift: bool,
    error: Option<Box<dyn Error>>,
    audio: Option<audio::Output>,
    /// 音に合わせるための基準。この時刻に anchor_t を出していた
    anchor: Instant,
    anchor_t: f64,
    cues: Vec<Cue>,
    /// 描き終わって順番を待っているコマ
    ready: VecDeque<Ready>,
    /// 使い回すテクスチャ。ウィンドウの大きさが変わったら捨てる
    spare: Vec<Ready>,
    /// いまウィンドウに出しているコマ
    shown: Option<Ready>,
    /// ステータスバーを絵に重ねるか。s で切り替える
    status: bool,
    /// ならした fps。ステータスバーに出す
    fps: f64,
    /// MOPHILA_TIMING のときに、進み具合を知らせた時刻
    last_note: Instant,
    /// MOPHILA_TIMING で段階ごとの時間を測る
    timing: crate::timing::Timing,
    /// 出す番が来たのに描き終わっていなかった回数
    starved: usize,
}

struct State<'a> {
    window: Arc<Window>,
    context: RenderContext,
    surface: RenderSurface<'a>,
    renderer: Renderer,
}

/// Vello が描き込むテクスチャ。ウィンドウへは blit で写す
fn new_target(device: &wgpu::Device, width: u32, height: u32) -> Ready {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mophila preview frame"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        format: wgpu::TextureFormat::Rgba8Unorm,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Ready { _texture: texture, view, t: 0.0 }
}

impl Player<'_> {
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        // --size をそのまま出すと、画面に入らないときに OS が片方だけ詰めて比が崩れる。
        // 入らない分は縦横を同じだけ縮めて、指定した比のまま開く
        let (mut w, mut h) = (f64::from(self.size.0), f64::from(self.size.1));
        if let Some(screen) = event_loop.primary_monitor().or_else(|| event_loop.available_monitors().next()) {
            let area = screen.size().to_logical::<f64>(screen.scale_factor());
            // 上のバーやドックのぶんを見て、画面の 9 割に収める
            let room = (area.width * 0.9 / w).min(area.height * 0.9 / h).min(1.0);
            w *= room;
            h *= room;
            // 更新間隔の初期値。実際の間隔は出しながら測り直す
            if let Some(mhz) = screen.refresh_rate_millihertz() {
                self.tick_is(1000.0 / f64::from(mhz));
            }
        }
        let attrs = Window::default_attributes()
            .with_title(&self.name)
            .with_inner_size(LogicalSize::new(w, h));
        let window = Arc::new(event_loop.create_window(attrs)?);
        let inner = window.inner_size();
        let mut context = RenderContext::new();
        let surface = pollster::block_on(context.create_surface(window.clone(), inner.width, inner.height, wgpu::PresentMode::AutoVsync))?;
        let device = &context.devices[surface.dev_id].device;
        let renderer = Renderer::new(
            device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: AaSupport::area_only(),
                num_init_threads: NonZeroUsize::new(1),
                pipeline_cache: None,
            },
        )?;
        let handle = &context.devices[surface.dev_id];
        self.interp.cache_mut().shaders = Some(ShaderRunner::new(handle.device.clone(), handle.queue.clone()));
        if self.timing.on() {
            let (w, h) = (inner.width, inner.height);
            let ratio = f64::from(w) * f64::from(h) / (f64::from(self.size.0) * f64::from(self.size.1)).max(1.0);
            eprintln!("preview: drawing {w}x{h} ({ratio:.2}x the pixels of --size {}x{})", self.size.0, self.size.1);
        }
        self.state = Some(State { window, context, surface, renderer });
        Ok(())
    }

    /// 画面の更新間隔が分かったら、中身の刻みを決め直す。
    /// 画面のほうが速ければ、同じコマを複数回出して実時間に合わせる (毎回進めると速く流れてしまう)
    fn tick_is(&mut self, refresh: f64) {
        self.refresh = refresh;
        self.holds = ((TARGET_STEP / refresh).round() as usize).max(1);
        self.step = refresh * self.holds as f64;
    }

    /// 作り置きを捨てて、いまの位置から描き直す (seek と一時停止の切り替え)
    fn restart(&mut self) {
        self.spare.extend(self.ready.drain(..));
        if let Some(shown) = self.shown.take() {
            self.spare.push(shown);
        }
        self.next_t = self.position;
        self.waited = 0;
        self.anchor = Instant::now();
        self.anchor_t = self.position;
        if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }

    fn seek(&mut self, delta: f64) {
        self.position = (self.position + delta).clamp(0.0, self.duration.max(0.0));
        self.sync_audio();
        self.restart();
    }

    fn toggle_pause(&mut self) {
        if !self.playing && self.duration > 0.0 && self.position >= self.duration {
            self.position = 0.0;
        }
        self.playing = !self.playing;
        self.sync_audio();
        self.restart();
    }

    /// 音は別スレッドで実時間のまま流す。位置を渡すのは seek と一時停止のときだけで、
    /// 再生中に渡すと、作り置きのぶん遅れている映像の位置へ音が引き戻される
    fn sync_audio(&self) {
        if let Some(a) = &self.audio {
            a.sync(self.position, self.playing, true);
        }
    }

    /// キー操作。処理したら true
    fn key(&mut self, event_loop: &ActiveEventLoop, event: &KeyEvent) -> bool {
        if event.state != ElementState::Pressed {
            return false;
        }
        let step = if self.shift {
            self.duration * 0.1
        } else if self.playing {
            10.0
        } else {
            1.0
        };
        match &event.logical_key {
            Key::Named(NamedKey::Space) => self.toggle_pause(),
            Key::Named(NamedKey::ArrowLeft) => self.seek(-step),
            Key::Named(NamedKey::ArrowRight) => self.seek(step),
            Key::Character(c) if c.eq_ignore_ascii_case("h") => self.seek(-step),
            Key::Character(c) if c.eq_ignore_ascii_case("l") => self.seek(step),
            Key::Character(c) if c.eq_ignore_ascii_case("s") => {
                self.status = !self.status;
                self.restart();
            }
            Key::Character(c) if c.eq_ignore_ascii_case("q") => event_loop.exit(),
            _ => return false,
        }
        true
    }

    /// 1 回の描画。先に何コマか作り、出す番のコマをウィンドウに出す
    fn frame(&mut self) -> Result<(), Box<dyn Error>> {
        self.fill()?;
        self.pick();
        self.show()?;
        // 再生中は次を求め続ける。止まっていても、まだ何も出していなければ 1 枚出すまで続ける
        if let Some(state) = &self.state {
            if self.playing || self.shown.is_none() {
                state.window.request_redraw();
            }
        }
        Ok(())
    }

    /// 作り置きを補う。ウィンドウの空きを待つ前に作るので、待っている間が無駄にならない
    fn fill(&mut self) -> Result<(), Box<dyn Error>> {
        let want = if self.playing { LOOKAHEAD } else { 1 };
        for _ in 0..PER_TURN {
            if self.ready.len() >= want || self.state.is_none() {
                return Ok(());
            }
            if self.next_t > self.duration {
                if !self.looping {
                    return Ok(());
                }
                self.next_t = 0.0;
            }
            let t = self.next_t;
            self.draw(t)?;
            self.next_t = t + self.step;
        }
        Ok(())
    }

    /// 1 コマ描いて作り置きに積む
    fn draw(&mut self, t: f64) -> Result<(), Box<dyn Error>> {
        self.timing.at(t);
        let step = Instant::now();
        self.interp.begin_frame(t);
        let tracks = crate::lang::eval::all_tracks(&self.view);
        self.interp.apply_tracks(&tracks, t)?;
        self.timing.add("eval", step.elapsed());

        let status = self.status.then(|| self.status_line(t));
        let Some(state) = &mut self.state else { return Ok(()) };
        let (width, height) = (state.surface.config.width, state.surface.config.height);
        let step = Instant::now();
        let shot = scene::Shot { width: f64::from(width), height: f64::from(height), ..self.shot };
        let mut scene = scene::build(&self.view, shot, t, self.interp.cache_mut())?;
        // 字幕は絵の中に出す。ウィンドウの縦横比が違うと、絵の上下左右に帯が空いている
        let area = scene::picture(&self.view, shot)?;
        scene::overlay_subtitles(&mut scene, self.interp.cache_mut(), &self.cues, t, area);
        if let Some(line) = &status {
            scene::overlay_status(&mut scene, self.interp.cache_mut(), line, area);
        }
        self.timing.add("scene", step.elapsed());

        let step = Instant::now();
        let handle = &state.context.devices[state.surface.dev_id];
        let mut target = match self.spare.pop() {
            Some(target) => target,
            None => new_target(&handle.device, width, height),
        };
        target.t = t;
        shader::apply_overrides(&mut state.renderer, self.interp.cache_mut().shaders.as_mut());
        let params = RenderParams { base_color: self.shot.pad, width, height, antialiasing_method: AaConfig::Area };
        state.renderer.render_to_texture(&handle.device, &handle.queue, &scene, &target.view, &params)?;
        self.timing.add("render", step.elapsed());
        self.ready.push_back(target);
        Ok(())
    }

    /// 出すコマを決める。画面の更新に合わせて進め、holds 回に 1 度だけ入れ替える
    fn pick(&mut self) {
        let now = Instant::now();
        let gap = now.duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;
        // 画面の更新間隔。詰まって伸びた回は混ぜない
        if !self.ready.is_empty() && (1.0 / 300.0..=1.0 / 20.0).contains(&gap) {
            self.tick_is(self.refresh * 0.9 + gap * 0.1);
        }
        // まだ 1 コマも出していないなら、できた先から出す (開いた直後と seek の後)
        if self.shown.is_none() {
            if let Some(next) = self.ready.pop_front() {
                (self.position, self.anchor, self.anchor_t) = (next.t, now, next.t);
                self.shown = Some(next);
                self.waited = 0;
            }
            return;
        }
        if !self.playing {
            return;
        }
        self.waited += 1;
        if self.waited < self.holds {
            return;
        }
        if self.ready.is_empty() {
            // 出す番が来たのに描けていない。飛ばさずに待つので、絵はそのぶんゆっくりになる
            if !self.done() {
                self.starved += 1;
                if self.timing.on() && self.starved <= 20 {
                    let last = if self.starved == 20 { "  (no more of these)" } else { "" };
                    eprintln!("preview: {:.2}s was not ready in time{last}", self.next_t);
                }
            }
            return;
        }
        self.waited = 0;
        // 音があるときは実時間が親時計。遅れているぶんは捨てて追いつく (音には触らない)
        let mut take = 1;
        if self.audio.is_some() {
            let want = self.anchor_t + now.duration_since(self.anchor).as_secs_f64();
            while take < self.ready.len() && self.ready[take - 1].t + self.step < want {
                take += 1;
            }
        }
        for _ in 0..take {
            if let Some(next) = self.ready.pop_front() {
                self.position = next.t;
                if let Some(old) = self.shown.replace(next) {
                    self.spare.push(old);
                }
            }
        }
        let content = now.duration_since(self.last_content).as_secs_f64();
        self.last_content = now;
        if content > 0.0 {
            let rate = take as f64 / content;
            self.fps = match self.fps {
                0.0 => rate,
                before => before * 0.9 + rate * 0.1,
            };
        }
        if self.done() {
            self.playing = false;
            self.sync_audio();
        }
        // 進み具合を、ときどき 1 行だけ
        if self.timing.on() && now.duration_since(self.last_note).as_secs_f64() >= 2.0 {
            self.last_note = now;
            eprintln!("preview: {:.2}s  {:.0}fps  {:.2}x real time", self.position, self.fps, self.fps * self.step);
        }
    }

    /// ステータスバーの 1 行
    fn status_line(&self, t: f64) -> String {
        let percent = if self.duration > 0.0 { t / self.duration * 100.0 } else { 0.0 };
        let state = match self.playing {
            true => format!("{:.0}fps", self.fps),
            false => "paused".to_string(),
        };
        format!("{t:.1}s / {:.1}s   {percent:.0}%   {state}", self.duration)
    }

    /// 最後まで出し終わったか
    fn done(&self) -> bool {
        !self.looping && self.next_t > self.duration && self.ready.is_empty()
    }

    /// いま出しているコマをウィンドウに出す
    fn show(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(state) = &mut self.state else { return Ok(()) };
        let Some(shown) = &self.shown else { return Ok(()) };
        let step = Instant::now();
        let handle = &state.context.devices[state.surface.dev_id];
        let frame = match state.surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                state.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                let size = state.window.inner_size();
                state.context.resize_surface(&mut state.surface, size.width, size.height);
                state.window.request_redraw();
                return Ok(());
            }
            other => return Err(format!("surface error: {other:?}").into()),
        };
        let mut encoder = handle.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mophila blit") });
        let target = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        state.surface.blitter.copy(&handle.device, &mut encoder, &shown.view, &target);
        handle.queue.submit([encoder.finish()]);
        frame.present();
        self.timing.add("present", step.elapsed());
        Ok(())
    }

    /// ウィンドウの大きさが変わったら、作り置きのテクスチャは捨てて描き直す
    fn resized(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        let Some(state) = &mut self.state else { return };
        state.context.resize_surface(&mut state.surface, size.width, size.height);
        self.ready.clear();
        self.spare.clear();
        self.shown = None;
        self.next_t = self.position;
        self.waited = 0;
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, e: Box<dyn Error>) {
        self.error = Some(e);
        event_loop.exit();
    }
}

impl ApplicationHandler for Player<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none() {
            if let Err(e) = self.open(event_loop) {
                self.fail(event_loop, e);
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.resized(size),
            WindowEvent::ModifiersChanged(modifiers) => self.shift = modifiers.state().shift_key(),
            WindowEvent::KeyboardInput { event, .. } => {
                self.key(event_loop, &event);
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.frame() {
                    self.fail(event_loop, e);
                }
            }
            _ => {}
        }
    }
}
