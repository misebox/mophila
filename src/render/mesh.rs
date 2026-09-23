//! Mesh を GPU で描く。面ごとに図形を作らず、頂点をそのまま送って深度バッファで前後を決める。
//!
//! 投影は space3d の `Camera::spot` と同じ式を頂点シェーダに写したもの。箱の座標は
//!   bx = center.x + vx * k,  by = center.y - vy * k    (k は奥行きでの縮尺)
//! で、そこから描き先のピクセル、さらにクリップ座標へ直す。透視のときは w を奥行きにしてあるので、
//! 面の内側の補間も正しく効く。

use vello::wgpu;

/// 頂点。面ごとに分けて持つので、法線は面のもの (平らに塗れる)
#[derive(Clone, Copy)]
pub struct Vertex {
    pub at: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

/// 頂点 1 つぶんのバイト数
pub const VERTEX_BYTES: usize = 40;

/// 頂点の並びを、そのまま GPU へ送れるバイト列にする
pub fn bytes_of(vertices: &[Vertex]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vertices.len() * VERTEX_BYTES);
    for v in vertices {
        for f in v.at.iter().chain(&v.normal).chain(&v.color) {
            out.extend_from_slice(&f.to_le_bytes());
        }
    }
    out
}

/// 頂点シェーダに渡すもの。space3d のカメラをそのまま数にしたもの
#[derive(Clone, Copy, Default)]
pub struct View3 {
    pub eye: [f32; 3],
    /// 透視なら焦点距離。平行なら 0
    pub focal: f32,
    pub right: [f32; 3],
    /// 平行のときの縮尺。透視なら 0
    pub unit: f32,
    pub up: [f32; 3],
    pub near: f32,
    pub fwd: [f32; 3],
    pub far: f32,
    /// 箱の中心
    pub center: [f32; 2],
    /// 描き先のピクセル (0, 0) が指す箱の座標
    pub origin: [f32; 2],
    /// 1 ピクセルあたりの箱の座標
    pub step: [f32; 2],
    /// 描き先の大きさ (ピクセル)
    pub size: [f32; 2],
    /// 光の向き
    pub light: [f32; 3],
    /// 影の側の明るさ
    pub ambient: f32,
}

/// uniform は vec3 が 16 バイト境界に乗るので、後ろの f32 と組にして 16 バイトずつ並べる
pub const VIEW_BYTES: usize = 112;

impl View3 {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(VIEW_BYTES);
        let mut put = |xs: &[f32]| {
            for f in xs {
                out.extend_from_slice(&f.to_le_bytes());
            }
        };
        put(&[self.eye[0], self.eye[1], self.eye[2], self.focal]);
        put(&[self.right[0], self.right[1], self.right[2], self.unit]);
        put(&[self.up[0], self.up[1], self.up[2], self.near]);
        put(&[self.fwd[0], self.fwd[1], self.fwd[2], self.far]);
        put(&[self.center[0], self.center[1], self.origin[0], self.origin[1]]);
        put(&[self.step[0], self.step[1], self.size[0], self.size[1]]);
        put(&[self.light[0], self.light[1], self.light[2], self.ambient]);
        out
    }
}

const WGSL: &str = r#"
struct View {
  eye: vec3<f32>, focal: f32,
  right: vec3<f32>, unit: f32,
  up: vec3<f32>, near: f32,
  fwd: vec3<f32>, far: f32,
  center: vec2<f32>, origin: vec2<f32>,
  step: vec2<f32>, size: vec2<f32>,
  light: vec3<f32>, ambient: f32,
}
@group(0) @binding(0) var<uniform> v: View;

struct Out {
  @builtin(position) clip: vec4<f32>,
  @location(0) tint: vec4<f32>,
}

@vertex
fn vs(@location(0) at: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) color: vec4<f32>) -> Out {
  let d = at - v.eye;
  // 視点から見た座標 (右, 上, 奥)
  let s = vec3<f32>(dot(d, v.right), dot(d, v.up), dot(d, v.fwd));
  // 箱の座標を、描き先のピクセル、さらに -1..1 に直すための係数
  let ax = 2.0 / (v.step.x * v.size.x);
  let ay = 2.0 / (v.step.y * v.size.y);
  let bx = (v.center.x - v.origin.x) * ax - 1.0;
  let by = 1.0 - (v.center.y - v.origin.y) * ay;
  var out: Out;
  if (v.focal > 0.0) {
    // 透視。w を奥行きにして、割り算を GPU に任せる
    out.clip = vec4<f32>(
      s.z * bx + s.x * v.focal * ax,
      s.z * by - s.y * v.focal * ay,
      v.far * (s.z - v.near) / (v.far - v.near),
      s.z);
  } else {
    out.clip = vec4<f32>(bx + s.x * v.unit * ax, by - s.y * v.unit * ay, (s.z - v.near) / (v.far - v.near), 1.0);
  }
  // 面の向きで明るさを決める。space3d.shade と同じ式
  let lit = max(dot(normalize(normal), normalize(v.light)), 0.0);
  let k = v.ambient + (1.0 - v.ambient) * lit;
  out.tint = vec4<f32>(color.rgb * k, color.a);
  return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
  // Vello は掛け合わせ済みの色を待っている
  return vec4<f32>(in.tint.rgb * in.tint.a, in.tint.a);
}
"#;

pub const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// 深度付きで三角形を描く pipeline。1 つだけ作って使い回す
pub struct Meshes {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

impl Meshes {
    pub fn new(device: &wgpu::Device) -> Meshes {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("mophila mesh"), source: wgpu::ShaderSource::Wgsl(WGSL.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mophila mesh"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("mophila mesh"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mophila mesh"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: VERTEX_BYTES as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 1 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 24, shader_location: 2 },
                    ],
                }],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Meshes { pipeline, layout }
    }

    /// 1 回ぶん描く。color が描き先、depth は同じ大きさの深度テクスチャ
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        uniforms: &wgpu::Buffer,
        points: &wgpu::Buffer,
        view: &View3,
        count: u32,
        background: [f32; 4],
    ) {
        queue.write_buffer(uniforms, 0, &view.bytes());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mophila mesh"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mophila mesh"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: f64::from(background[0] * background[3]),
                        g: f64::from(background[1] * background[3]),
                        b: f64::from(background[2] * background[3]),
                        a: f64::from(background[3]),
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.set_vertex_buffer(0, points.slice(..));
        pass.draw(0..count, 0..1);
    }
}
