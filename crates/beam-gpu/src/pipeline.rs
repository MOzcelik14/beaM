//! GPU edit pipeline with modular, reorderable WGSL shader stages and ping-pong execution.

use beam_core::error::{BeamError, Result};
use beam_core::sidecar::EditStack;
use std::sync::Arc;

pub const DISPLAY_SHADER_SRC: &str = include_str!("../../../shaders/display_transform.wgsl");
pub const EXPOSURE_SHADER_SRC: &str = include_str!("../../../shaders/exposure.wgsl");
pub const WHITE_BALANCE_SHADER_SRC: &str = include_str!("../../../shaders/white_balance.wgsl");
pub const CONTRAST_SHADER_SRC: &str = include_str!("../../../shaders/contrast.wgsl");
pub const HIGHLIGHTS_SHADOWS_SHADER_SRC: &str =
    include_str!("../../../shaders/highlights_shadows.wgsl");
pub const TONE_CURVE_SHADER_SRC: &str = include_str!("../../../shaders/tone_curve.wgsl");
pub const HSL_SHADER_SRC: &str = include_str!("../../../shaders/hsl.wgsl");
pub const VIBRANCE_SATURATION_SHADER_SRC: &str =
    include_str!("../../../shaders/vibrance_saturation.wgsl");
pub const VIGNETTE_GRAIN_SHADER_SRC: &str = include_str!("../../../shaders/vignette_grain.wgsl");
pub const COLOR_GRADING_SHADER_SRC: &str = include_str!("../../../shaders/color_grading.wgsl");
pub const DETAIL_SHADER_SRC: &str = include_str!("../../../shaders/detail.wgsl");
pub const LUT3D_SHADER_SRC: &str = include_str!("../../../shaders/lut3d.wgsl");
pub const MASK_ADJUST_SHADER_SRC: &str = include_str!("../../../shaders/mask_adjust.wgsl");
pub const EXPORT_TRANSFORM_SHADER_SRC: &str =
    include_str!("../../../shaders/export_transform.wgsl");

/// GPU texture container for an image in linear working space.
pub struct GpuImage {
    pub width: u32,
    pub height: u32,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

impl GpuImage {
    /// Upload linear RGBA32F data to a new GPU texture.
    pub fn from_rgba32f(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        data: &[f32],
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(BeamError::GpuPipeline(
                "Image dimensions cannot be zero".into(),
            ));
        }

        let max_dim = device.limits().max_texture_dimension_2d;
        if width > max_dim || height > max_dim {
            return Err(BeamError::GpuPipeline(format!(
                "Image dimensions ({width}x{height}) exceed GPU maximum 2D texture limit ({max_dim})"
            )));
        }

        let expected_len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| BeamError::GpuPipeline("Pixel count calculation overflowed".into()))?;

        if data.len() != expected_len {
            return Err(BeamError::GpuPipeline(format!(
                "Data size mismatch: expected {expected_len} floats, got {}",
                data.len()
            )));
        }

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("beam_raw_linear_texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4 * std::mem::size_of::<f32>() as u32),
                rows_per_image: Some(height),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Ok(Self {
            width,
            height,
            texture,
            view,
        })
    }
}

// ----------------------------------------------------------------------------
// Stage Uniform Definitions
// ----------------------------------------------------------------------------

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ExposureUniforms {
    pub multiplier: f32,
    pub enabled: u32,
    pub _pad0: u32,
    pub _pad1: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WhiteBalanceUniforms {
    pub gain: [f32; 4],
    pub enabled: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ContrastUniforms {
    pub slope: f32,
    pub midpoint: f32,
    pub enabled: u32,
    pub _pad0: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HighlightsShadowsUniforms {
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub enabled: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VibranceSaturationUniforms {
    pub vibrance: f32,
    pub saturation: f32,
    pub enabled: u32,
    pub _pad0: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VignetteGrainUniforms {
    pub vignette_amount: f32,
    pub vignette_midpoint: f32,
    pub vignette_feather: f32,
    pub grain_amount: f32,
    pub grain_size: f32,
    pub enabled: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub uv_offset: [f32; 2],
    pub uv_scale: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ToneCurveUniforms {
    pub master_lut: [[f32; 4]; 64],
    pub red_lut: [[f32; 4]; 64],
    pub green_lut: [[f32; 4]; 64],
    pub blue_lut: [[f32; 4]; 64],
    pub enabled: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ColorGradingUniforms {
    pub shadows: [f32; 4],
    pub midtones: [f32; 4],
    pub highlights: [f32; 4],
    pub params: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DetailUniforms {
    pub clarity: f32,
    pub dehaze: f32,
    pub sharpening: f32,
    pub noise_reduction_luma: f32,
    pub noise_reduction_color: f32,
    pub enabled: u32,
    pub _pad0: u32,
    pub _pad1: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Lut3DUniforms {
    pub domain_min: [f32; 4],
    pub domain_max: [f32; 4],
    pub params: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MaskGpuEntry {
    pub geom0: [f32; 4],
    pub geom1: [f32; 4],
    pub adj0: [f32; 4],
    pub adj1: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MaskUniforms {
    pub masks: [MaskGpuEntry; 8],
    pub count: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
    pub uv_rect: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HslUniforms {
    pub bands: [[f32; 4]; 8],
    pub enabled: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

// ----------------------------------------------------------------------------
// Individual Shader Stage Passes
// ----------------------------------------------------------------------------

struct StagePass<U: bytemuck::Pod + bytemuck::Zeroable> {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    _marker: std::marker::PhantomData<U>,
}

impl<U: bytemuck::Pod + bytemuck::Zeroable> StagePass<U> {
    fn new(
        device: &wgpu::Device,
        label: &'static str,
        shader_src: &'static str,
        initial_uniform: U,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("{label}_bgl")),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&format!("{label}_layout")),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&format!("{label}_pipeline")),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let uniform_buffer = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{label}_uniform_buffer")),
                contents: bytemuck::bytes_of(&initial_uniform),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            },
        );

        Self {
            pipeline,
            bind_group_layout,
            uniform_buffer,
            _marker: std::marker::PhantomData,
        }
    }

    fn update_uniform(&self, queue: &wgpu::Queue, uniform: &U) {
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(uniform));
    }

    fn render(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input_view: &wgpu::TextureView,
        target_view: &wgpu::TextureView,
    ) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stage_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stage_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(0, &bind_group, &[]);
        rpass.draw(0..3, 0..1);
    }
}

// ----------------------------------------------------------------------------
// Display Transform Pipeline (Linear Rec.2020 -> sRGB)
// ----------------------------------------------------------------------------

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DisplayGeometryUniforms {
    pub crop_x: f32,
    pub crop_y: f32,
    pub crop_w: f32,
    pub crop_h: f32,
    pub rotation: u32,
    pub flip_h: u32,
    pub flip_v: u32,
    pub straighten_rad: f32,
    pub crop_enabled: u32,
    pub show_shadow_clipping: u32,
    pub show_highlight_clipping: u32,
    pub _pad2: u32,
}

impl Default for DisplayGeometryUniforms {
    fn default() -> Self {
        Self {
            crop_x: 0.0,
            crop_y: 0.0,
            crop_w: 1.0,
            crop_h: 1.0,
            rotation: 0,
            flip_h: 0,
            flip_v: 0,
            straighten_rad: 0.0,
            crop_enabled: 0,
            show_shadow_clipping: 0,
            show_highlight_clipping: 0,
            _pad2: 0,
        }
    }
}

pub struct DisplayPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    target_format: wgpu::TextureFormat,
}

impl DisplayPipeline {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("display_transform_shader"),
            source: wgpu::ShaderSource::Wgsl(DISPLAY_SHADER_SRC.into()),
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("display_geometry_uniform_buffer"),
            size: std::mem::size_of::<DisplayGeometryUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("display_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("display_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("display_render_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
            uniform_buffer,
            target_format,
        }
    }

    pub fn render_to_target(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input_view: &wgpu::TextureView,
        target_view: &wgpu::TextureView,
        uniforms: &DisplayGeometryUniforms,
    ) {
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(uniforms));

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("display_render_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("display_command_encoder"),
        });

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("display_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            rpass.set_pipeline(&self.pipeline);
            rpass.set_bind_group(0, &bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }

        queue.submit(std::iter::once(encoder.finish()));
    }

    pub fn target_format(&self) -> wgpu::TextureFormat {
        self.target_format
    }
}

// ----------------------------------------------------------------------------
// Export Transform Pipeline (Linear Rec.2020 -> sRGB / Display P3 / Adobe RGB)
// ----------------------------------------------------------------------------

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ExportUniforms {
    pub color_space: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

pub struct ExportDisplayPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    target_format: wgpu::TextureFormat,
}

impl ExportDisplayPipeline {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("export_transform_shader"),
            source: wgpu::ShaderSource::Wgsl(EXPORT_TRANSFORM_SHADER_SRC.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("export_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("export_uniform_buffer"),
            size: std::mem::size_of::<ExportUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("export_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("export_render_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
            uniform_buffer,
            target_format,
        }
    }

    pub fn render_to_target(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input_view: &wgpu::TextureView,
        target_view: &wgpu::TextureView,
        color_space: u32,
    ) {
        let uniforms = ExportUniforms {
            color_space,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("export_render_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("export_command_encoder"),
        });

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("export_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            rpass.set_pipeline(&self.pipeline);
            rpass.set_bind_group(0, &bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }

        queue.submit(std::iter::once(encoder.finish()));
    }

    pub fn target_format(&self) -> wgpu::TextureFormat {
        self.target_format
    }
}

// ----------------------------------------------------------------------------
// 3D LUT Pass (texture_2d + uniform + texture_3d + sampler)
// ----------------------------------------------------------------------------

pub struct Lut3DPass {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    sampler: wgpu::Sampler,
    lut_texture: Option<wgpu::Texture>,
    lut_view: Option<wgpu::TextureView>,
    cached_path: Option<String>,
    cached_size: u32,
    cached_domain_min: [f32; 3],
    cached_domain_max: [f32; 3],
    cached_valid: bool,
}

fn f32_to_f16_bits(val: f32) -> u16 {
    let bits = val.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let frac = bits & 0x007f_ffff;

    if exp == 255 {
        return sign | 0x7c00 | if frac != 0 { 0x0200 } else { 0 };
    }
    let new_exp = exp - 127 + 15;
    if new_exp >= 31 {
        return sign | 0x7c00;
    }
    if new_exp <= 0 {
        if new_exp < -10 {
            return sign;
        }
        let m = (frac | 0x0080_0000) >> (14 - new_exp);
        return sign | (m as u16);
    }
    sign | ((new_exp as u16) << 10) | ((frac >> 13) as u16)
}

impl Lut3DPass {
    fn create_3d_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: u32,
        rgba16f: &[u16],
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lut3d_texture"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: size,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(rgba16f),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(8 * size),
                rows_per_image: Some(size),
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: size,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D3),
            ..Default::default()
        });
        (texture, view)
    }

    pub fn new(device: &wgpu::Device, output_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lut3d_shader"),
            source: wgpu::ShaderSource::Wgsl(LUT3D_SHADER_SRC.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lut3d_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lut3d_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lut3d_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        use wgpu::util::DeviceExt;
        let initial_uniforms = Lut3DUniforms {
            domain_min: [0.0, 0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0, 0.0],
            params: [1.0, 0.0, 2.0, 0.0],
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("lut3d_uniform_buffer"),
            contents: bytemuck::bytes_of(&initial_uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lut3d_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Self {
            pipeline,
            bind_group_layout,
            uniform_buffer,
            sampler,
            lut_texture: None,
            lut_view: None,
            cached_path: None,
            cached_size: 2,
            cached_domain_min: [0.0, 0.0, 0.0],
            cached_domain_max: [1.0, 1.0, 1.0],
            cached_valid: false,
        }
    }

    pub fn update_from_stack(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        lut_adj: &beam_core::sidecar::LutAdjustment,
    ) {
        if self.lut_view.is_none() {
            let one = f32_to_f16_bits(1.0);
            let identity_data: Vec<u16> = [
                [0u16, 0, 0, one],
                [one, 0, 0, one],
                [0, one, 0, one],
                [one, one, 0, one],
                [0, 0, one, one],
                [one, 0, one, one],
                [0, one, one, one],
                [one, one, one, one],
            ]
            .concat();
            let (tex, view) = Self::create_3d_texture(device, queue, 2, &identity_data);
            self.lut_texture = Some(tex);
            self.lut_view = Some(view);
        }

        if lut_adj.enabled {
            if let Some(ref path_str) = lut_adj.path {
                if self.cached_path.as_deref() != Some(path_str.as_str()) {
                    self.cached_path = Some(path_str.clone());
                    match beam_core::lut::parse_cube_file(std::path::Path::new(path_str)) {
                        Ok(cube) => {
                            let one = f32_to_f16_bits(1.0);
                            let rgba16f: Vec<u16> = cube
                                .data
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .flat_map(|rgba| {
                                    [
                                        f32_to_f16_bits(rgba[0]),
                                        f32_to_f16_bits(rgba[1]),
                                        f32_to_f16_bits(rgba[2]),
                                        one,
                                    ]
                                })
                                .collect();
                            let (tex, view) =
                                Self::create_3d_texture(device, queue, cube.size as u32, &rgba16f);
                            self.lut_texture = Some(tex);
                            self.lut_view = Some(view);
                            self.cached_size = cube.size as u32;
                            self.cached_domain_min = cube.domain_min;
                            self.cached_domain_max = cube.domain_max;
                            self.cached_valid = true;
                        }
                        Err(e) => {
                            eprintln!("Failed to load 3D LUT '{}': {}", path_str, e);
                            self.cached_valid = false;
                        }
                    }
                }
            } else {
                self.cached_path = None;
                self.cached_valid = false;
            }
        }

        let enabled_f = if lut_adj.enabled && self.cached_valid && lut_adj.path.is_some() {
            1.0
        } else {
            0.0
        };
        let uniforms = Lut3DUniforms {
            domain_min: [
                self.cached_domain_min[0],
                self.cached_domain_min[1],
                self.cached_domain_min[2],
                0.0,
            ],
            domain_max: [
                self.cached_domain_max[0],
                self.cached_domain_max[1],
                self.cached_domain_max[2],
                0.0,
            ],
            params: [lut_adj.intensity, enabled_f, self.cached_size as f32, 0.0],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    pub fn render(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input_view: &wgpu::TextureView,
        output_view: &wgpu::TextureView,
    ) {
        let Some(lut_view) = self.lut_view.as_ref() else {
            return;
        };
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lut3d_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(lut_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("lut3d_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(0, &bind_group, &[]);
        rpass.draw(0..3, 0..1);
    }
}

// ----------------------------------------------------------------------------
// Full Edit Pipeline (Ping-Pong Intermediate Passes)
// ----------------------------------------------------------------------------

pub struct EditPipeline {
    white_balance_stage: StagePass<WhiteBalanceUniforms>,
    exposure_stage: StagePass<ExposureUniforms>,
    highlights_shadows_stage: StagePass<HighlightsShadowsUniforms>,
    contrast_stage: StagePass<ContrastUniforms>,
    tone_curve_stage: StagePass<ToneCurveUniforms>,
    hsl_stage: StagePass<HslUniforms>,
    vibrance_saturation_stage: StagePass<VibranceSaturationUniforms>,
    color_grading_stage: StagePass<ColorGradingUniforms>,
    detail_stage: StagePass<DetailUniforms>,
    lut3d_stage: Lut3DPass,
    vignette_grain_stage: StagePass<VignetteGrainUniforms>,
    mask_stage: StagePass<MaskUniforms>,
    display_pipeline: DisplayPipeline,
    export_display_pipeline: Option<ExportDisplayPipeline>,

    // Ping-pong intermediate linear working textures
    ping_texture: Option<wgpu::Texture>,
    ping_view: Option<wgpu::TextureView>,
    pong_texture: Option<wgpu::Texture>,
    pong_view: Option<wgpu::TextureView>,
    current_dimensions: (u32, u32),
}

impl EditPipeline {
    pub fn new(device: &wgpu::Device, display_format: wgpu::TextureFormat) -> Self {
        let working_format = wgpu::TextureFormat::Rgba32Float;

        let white_balance_stage = StagePass::new(
            device,
            "wb_stage",
            WHITE_BALANCE_SHADER_SRC,
            WhiteBalanceUniforms {
                gain: [1.0, 1.0, 1.0, 1.0],
                enabled: 1,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            },
            working_format,
        );

        let exposure_stage = StagePass::new(
            device,
            "exposure_stage",
            EXPOSURE_SHADER_SRC,
            ExposureUniforms {
                multiplier: 1.0,
                enabled: 1,
                _pad0: 0,
                _pad1: 0,
            },
            working_format,
        );

        let highlights_shadows_stage = StagePass::new(
            device,
            "hs_stage",
            HIGHLIGHTS_SHADOWS_SHADER_SRC,
            HighlightsShadowsUniforms {
                highlights: 0.0,
                shadows: 0.0,
                whites: 0.0,
                blacks: 0.0,
                enabled: 1,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            },
            working_format,
        );

        let contrast_stage = StagePass::new(
            device,
            "contrast_stage",
            CONTRAST_SHADER_SRC,
            ContrastUniforms {
                slope: 1.0,
                midpoint: 0.18,
                enabled: 1,
                _pad0: 0,
            },
            working_format,
        );

        let identity_lut = beam_core::color::generate_tone_curve_lut(&[
            beam_core::sidecar::ToneCurvePoint { x: 0.0, y: 0.0 },
            beam_core::sidecar::ToneCurvePoint { x: 1.0, y: 1.0 },
        ]);
        let mut initial_lut = [[0.0f32; 4]; 64];
        for i in 0..64 {
            initial_lut[i] = [
                identity_lut[i * 4],
                identity_lut[i * 4 + 1],
                identity_lut[i * 4 + 2],
                identity_lut[i * 4 + 3],
            ];
        }

        let tone_curve_stage = StagePass::new(
            device,
            "tc_stage",
            TONE_CURVE_SHADER_SRC,
            ToneCurveUniforms {
                master_lut: initial_lut,
                red_lut: initial_lut,
                green_lut: initial_lut,
                blue_lut: initial_lut,
                enabled: 1,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            },
            working_format,
        );

        let hsl_stage = StagePass::new(
            device,
            "hsl_stage",
            HSL_SHADER_SRC,
            HslUniforms {
                bands: [[0.0; 4]; 8],
                enabled: 1,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            },
            working_format,
        );

        let vibrance_saturation_stage = StagePass::new(
            device,
            "vibrance_saturation_stage",
            VIBRANCE_SATURATION_SHADER_SRC,
            VibranceSaturationUniforms {
                vibrance: 0.0,
                saturation: 0.0,
                enabled: 1,
                _pad0: 0,
            },
            working_format,
        );

        let color_grading_stage = StagePass::new(
            device,
            "color_grading_stage",
            COLOR_GRADING_SHADER_SRC,
            ColorGradingUniforms {
                shadows: [0.0; 4],
                midtones: [0.0; 4],
                highlights: [0.0; 4],
                params: [0.5, 0.0, 0.0, 0.0],
            },
            working_format,
        );

        let detail_stage = StagePass::new(
            device,
            "detail_stage",
            DETAIL_SHADER_SRC,
            DetailUniforms {
                clarity: 0.0,
                dehaze: 0.0,
                sharpening: 0.0,
                noise_reduction_luma: 0.0,
                noise_reduction_color: 0.0,
                enabled: 0,
                _pad0: 0,
                _pad1: 0,
            },
            working_format,
        );

        let lut3d_stage = Lut3DPass::new(device, working_format);

        let vignette_grain_stage = StagePass::new(
            device,
            "vignette_grain_stage",
            VIGNETTE_GRAIN_SHADER_SRC,
            VignetteGrainUniforms {
                vignette_amount: 0.0,
                vignette_midpoint: 50.0,
                vignette_feather: 50.0,
                grain_amount: 0.0,
                grain_size: 1.5,
                enabled: 1,
                _pad0: 0,
                _pad1: 0,
                uv_offset: [0.0, 0.0],
                uv_scale: [1.0, 1.0],
            },
            working_format,
        );

        let mask_stage = StagePass::new(
            device,
            "mask_stage",
            MASK_ADJUST_SHADER_SRC,
            MaskUniforms {
                masks: [MaskGpuEntry {
                    geom0: [0.0; 4],
                    geom1: [0.0; 4],
                    adj0: [1.0, 1.0, 0.0, 0.0],
                    adj1: [1.0, 1.0, 1.0, 0.0],
                }; 8],
                count: 0,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
                uv_rect: [0.0, 0.0, 1.0, 1.0],
            },
            working_format,
        );

        let display_pipeline = DisplayPipeline::new(device, display_format);

        Self {
            white_balance_stage,
            exposure_stage,
            highlights_shadows_stage,
            contrast_stage,
            tone_curve_stage,
            hsl_stage,
            vibrance_saturation_stage,
            color_grading_stage,
            detail_stage,
            lut3d_stage,
            vignette_grain_stage,
            mask_stage,
            display_pipeline,
            export_display_pipeline: None,
            ping_texture: None,
            ping_view: None,
            pong_texture: None,
            pong_view: None,
            current_dimensions: (0, 0),
        }
    }

    /// Ensure intermediate ping-pong textures match the required resolution.
    fn ensure_intermediate_textures(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.current_dimensions == (width, height) && self.ping_texture.is_some() {
            return;
        }

        let desc = wgpu::TextureDescriptor {
            label: Some("beam_ping_pong_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };

        let ping = device.create_texture(&desc);
        let ping_view = ping.create_view(&wgpu::TextureViewDescriptor::default());

        let pong = device.create_texture(&desc);
        let pong_view = pong.create_view(&wgpu::TextureViewDescriptor::default());

        self.ping_texture = Some(ping);
        self.ping_view = Some(ping_view);
        self.pong_texture = Some(pong);
        self.pong_view = Some(pong_view);
        self.current_dimensions = (width, height);
    }

    /// Update uniforms for all stages from the edit stack.
    fn update_uniforms(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        edit_stack: &EditStack,
    ) {
        self.update_uniforms_with_uv_rect(device, queue, edit_stack, [0.0, 0.0, 1.0, 1.0]);
    }

    fn update_uniforms_with_uv_rect(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        edit_stack: &EditStack,
        uv_rect: [f32; 4],
    ) {
        let wb_gains = beam_core::color::white_balance_gains(
            edit_stack.white_balance.temperature,
            edit_stack.white_balance.tint,
        );
        self.white_balance_stage.update_uniform(
            queue,
            &WhiteBalanceUniforms {
                gain: [wb_gains[0], wb_gains[1], wb_gains[2], 1.0],
                enabled: if edit_stack.white_balance.enabled {
                    1
                } else {
                    0
                },
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            },
        );

        let ev_mult = 2.0f32.powf(edit_stack.exposure.ev);
        self.exposure_stage.update_uniform(
            queue,
            &ExposureUniforms {
                multiplier: ev_mult,
                enabled: if edit_stack.exposure.enabled { 1 } else { 0 },
                _pad0: 0,
                _pad1: 0,
            },
        );

        self.highlights_shadows_stage.update_uniform(
            queue,
            &HighlightsShadowsUniforms {
                highlights: edit_stack.highlights_shadows.highlights,
                shadows: edit_stack.highlights_shadows.shadows,
                whites: edit_stack.highlights_shadows.whites,
                blacks: edit_stack.highlights_shadows.blacks,
                enabled: if edit_stack.highlights_shadows.enabled {
                    1
                } else {
                    0
                },
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            },
        );

        let contrast_slope = beam_core::color::contrast_slope(edit_stack.contrast.contrast);
        self.contrast_stage.update_uniform(
            queue,
            &ContrastUniforms {
                slope: contrast_slope,
                midpoint: 0.18,
                enabled: if edit_stack.contrast.enabled { 1 } else { 0 },
                _pad0: 0,
            },
        );

        let pack_curve_lut = |points: &[beam_core::sidecar::ToneCurvePoint]| -> [[f32; 4]; 64] {
            let curve_lut = beam_core::color::generate_tone_curve_lut(points);
            let mut packed = [[0.0f32; 4]; 64];
            for i in 0..64 {
                packed[i] = [
                    curve_lut[i * 4],
                    curve_lut[i * 4 + 1],
                    curve_lut[i * 4 + 2],
                    curve_lut[i * 4 + 3],
                ];
            }
            packed
        };
        self.tone_curve_stage.update_uniform(
            queue,
            &ToneCurveUniforms {
                master_lut: pack_curve_lut(&edit_stack.tone_curve.points),
                red_lut: pack_curve_lut(&edit_stack.tone_curve.red_points),
                green_lut: pack_curve_lut(&edit_stack.tone_curve.green_points),
                blue_lut: pack_curve_lut(&edit_stack.tone_curve.blue_points),
                enabled: if edit_stack.tone_curve.enabled { 1 } else { 0 },
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            },
        );

        let mut hsl_bands = [[0.0f32; 4]; 8];
        for (i, band) in edit_stack.hsl.bands.iter().enumerate() {
            hsl_bands[i] = [band.hue, band.saturation, band.luminance, 0.0];
        }
        self.hsl_stage.update_uniform(
            queue,
            &HslUniforms {
                bands: hsl_bands,
                enabled: if edit_stack.hsl.enabled { 1 } else { 0 },
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            },
        );

        self.vibrance_saturation_stage.update_uniform(
            queue,
            &VibranceSaturationUniforms {
                vibrance: edit_stack.vibrance_saturation.vibrance,
                saturation: edit_stack.vibrance_saturation.saturation,
                enabled: if edit_stack.vibrance_saturation.enabled {
                    1
                } else {
                    0
                },
                _pad0: 0,
            },
        );

        let wheel_to_vec4 = |w: &beam_core::sidecar::ColorWheelGrade| -> [f32; 4] {
            let h = (w.hue.rem_euclid(360.0)) / 60.0;
            let sector = h.floor() as i32;
            let f = h - sector as f32;
            let (r, g, b) = match sector {
                0 => (1.0, f, 0.0),
                1 => (1.0 - f, 1.0, 0.0),
                2 => (0.0, 1.0, f),
                3 => (0.0, 1.0 - f, 1.0),
                4 => (f, 0.0, 1.0),
                _ => (1.0, 0.0, 1.0 - f),
            };
            let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let sat = w.saturation.clamp(0.0, 1.0);
            [
                (r - lum) * sat,
                (g - lum) * sat,
                (b - lum) * sat,
                w.luminance,
            ]
        };
        let cg = &edit_stack.color_grading;
        self.color_grading_stage.update_uniform(
            queue,
            &ColorGradingUniforms {
                shadows: wheel_to_vec4(&cg.shadows),
                midtones: wheel_to_vec4(&cg.midtones),
                highlights: wheel_to_vec4(&cg.highlights),
                params: [
                    cg.blending,
                    cg.balance,
                    if cg.enabled { 1.0 } else { 0.0 },
                    0.0,
                ],
            },
        );

        let det = &edit_stack.detail;
        self.detail_stage.update_uniform(
            queue,
            &DetailUniforms {
                clarity: det.clarity,
                dehaze: det.dehaze,
                sharpening: det.sharpening,
                noise_reduction_luma: det.noise_reduction_luma,
                noise_reduction_color: det.noise_reduction_color,
                enabled: if det.enabled { 1 } else { 0 },
                _pad0: 0,
                _pad1: 0,
            },
        );

        self.lut3d_stage
            .update_from_stack(device, queue, &edit_stack.lut);

        self.vignette_grain_stage.update_uniform(
            queue,
            &VignetteGrainUniforms {
                vignette_amount: edit_stack.vignette_grain.vignette_amount,
                vignette_midpoint: edit_stack.vignette_grain.vignette_midpoint,
                vignette_feather: edit_stack.vignette_grain.vignette_feather,
                grain_amount: edit_stack.vignette_grain.grain_amount,
                grain_size: edit_stack.vignette_grain.grain_size,
                enabled: if edit_stack.vignette_grain.enabled {
                    1
                } else {
                    0
                },
                _pad0: 0,
                _pad1: 0,
                uv_offset: [uv_rect[0], uv_rect[1]],
                uv_scale: [uv_rect[2], uv_rect[3]],
            },
        );
        let mut gpu_masks = [MaskGpuEntry {
            geom0: [0.0; 4],
            geom1: [0.0; 4],
            adj0: [1.0, 1.0, 0.0, 0.0],
            adj1: [1.0, 1.0, 1.0, 0.0],
        }; 8];
        let mask_count = edit_stack.masks.len().min(8);
        for (i, m) in edit_stack.masks.iter().take(8).enumerate() {
            let (geom0, geom1) = match m.shape {
                beam_core::sidecar::MaskShape::LinearGradient { start, end } => (
                    [0.0, 0.0, 0.0, if m.enabled { 1.0 } else { 0.0 }],
                    [start[0], start[1], end[0], end[1]],
                ),
                beam_core::sidecar::MaskShape::RadialGradient {
                    center,
                    radius,
                    feather,
                    invert,
                } => (
                    [
                        1.0,
                        if invert { 1.0 } else { 0.0 },
                        feather,
                        if m.enabled { 1.0 } else { 0.0 },
                    ],
                    [center[0], center[1], radius[0], radius[1]],
                ),
            };
            let wb_gains = beam_core::color::white_balance_gains(m.temperature, m.tint);
            let exp_mult = 2.0f32.powf(m.exposure);
            let contrast_slope = beam_core::color::contrast_slope(m.contrast);
            gpu_masks[i] = MaskGpuEntry {
                geom0,
                geom1,
                adj0: [
                    exp_mult,
                    contrast_slope,
                    m.highlights / 100.0,
                    m.shadows / 100.0,
                ],
                adj1: [wb_gains[0], wb_gains[1], wb_gains[2], m.saturation / 100.0],
            };
        }
        self.mask_stage.update_uniform(
            queue,
            &MaskUniforms {
                masks: gpu_masks,
                count: mask_count as u32,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
                uv_rect,
            },
        );
    }

    /// Execute intermediate shader stages. Returns `true` if final output is in `pong`, `false` if in `ping`.
    fn render_intermediate_stages(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &GpuImage,
    ) -> bool {
        let ping = self.ping_view.as_ref().unwrap();
        let pong = self.pong_view.as_ref().unwrap();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("edit_pipeline_encoder"),
        });

        // Stage 1: White balance (input -> ping)
        self.white_balance_stage
            .render(device, &mut encoder, &input.view, ping);

        // Stage 2: Exposure (ping -> pong)
        self.exposure_stage.render(device, &mut encoder, ping, pong);

        // Stage 3: Highlights, Shadows, Whites & Blacks (pong -> ping)
        self.highlights_shadows_stage
            .render(device, &mut encoder, pong, ping);

        // Stage 4: Contrast (ping -> pong)
        self.contrast_stage.render(device, &mut encoder, ping, pong);

        // Stage 5: Tone Curve (pong -> ping)
        self.tone_curve_stage
            .render(device, &mut encoder, pong, ping);

        // Stage 6: HSL (ping -> pong)
        self.hsl_stage.render(device, &mut encoder, ping, pong);

        // Stage 7: Vibrance & Saturation (pong -> ping)
        self.vibrance_saturation_stage
            .render(device, &mut encoder, pong, ping);

        // Stage 8: Color Grading (ping -> pong)
        self.color_grading_stage
            .render(device, &mut encoder, ping, pong);

        // Stage 9: Detail (pong -> ping)
        self.detail_stage.render(device, &mut encoder, pong, ping);

        // Stage 10: 3D LUT (ping -> pong)
        self.lut3d_stage.render(device, &mut encoder, ping, pong);

        // Stage 11: Vignette & Grain (pong -> ping)
        self.vignette_grain_stage
            .render(device, &mut encoder, pong, ping);

        // Stage 12: Parametric Masks (ping -> pong)
        self.mask_stage.render(device, &mut encoder, ping, pong);

        queue.submit(std::iter::once(encoder.finish()));
        true
    }

    /// Execute the full edit stack from input texture to output display target.
    /// Execute the full edit stack from input texture to output display target.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &GpuImage,
        target_view: &wgpu::TextureView,
        edit_stack: &EditStack,
    ) {
        self.render_with_crop_mode(device, queue, input, target_view, edit_stack, false);
    }

    /// Execute the full edit stack, optionally keeping uncropped view for interactive crop editing.
    pub fn render_with_crop_mode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &GpuImage,
        target_view: &wgpu::TextureView,
        edit_stack: &EditStack,
        crop_mode_active: bool,
    ) {
        self.render_with_options(
            device,
            queue,
            input,
            target_view,
            edit_stack,
            crop_mode_active,
            false,
            false,
        );
    }

    /// Execute the full edit stack, optionally keeping uncropped view and showing clipping warnings.
    #[allow(clippy::too_many_arguments)]
    pub fn render_with_options(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &GpuImage,
        target_view: &wgpu::TextureView,
        edit_stack: &EditStack,
        crop_mode_active: bool,
        show_shadow_clipping: bool,
        show_highlight_clipping: bool,
    ) {
        self.ensure_intermediate_textures(device, input.width, input.height);
        self.update_uniforms(device, queue, edit_stack);
        let in_pong = self.render_intermediate_stages(device, queue, input);

        let intermediate_view = if in_pong {
            self.pong_view.as_ref().unwrap()
        } else {
            self.ping_view.as_ref().unwrap()
        };

        let crop = &edit_stack.crop;
        let uniforms = DisplayGeometryUniforms {
            crop_x: crop.x,
            crop_y: crop.y,
            crop_w: crop.width,
            crop_h: crop.height,
            rotation: crop.rotation,
            flip_h: if crop.flip_h { 1 } else { 0 },
            flip_v: if crop.flip_v { 1 } else { 0 },
            straighten_rad: crop.straighten.to_radians(),
            crop_enabled: if crop.enabled && !crop_mode_active {
                1
            } else {
                0
            },
            show_shadow_clipping: if show_shadow_clipping { 1 } else { 0 },
            show_highlight_clipping: if show_highlight_clipping { 1 } else { 0 },
            _pad2: 0,
        };

        // Final Stage: Display Transform (intermediate -> target_view)
        self.display_pipeline.render_to_target(
            device,
            queue,
            intermediate_view,
            target_view,
            &uniforms,
        );
    }

    /// Execute the complete edit stack onto an image and read back 8-bit RGBA pixels in requested color space.
    pub fn render_to_rgba8_with_colorspace(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &GpuImage,
        edit_stack: &EditStack,
        color_space: beam_core::ExportColorSpace,
    ) -> Result<Vec<u8>> {
        self.render_to_rgba8_with_colorspace_and_uv_rect(
            device,
            queue,
            input,
            edit_stack,
            color_space,
            [0.0, 0.0, 1.0, 1.0],
        )
    }

    fn render_to_rgba8_with_colorspace_and_uv_rect(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &GpuImage,
        edit_stack: &EditStack,
        color_space: beam_core::ExportColorSpace,
        uv_rect: [f32; 4],
    ) -> Result<Vec<u8>> {
        let width = input.width;
        let height = input.height;

        if width == 0 || height == 0 {
            return Err(BeamError::GpuPipeline(
                "Cannot render image with zero width or height".into(),
            ));
        }

        let max_dim = device.limits().max_texture_dimension_2d;
        if width > max_dim || height > max_dim {
            return Err(BeamError::GpuPipeline(format!(
                "Render dimensions ({width}x{height}) exceed GPU maximum 2D texture limit ({max_dim})"
            )));
        }

        // 1. Create target texture with Rgba8Unorm format
        let target_desc = wgpu::TextureDescriptor {
            label: Some("beam_export_target_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        let target_texture = device.create_texture(&target_desc);
        let target_view = target_texture.create_view(&wgpu::TextureViewDescriptor::default());

        if self.export_display_pipeline.is_none() {
            self.export_display_pipeline = Some(ExportDisplayPipeline::new(
                device,
                wgpu::TextureFormat::Rgba8Unorm,
            ));
        }

        self.ensure_intermediate_textures(device, width, height);
        self.update_uniforms_with_uv_rect(device, queue, edit_stack, uv_rect);
        let in_pong = self.render_intermediate_stages(device, queue, input);

        let intermediate_view = if in_pong {
            self.pong_view.as_ref()
        } else {
            self.ping_view.as_ref()
        }
        .ok_or_else(|| BeamError::GpuPipeline("Intermediate texture view missing".into()))?;
        let cs_val = match color_space {
            beam_core::ExportColorSpace::Srgb => 0u32,
            beam_core::ExportColorSpace::DisplayP3 => 1u32,
            beam_core::ExportColorSpace::AdobeRgb => 2u32,
        };
        self.export_display_pipeline
            .as_ref()
            .ok_or_else(|| BeamError::GpuPipeline("Export display pipeline missing".into()))?
            .render_to_target(device, queue, intermediate_view, &target_view, cs_val);

        // 3. Staging buffer with 256-byte row alignment
        let bytes_per_pixel = 4u32;
        let unaligned_bytes_per_row = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let bytes_per_row = (unaligned_bytes_per_row + align - 1) & !(align - 1);

        let staging_buffer_size = (bytes_per_row as u64)
            .checked_mul(height as u64)
            .ok_or_else(|| {
                BeamError::GpuPipeline("Staging buffer size calculation overflowed".into())
            })?;

        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("beam_export_staging_buffer"),
            size: staging_buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut copy_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("beam_export_readback_encoder"),
        });

        copy_encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        queue.submit(std::iter::once(copy_encoder.finish()));

        // 4. Map buffer and read back values
        let slice = staging_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });

        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| BeamError::GpuPipeline(format!("Device poll failed: {e:?}")))?;

        rx.recv()
            .map_err(|e| BeamError::GpuPipeline(format!("Readback channel receive failed: {e}")))?
            .map_err(|e| BeamError::GpuPipeline(format!("Buffer map failed: {e:?}")))?;

        let mut output_bytes = vec![0u8; (width * height * 4) as usize];
        {
            let mapped = slice.get_mapped_range().map_err(|e| {
                BeamError::GpuPipeline(format!("Failed to get mapped range: {e:?}"))
            })?;
            let row_len = (width * 4) as usize;
            for row in 0..height as usize {
                let src_start = row * (bytes_per_row as usize);
                let src_end = src_start + row_len;
                let dst_start = row * row_len;
                let dst_end = dst_start + row_len;
                output_bytes[dst_start..dst_end].copy_from_slice(&mapped[src_start..src_end]);
            }
        }
        staging_buffer.unmap();

        Ok(output_bytes)
    }

    /// Execute the complete edit stack onto an image and read back sRGB 8-bit RGBA pixels.
    pub fn render_to_rgba8(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &GpuImage,
        edit_stack: &EditStack,
    ) -> Result<Vec<u8>> {
        self.render_to_rgba8_with_colorspace(
            device,
            queue,
            input,
            edit_stack,
            beam_core::ExportColorSpace::Srgb,
        )
    }

    /// Execute the complete edit stack onto an image buffer using tiling if dimensions exceed `max_tile_size`.
    /// Guarantees that GPU VRAM usage is strictly bounded and large images (e.g. 40+ MP or 100 MP)
    /// never exceed hardware texture limits.
    #[allow(clippy::too_many_arguments)]
    pub fn render_tiled_to_rgba8(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        data: &[f32],
        edit_stack: &EditStack,
        color_space: beam_core::ExportColorSpace,
        max_tile_size: u32,
    ) -> Result<Vec<u8>> {
        const TILE_HALO: u32 = 8;
        let max_tile_size = max_tile_size.max(256);

        // If image fits within a single tile along both axes, run standard single pass
        if width <= max_tile_size && height <= max_tile_size {
            let gpu_image = GpuImage::from_rgba32f(device, queue, width, height, data)?;
            return self.render_to_rgba8_with_colorspace(
                device,
                queue,
                &gpu_image,
                edit_stack,
                color_space,
            );
        }

        // Tiled grid processing with halo overlap for spatial filters (detail/sharpening/NR)
        // and global UV rect for vignette, grain, and local masks.
        let mut final_rgba8 = vec![0u8; (width * height * 4) as usize];
        let tiles_x = width.div_ceil(max_tile_size);
        let tiles_y = height.div_ceil(max_tile_size);

        for ty in 0..tiles_y {
            let tile_y = ty * max_tile_size;
            let tile_h = max_tile_size.min(height - tile_y);
            let ext_y = tile_y.saturating_sub(TILE_HALO);
            let ext_bottom = (tile_y + tile_h + TILE_HALO).min(height);
            let ext_h = ext_bottom - ext_y;
            let inner_y = tile_y - ext_y;

            for tx in 0..tiles_x {
                let tile_x = tx * max_tile_size;
                let tile_w = max_tile_size.min(width - tile_x);
                let ext_x = tile_x.saturating_sub(TILE_HALO);
                let ext_right = (tile_x + tile_w + TILE_HALO).min(width);
                let ext_w = ext_right - ext_x;
                let inner_x = tile_x - ext_x;

                // Extract halo-expanded sub-rectangle of f32 pixel data
                let mut tile_data = Vec::with_capacity((ext_w * ext_h * 4) as usize);
                for row in 0..ext_h {
                    let src_y = ext_y + row;
                    let src_start = ((src_y * width + ext_x) * 4) as usize;
                    let src_end = src_start + (ext_w * 4) as usize;
                    tile_data.extend_from_slice(&data[src_start..src_end]);
                }

                let tile_gpu_image =
                    GpuImage::from_rgba32f(device, queue, ext_w, ext_h, &tile_data)?;

                let uv_rect = [
                    ext_x as f32 / width as f32,
                    ext_y as f32 / height as f32,
                    ext_w as f32 / width as f32,
                    ext_h as f32 / height as f32,
                ];
                let tile_rgba8 = self.render_to_rgba8_with_colorspace_and_uv_rect(
                    device,
                    queue,
                    &tile_gpu_image,
                    edit_stack,
                    color_space,
                    uv_rect,
                )?;

                // Copy only the inner (non-halo) tile bytes into final image buffer
                for row in 0..tile_h {
                    let dst_y = tile_y + row;
                    let dst_start = ((dst_y * width + tile_x) * 4) as usize;
                    let dst_end = dst_start + (tile_w * 4) as usize;
                    let tile_row_start = (((inner_y + row) * ext_w + inner_x) * 4) as usize;
                    let tile_row_end = tile_row_start + (tile_w * 4) as usize;

                    final_rgba8[dst_start..dst_end]
                        .copy_from_slice(&tile_rgba8[tile_row_start..tile_row_end]);
                }
            }
        }

        Ok(final_rgba8)
    }
}

// ----------------------------------------------------------------------------
// Headless GPU Context & Golden Image Tests
// ----------------------------------------------------------------------------

pub struct HeadlessGpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
}

impl HeadlessGpu {
    pub fn new() -> Option<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .or_else(|_| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: None,
                force_fallback_adapter: true,
                apply_limit_buckets: false,
            }))
        })
        .ok()?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("beam_headless_device"),
            ..Default::default()
        }))
        .ok()?;

        Some(Self {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_shaders_wgsl_syntax_valid() {
        if let Some(gpu) = HeadlessGpu::new() {
            let mut pipeline = EditPipeline::new(&gpu.device, wgpu::TextureFormat::Rgba8Unorm);
            let input_data = vec![0.25f32; 16 * 4];
            let gpu_img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, 4, 4, &input_data)
                .expect("Failed to create input image");

            let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("test_target"),
                size: wgpu::Extent3d {
                    width: 4,
                    height: 4,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

            let mut edit_stack = EditStack::default();
            edit_stack.exposure.ev = 1.0; // Double linear exposure

            pipeline.render(&gpu.device, &gpu.queue, &gpu_img, &target_view, &edit_stack);
        } else {
            println!("Skipping headless GPU test (no compatible GPU adapter available)");
        }
    }

    #[test]
    fn test_golden_exposure_shader_stage() {
        let Some(gpu) = HeadlessGpu::new() else {
            println!("Skipping golden exposure test (no GPU adapter)");
            return;
        };

        let width = 2u32;
        let height = 2u32;
        let input_val = 0.25f32;
        let input_data = vec![input_val; (width * height * 4) as usize];

        let input_img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
            .expect("Failed to create input image");

        let target_tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("golden_exposure_target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let stage = StagePass::new(
            &gpu.device,
            "golden_exposure",
            EXPOSURE_SHADER_SRC,
            ExposureUniforms {
                multiplier: 2.0, // +1 EV -> 2x linear multiplier
                enabled: 1,
                _pad0: 0,
                _pad1: 0,
            },
            wgpu::TextureFormat::Rgba32Float,
        );

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("golden_encoder"),
            });
        stage.render(&gpu.device, &mut encoder, &input_img.view, &target_view);

        // Copy rendered texture to staging buffer for readback verification
        let bytes_per_pixel = 16u32;
        let unaligned_bytes_per_row = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let bytes_per_row = (unaligned_bytes_per_row + align - 1) & !(align - 1);

        let staging_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("golden_readback_buffer"),
            size: (bytes_per_row as u64) * (height as u64),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        gpu.queue.submit(std::iter::once(encoder.finish()));

        // Map buffer and read back values
        let slice = staging_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            tx.send(res).unwrap();
        });

        gpu.instance.poll_all(true);
        rx.recv().unwrap().expect("Failed to map staging buffer");

        {
            let data = slice
                .get_mapped_range()
                .expect("Failed to get mapped range");
            let floats: &[f32] = bytemuck::cast_slice(&data[..16]);
            // Input was 0.25, with 2.0x multiplier the output must be exactly 0.50
            assert!(
                (floats[0] - 0.50).abs() < 1e-4,
                "Golden image test failed: expected 0.50, got {}",
                floats[0]
            );
            assert!(
                (floats[1] - 0.50).abs() < 1e-4,
                "Golden image test failed: expected 0.50, got {}",
                floats[1]
            );
            assert!(
                (floats[2] - 0.50).abs() < 1e-4,
                "Golden image test failed: expected 0.50, got {}",
                floats[2]
            );
        }
        staging_buffer.unmap();
    }

    #[test]
    fn test_golden_pipeline_with_all_adjustments() {
        let Some(gpu) = HeadlessGpu::new() else {
            println!("Skipping pipeline test (no GPU adapter)");
            return;
        };

        let width = 4u32;
        let height = 4u32;
        let mut input_data = vec![0.0f32; (width * height * 4) as usize];
        for i in 0..(width * height) as usize {
            input_data[i * 4] = 0.35;
            input_data[i * 4 + 1] = 0.35;
            input_data[i * 4 + 2] = 0.35;
            input_data[i * 4 + 3] = 1.0;
        }
        let input_img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
            .expect("Failed to create input image");

        let target_tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("all_adjustments_target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let mut pipeline = EditPipeline::new(&gpu.device, wgpu::TextureFormat::Rgba8Unorm);
        let mut edit_stack = EditStack::default();
        edit_stack.exposure.ev = 0.5;
        edit_stack.highlights_shadows.highlights = -30.0;
        edit_stack.highlights_shadows.shadows = 40.0;
        edit_stack.tone_curve.points = vec![
            beam_core::sidecar::ToneCurvePoint { x: 0.0, y: 0.0 },
            beam_core::sidecar::ToneCurvePoint { x: 0.5, y: 0.6 },
            beam_core::sidecar::ToneCurvePoint { x: 1.0, y: 1.0 },
        ];
        edit_stack.hsl.bands[0].saturation = 25.0; // Red sat boost

        pipeline.render(
            &gpu.device,
            &gpu.queue,
            &input_img,
            &target_view,
            &edit_stack,
        );

        // Verify output can be copied and is valid
        let bytes_per_pixel = 4u32;
        let unaligned_bytes_per_row = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let bytes_per_row = (unaligned_bytes_per_row + align - 1) & !(align - 1);

        let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging"),
            size: (bytes_per_row as u64) * (height as u64),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("readback_encoder"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit(std::iter::once(encoder.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            tx.send(res).unwrap();
        });
        gpu.instance.poll_all(true);
        rx.recv().unwrap().expect("Failed to map staging buffer");

        {
            let data = slice
                .get_mapped_range()
                .expect("Failed to get mapped range");
            let pixels = &data[..4];
            assert!(pixels[0] > 0, "Red channel should be non-zero");
            assert!(pixels[3] == 255, "Alpha channel should be 255");
        }
        staging.unmap();
    }

    #[test]
    fn test_render_to_rgba8_export() {
        let Some(gpu) = HeadlessGpu::new() else {
            println!("Skipping render_to_rgba8 test (no GPU adapter)");
            return;
        };

        let width = 5u32; // Not a multiple of 64 (256 bytes) to verify row padding alignment!
        let height = 3u32;
        let input_val = 0.5f32;
        let mut input_data = vec![input_val; (width * height * 4) as usize];
        // Set alpha to 1.0
        for i in 0..(width * height) as usize {
            input_data[i * 4 + 3] = 1.0;
        }

        let input_img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
            .expect("Failed to create input image");

        let mut pipeline = EditPipeline::new(&gpu.device, wgpu::TextureFormat::Rgba8Unorm);
        let mut edit_stack = EditStack::default();
        edit_stack.exposure.ev = 0.5;

        let rgba = pipeline
            .render_to_rgba8(&gpu.device, &gpu.queue, &input_img, &edit_stack)
            .expect("render_to_rgba8 failed");

        assert_eq!(rgba.len(), (width * height * 4) as usize);
        // Alpha should be 255
        for i in 0..(width * height) as usize {
            assert_eq!(
                rgba[i * 4 + 3],
                255,
                "Alpha channel should be 255 at pixel {i}"
            );
            assert!(rgba[i * 4] > 0, "Red channel should be positive");
        }
    }

    #[test]
    fn test_render_tiled_matches_monolithic() {
        let Some(gpu) = HeadlessGpu::new() else {
            println!("Skipping tiled test (no GPU adapter)");
            return;
        };

        let width = 17u32;
        let height = 13u32;
        let mut input_data = vec![0.0f32; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let idx = ((y * width + x) * 4) as usize;
                input_data[idx] = (x as f32) / (width as f32);
                input_data[idx + 1] = (y as f32) / (height as f32);
                input_data[idx + 2] = 0.4f32;
                input_data[idx + 3] = 1.0f32;
            }
        }

        let input_img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
            .expect("Failed to create input image");

        let mut pipeline = EditPipeline::new(&gpu.device, wgpu::TextureFormat::Rgba8Unorm);
        let mut edit_stack = EditStack::default();
        edit_stack.exposure.ev = 0.5;
        edit_stack.contrast.contrast = 20.0;

        let monolithic = pipeline
            .render_to_rgba8(&gpu.device, &gpu.queue, &input_img, &edit_stack)
            .expect("monolithic render failed");

        let tiled = pipeline
            .render_tiled_to_rgba8(
                &gpu.device,
                &gpu.queue,
                width,
                height,
                &input_data,
                &edit_stack,
                beam_core::ExportColorSpace::Srgb,
                7, // Small tile size to force 3x2 grid of tiles
            )
            .expect("tiled render failed");

        assert_eq!(monolithic.len(), tiled.len());
        assert_eq!(
            monolithic, tiled,
            "Tiled render must be bit-exact with monolithic render!"
        );
    }

    #[test]
    fn test_gpu_zero_dimension_handling() {
        let Some(gpu) = HeadlessGpu::new() else {
            return;
        };

        let res = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, 0, 100, &[]);
        assert!(res.is_err(), "Must reject zero width");

        let res2 = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, 100, 0, &[]);
        assert!(res2.is_err(), "Must reject zero height");
    }

    #[test]
    fn test_render_tiled_matches_monolithic_moderate_image() {
        let Some(gpu) = HeadlessGpu::new() else {
            println!("Skipping moderate tiled test (no GPU adapter)");
            return;
        };

        let width = 256u32;
        let height = 256u32;
        let mut input_data = vec![0.0f32; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let idx = ((y * width + x) * 4) as usize;
                input_data[idx] = (x as f32) / (width as f32);
                input_data[idx + 1] = (y as f32) / (height as f32);
                input_data[idx + 2] = ((x + y) as f32) / ((width + height) as f32);
                input_data[idx + 3] = 1.0f32;
            }
        }

        let input_img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
            .expect("Failed to create input image");

        let mut pipeline = EditPipeline::new(&gpu.device, wgpu::TextureFormat::Rgba8Unorm);
        let mut edit_stack = EditStack::default();
        edit_stack.exposure.ev = 0.75;
        edit_stack.contrast.contrast = 25.0;
        edit_stack.white_balance.temperature = 1000.0;

        let monolithic = pipeline
            .render_to_rgba8(&gpu.device, &gpu.queue, &input_img, &edit_stack)
            .expect("monolithic render failed");

        let tiled = pipeline
            .render_tiled_to_rgba8(
                &gpu.device,
                &gpu.queue,
                width,
                height,
                &input_data,
                &edit_stack,
                beam_core::ExportColorSpace::Srgb,
                64, // 4x4 tiles of 64x64
            )
            .expect("tiled render failed");

        assert_eq!(monolithic.len(), tiled.len());
        // Stated tolerance: bit-exact equality (0 difference) between monolithic and tiled pipelines
        assert_eq!(
            monolithic, tiled,
            "Moderate tiled render (256x256) must be bit-exact with monolithic render"
        );
    }

    #[test]
    fn test_gpu_oversized_dimension_handling() {
        let Some(gpu) = HeadlessGpu::new() else {
            return;
        };

        let max_dim = gpu.device.limits().max_texture_dimension_2d;
        assert!(max_dim > 0);

        let res_w = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, max_dim + 1, 1, &[]);
        assert!(
            res_w.is_err(),
            "Must reject width exceeding max_texture_dimension_2d"
        );

        let res_h = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, 1, max_dim + 1, &[]);
        assert!(
            res_h.is_err(),
            "Must reject height exceeding max_texture_dimension_2d"
        );
    }

    #[test]
    fn test_gpu_memory_release_on_image_switch() {
        let Some(gpu) = HeadlessGpu::new() else {
            return;
        };

        let width = 128u32;
        let height = 128u32;
        let dummy_data = vec![0.5f32; (width * height * 4) as usize];
        let mut pipeline = EditPipeline::new(&gpu.device, wgpu::TextureFormat::Rgba8Unorm);
        let edit_stack = EditStack::default();

        // Simulate user rapidly switching between 30 different photos in editor
        for _ in 0..30 {
            let img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &dummy_data)
                .expect("Image allocation must succeed without leaking GPU memory");
            let rendered = pipeline.render_to_rgba8(&gpu.device, &gpu.queue, &img, &edit_stack);
            assert!(rendered.is_ok(), "Render must succeed");
            // `img` and `rendered` drop here at end of loop iteration, releasing GPU textures
        }
    }

    #[test]
    fn test_lut3d_identity_cube_preserves_colors() {
        let Some(gpu) = HeadlessGpu::new() else {
            return;
        };

        let tmp_dir = std::env::temp_dir().join("beam_lut3d_test");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let lut_path = tmp_dir.join("identity_2.cube");
        std::fs::write(
            &lut_path,
            "LUT_3D_SIZE 2\n0.0 0.0 0.0\n1.0 0.0 0.0\n0.0 1.0 0.0\n1.0 1.0 0.0\n0.0 0.0 1.0\n1.0 0.0 1.0\n0.0 1.0 1.0\n1.0 1.0 1.0\n",
        )
        .unwrap();

        let width = 8u32;
        let height = 8u32;
        let input_data: Vec<f32> = [0.25f32, 0.50, 0.15, 1.0].repeat((width * height) as usize);
        let img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
            .expect("Failed to create input image");

        let mut pipeline = EditPipeline::new(&gpu.device, wgpu::TextureFormat::Rgba8Unorm);
        let baseline = pipeline
            .render_to_rgba8(&gpu.device, &gpu.queue, &img, &EditStack::default())
            .expect("baseline render failed");

        let mut stack_with_lut = EditStack::default();
        stack_with_lut.lut.enabled = true;
        stack_with_lut.lut.intensity = 1.0;
        stack_with_lut.lut.path = Some(lut_path.to_string_lossy().into_owned());

        let with_lut = pipeline
            .render_to_rgba8(&gpu.device, &gpu.queue, &img, &stack_with_lut)
            .expect("LUT render failed");

        for (a, b) in baseline.iter().zip(with_lut.iter()) {
            assert!(
                (*a as i32 - *b as i32).abs() <= 2,
                "Identity 3D .cube LUT must preserve colors (baseline={a}, with_lut={b})"
            );
        }
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_render_tiled_matches_monolithic_with_spatial_stages() {
        let Some(gpu) = HeadlessGpu::new() else {
            return;
        };

        let width = 520u32;
        let height = 520u32;
        let mut input_data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                input_data.push(((x * 7 + y * 3) % 256) as f32 / 255.0 * 0.6 + 0.05);
                input_data.push(((x * 5 + y * 11) % 256) as f32 / 255.0 * 0.6 + 0.05);
                input_data.push(((x * 13 + y * 2) % 256) as f32 / 255.0 * 0.6 + 0.05);
                input_data.push(1.0);
            }
        }
        let input_img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
            .expect("Failed to create input image");

        let mut pipeline = EditPipeline::new(&gpu.device, wgpu::TextureFormat::Rgba8Unorm);
        let mut edit_stack = EditStack::default();
        edit_stack.detail.sharpening = 0.6;
        edit_stack.detail.clarity = 0.4;
        edit_stack.vignette_grain.vignette_amount = -60.0;
        edit_stack.vignette_grain.vignette_midpoint = 50.0;
        edit_stack.vignette_grain.vignette_feather = 50.0;
        edit_stack.masks.push(beam_core::sidecar::MaskLayer {
            id: 1,
            name: "Grad".into(),
            enabled: true,
            shape: beam_core::sidecar::MaskShape::LinearGradient {
                start: [0.0, 0.2],
                end: [1.0, 0.8],
            },
            exposure: 0.8,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            temperature: 0.0,
            tint: 0.0,
            saturation: 0.0,
        });

        let monolithic = pipeline
            .render_to_rgba8(&gpu.device, &gpu.queue, &input_img, &edit_stack)
            .expect("monolithic render failed");

        let tiled = pipeline
            .render_tiled_to_rgba8(
                &gpu.device,
                &gpu.queue,
                width,
                height,
                &input_data,
                &edit_stack,
                beam_core::ExportColorSpace::Srgb,
                256,
            )
            .expect("tiled render failed");

        assert_eq!(monolithic.len(), tiled.len());
        let max_diff = monolithic
            .iter()
            .zip(tiled.iter())
            .map(|(a, b)| (*a as i32 - *b as i32).abs())
            .max()
            .unwrap_or(0);
        assert!(
            max_diff <= 1,
            "Tiled render with detail/vignette/masks must match monolithic render (max_diff={max_diff})"
        );
    }
}
