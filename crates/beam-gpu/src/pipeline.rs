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
        if data.len() != (width * height * 4) as usize {
            return Err(BeamError::GpuPipeline(format!(
                "Data size mismatch: expected {} floats, got {}",
                width * height * 4,
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
    pub enabled: u32,
    pub _pad0: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ToneCurveUniforms {
    pub lut: [[f32; 4]; 64],
    pub enabled: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
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

pub struct DisplayPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    target_format: wgpu::TextureFormat,
}

impl DisplayPipeline {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("display_transform_shader"),
            source: wgpu::ShaderSource::Wgsl(DISPLAY_SHADER_SRC.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("display_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
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
            target_format,
        }
    }

    pub fn render_to_target(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input_view: &wgpu::TextureView,
        target_view: &wgpu::TextureView,
    ) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("display_render_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(input_view),
            }],
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
// Full Edit Pipeline (Ping-Pong Intermediate Passes)
// ----------------------------------------------------------------------------

pub struct EditPipeline {
    white_balance_stage: StagePass<WhiteBalanceUniforms>,
    exposure_stage: StagePass<ExposureUniforms>,
    highlights_shadows_stage: StagePass<HighlightsShadowsUniforms>,
    contrast_stage: StagePass<ContrastUniforms>,
    tone_curve_stage: StagePass<ToneCurveUniforms>,
    hsl_stage: StagePass<HslUniforms>,
    display_pipeline: DisplayPipeline,

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
                enabled: 1,
                _pad0: 0,
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
                lut: initial_lut,
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

        let display_pipeline = DisplayPipeline::new(device, display_format);

        Self {
            white_balance_stage,
            exposure_stage,
            highlights_shadows_stage,
            contrast_stage,
            tone_curve_stage,
            hsl_stage,
            display_pipeline,
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

    /// Execute the full edit stack from input texture to output display target.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &GpuImage,
        target_view: &wgpu::TextureView,
        edit_stack: &EditStack,
    ) {
        self.ensure_intermediate_textures(device, input.width, input.height);

        // 1. Update uniforms from edit stack
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
                enabled: if edit_stack.highlights_shadows.enabled {
                    1
                } else {
                    0
                },
                _pad0: 0,
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

        let curve_lut = beam_core::color::generate_tone_curve_lut(&edit_stack.tone_curve.points);
        let mut packed_lut = [[0.0f32; 4]; 64];
        for i in 0..64 {
            packed_lut[i] = [
                curve_lut[i * 4],
                curve_lut[i * 4 + 1],
                curve_lut[i * 4 + 2],
                curve_lut[i * 4 + 3],
            ];
        }
        self.tone_curve_stage.update_uniform(
            queue,
            &ToneCurveUniforms {
                lut: packed_lut,
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

        // Stage 3: Highlights & Shadows (pong -> ping)
        self.highlights_shadows_stage
            .render(device, &mut encoder, pong, ping);

        // Stage 4: Contrast (ping -> pong)
        self.contrast_stage.render(device, &mut encoder, ping, pong);

        // Stage 5: Tone Curve (pong -> ping)
        self.tone_curve_stage
            .render(device, &mut encoder, pong, ping);

        // Stage 6: HSL (ping -> pong)
        self.hsl_stage.render(device, &mut encoder, ping, pong);

        queue.submit(std::iter::once(encoder.finish()));

        // Final Stage: Display Transform (pong -> target_view)
        self.display_pipeline
            .render_to_target(device, queue, pong, target_view);
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
            size: (bytes_per_row * height) as u64,
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
            size: (bytes_per_row * height) as u64,
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
}
