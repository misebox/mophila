use std::error::Error;
use std::num::NonZeroUsize;

use vello::peniko::Color;
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene, wgpu};

use crate::render::shader::{self, ShaderRunner};

/// ウィンドウなしで Scene を描画し、ピクセルを CPU に読み戻す。
/// 読み戻し用バッファを 2 つ持ち、フレーム N の読み戻しを待つ間にフレーム N+1 を描画できる
pub struct HeadlessRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: [wgpu::Buffer; 2],
    /// 各バッファへコピーした submit の番号。読み戻し待ちに使う
    submitted: [Option<wgpu::SubmissionIndex>; 2],
    next: usize,
    width: u32,
    height: u32,
    padded_bytes_per_row: u32,
}

impl HeadlessRenderer {
    pub fn new(width: u32, height: u32) -> Result<Self, Box<dyn Error>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;

        let renderer = Renderer::new(
            &device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: AaSupport::area_only(),
                num_init_threads: NonZeroUsize::new(1),
                pipeline_cache: None,
            },
        )?;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mophila target"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let padded_bytes_per_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = std::array::from_fn(|i| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(if i == 0 { "mophila readback 0" } else { "mophila readback 1" }),
                size: u64::from(padded_bytes_per_row) * u64::from(height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        });

        Ok(Self { device, queue, renderer, texture, view, readback, submitted: [None, None], next: 0, width, height, padded_bytes_per_row })
    }

    /// Shader を走らせるもの。同じ装置を使う
    pub fn shader_runner(&self) -> ShaderRunner {
        ShaderRunner::new(self.device.clone(), self.queue.clone())
    }

    /// Scene を描画し、ピクセルを読み戻し用バッファへコピーする命令を発行する。
    /// 戻り値はバッファの番号で、read_pixels に渡す。同じ番号を読み戻す前に 2 回描くことはできない。
    /// shaders はこのフレームで Shader を計算したもの (画像の登録に使う)
    pub fn render(&mut self, scene: &Scene, shaders: Option<&mut ShaderRunner>) -> Result<usize, Box<dyn Error>> {
        let slot = self.next;
        if self.submitted[slot].is_some() {
            return Err("frame not read back before reusing its buffer".into());
        }
        self.next = (slot + 1) % 2;
        shader::apply_overrides(&mut self.renderer, shaders);
        let params = RenderParams {
            base_color: Color::WHITE,
            width: self.width,
            height: self.height,
            antialiasing_method: AaConfig::Area,
        };
        self.renderer.render_to_texture(&self.device, &self.queue, scene, &self.view, &params)?;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mophila copy") });
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback[slot],
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_bytes_per_row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        let index = self.queue.submit([encoder.finish()]);
        self.submitted[slot] = Some(index);
        Ok(slot)
    }

    /// render が返した番号のフレームを待ち、RGBA8 のピクセル列を out に書く。行のパディングは除去する。
    /// その submit までを待つので、後から投入したフレームの完了は待たない
    pub fn read_pixels(&mut self, slot: usize, out: &mut Vec<u8>) -> Result<(), Box<dyn Error>> {
        let index = self.submitted[slot].take().ok_or("nothing rendered into this buffer")?;
        let slice = self.readback[slot].slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(wgpu::PollType::Wait { submission_index: Some(index), timeout: None })?;
        rx.recv()??;

        let row_bytes = (self.width * 4) as usize;
        let padded = self.padded_bytes_per_row as usize;
        out.resize(row_bytes * self.height as usize, 0);
        {
            let data = slice.get_mapped_range();
            for (dst, src) in out.chunks_exact_mut(row_bytes).zip(data.chunks_exact(padded)) {
                dst.copy_from_slice(&src[..row_bytes]);
            }
        }
        self.readback[slot].unmap();
        Ok(())
    }
}
