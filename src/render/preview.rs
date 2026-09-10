//! ウィンドウを開いて実時間で再生する。描画が間に合わなければコマは飛ぶ。
//!
//! 操作: Space で一時停止・再開。← → または h l で 10 秒移動 (一時停止中は 1 秒)。Shift を押しながらで全体の 10%。
//! 最後まで再生したら最後の場面で止まる (--loop なら先頭に戻る)。閉じるのはウィンドウを閉じる。
//! --at で時刻を指定すると、その時刻の画面を一時停止で出す。
//! 音声は audio が出力デバイスに流す。字幕は画面の下に重ねる

use std::error::Error;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Instant;

use vello::peniko::Color;
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

pub fn run(name: String, interp: Interp, view: ObjRef, duration: f64, size: (u32, u32), looping: bool, start: Option<f64>, media: &Media) -> Result<(), Box<dyn Error>> {
    // 音が出せなくても再生はする
    let audio = if media.clips.is_empty() {
        None
    } else {
        audio::Output::open(media).map_err(|e| eprintln!("audio disabled: {e}")).ok()
    };
    let mut player = Player {
        name,
        interp,
        view,
        duration,
        size,
        looping,
        state: None,
        position: start.unwrap_or(0.0).clamp(0.0, duration.max(0.0)),
        playing: start.is_none(),
        last_tick: Instant::now(),
        shift: false,
        error: None,
        audio,
        cues: media.cues.clone(),
    };
    EventLoop::new()?.run_app(&mut player)?;
    match player.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

struct Player<'a> {
    /// タイトルバーに出す名前。スクリプト名か、埋め込みバイナリの名前
    name: String,
    interp: Interp,
    view: ObjRef,
    duration: f64,
    size: (u32, u32),
    looping: bool,
    state: Option<State<'a>>,
    /// 再生位置 (秒)
    position: f64,
    playing: bool,
    /// 前回の描画時刻。再生中はここからの経過分だけ position を進める
    last_tick: Instant,
    shift: bool,
    error: Option<Box<dyn Error>>,
    audio: Option<audio::Output>,
    cues: Vec<Cue>,
}

struct State<'a> {
    window: Arc<Window>,
    context: RenderContext,
    surface: RenderSurface<'a>,
    renderer: Renderer,
}

impl Player<'_> {
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let attrs = Window::default_attributes()
            .with_title(&self.name)
            .with_inner_size(LogicalSize::new(self.size.0, self.size.1));
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
        self.state = Some(State { window, context, surface, renderer });
        self.last_tick = Instant::now();
        Ok(())
    }

    /// 再生中なら経過時間の分だけ位置を進める。終端に着いたら、--loop なら先頭へ、そうでなければ止まる
    fn advance(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;
        if !self.playing || self.duration <= 0.0 {
            return;
        }
        self.position += dt;
        if self.position < self.duration {
            return;
        }
        if self.looping {
            self.position %= self.duration;
        } else {
            self.position = self.duration;
            self.playing = false;
        }
    }

    fn seek(&mut self, delta: f64) {
        self.position = (self.position + delta).clamp(0.0, self.duration.max(0.0));
        self.last_tick = Instant::now();
        self.sync_audio(true);
        if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }

    fn toggle_pause(&mut self) {
        if !self.playing && self.duration > 0.0 && self.position >= self.duration {
            self.position = 0.0;
        }
        self.playing = !self.playing;
        self.last_tick = Instant::now();
        self.sync_audio(true);
        if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }

    fn sync_audio(&self, force: bool) {
        if let Some(a) = &self.audio {
            a.sync(self.position, self.playing, force);
        }
    }

    /// キー操作。処理したら true
    fn key(&mut self, event: &KeyEvent) -> bool {
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
            _ => return false,
        }
        true
    }

    /// 1 フレーム描く
    fn frame(&mut self) -> Result<(), Box<dyn Error>> {
        self.advance();
        self.sync_audio(false);
        let t = self.position;
        let Some(state) = &mut self.state else { return Ok(()) };
        let percent = if self.duration > 0.0 { t / self.duration * 100.0 } else { 0.0 };
        state.window.set_title(&format!(
            "{}  {:.1}s / {:.1}s  {:.0}%{}",
            self.name,
            t,
            self.duration,
            percent,
            if self.playing { "" } else { "  (paused)" }
        ));

        self.interp.begin_frame(t);
        let tracks = crate::lang::eval::all_tracks(&self.view);
        for placed in &tracks {
            self.interp.apply_track(placed, t)?;
        }
        let (width, height) = (state.surface.config.width, state.surface.config.height);
        let mut scene = scene::build(&self.view, f64::from(width), f64::from(height), t, self.interp.cache_mut())?;
        scene::overlay_subtitles(&mut scene, self.interp.cache_mut(), &self.cues, t, f64::from(width), f64::from(height));

        let handle = &state.context.devices[state.surface.dev_id];
        shader::apply_overrides(&mut state.renderer, self.interp.cache_mut().shaders.as_mut());
        let params = RenderParams { base_color: Color::WHITE, width, height, antialiasing_method: AaConfig::Area };
        state.renderer.render_to_texture(&handle.device, &handle.queue, &scene, &state.surface.target_view, &params)?;

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
        state.surface.blitter.copy(&handle.device, &mut encoder, &state.surface.target_view, &target);
        handle.queue.submit([encoder.finish()]);
        frame.present();
        // 止まっているときは次の描画を求めない (seek や再開で求める)
        if self.playing {
            state.window.request_redraw();
        }
        Ok(())
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
            WindowEvent::Resized(size) => {
                if let Some(state) = &mut self.state {
                    state.context.resize_surface(&mut state.surface, size.width, size.height);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.shift = modifiers.state().shift_key(),
            WindowEvent::KeyboardInput { event, .. } => {
                self.key(&event);
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
