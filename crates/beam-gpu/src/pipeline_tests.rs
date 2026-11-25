use super::*;

#[test]
fn test_choose_renderable_float_format() {
    let req = WORKING_TEXTURE_USAGES;
    let full = wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_SRC;
    let sample_only = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;

    assert_eq!(
        choose_renderable_float_format(full, full, req),
        Some(wgpu::TextureFormat::Rgba32Float)
    );
    assert_eq!(
        choose_renderable_float_format(sample_only, full, req),
        Some(wgpu::TextureFormat::Rgba16Float)
    );
    assert_eq!(
        choose_renderable_float_format(sample_only, sample_only, req),
        None
    );
}

#[test]
fn test_f16_bits_to_f32_standard_values() {
    assert_eq!(f16_bits_to_f32(0x0000), 0.0);
    assert_eq!(f16_bits_to_f32(0x3C00), 1.0);
    assert_eq!(f16_bits_to_f32(0xBC00), -1.0);
    assert!((f16_bits_to_f32(0x3800) - 0.5).abs() < 1e-6);
}

#[test]
fn test_all_shaders_wgsl_syntax_valid() {
    if let Some(gpu) = HeadlessGpu::new() {
        let mut pipeline = gpu
            .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
            .expect("Failed to create EditPipeline");
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

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
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

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
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

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
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

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
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
    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
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

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
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

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
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
        luma_min: 0.0,
        luma_max: 1.0,
        luma_smoothness: 0.15,
        color_range_enabled: false,
        color_range_hue: 0.0,
        color_range_tolerance: 35.0,
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

#[test]
fn test_16bit_export_preserves_smooth_gradient() {
    let Some(gpu) = HeadlessGpu::new() else {
        return;
    };

    let width = 64u32;
    let height = 4u32;
    let mut input_data = Vec::with_capacity((width * height * 4) as usize);
    for _y in 0..height {
        for x in 0..width {
            // Very subtle 0.20..0.205 linear ramp where 8-bit collapses to ~1-2 levels
            let v = 0.20 + (x as f32 / (width - 1) as f32) * 0.005;
            input_data.extend_from_slice(&[v, v, v, 1.0]);
        }
    }

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
    let out16 = pipeline
        .render_tiled_to_rgba16(
            &gpu.device,
            &gpu.queue,
            width,
            height,
            &input_data,
            &EditStack::default(),
            beam_core::ExportColorSpace::Srgb,
            256,
        )
        .expect("16-bit render failed");

    let mut unique_vals = std::collections::BTreeSet::new();
    for x in 0..width {
        unique_vals.insert(out16[(x * 4) as usize]);
    }
    assert!(
        unique_vals.len() > 16,
        "16-bit readback must preserve subtle tonal steps (found only {} unique values)",
        unique_vals.len()
    );
}

#[test]
fn test_lens_correction_vignette_and_distortion() {
    let Some(gpu) = HeadlessGpu::new() else {
        return;
    };

    let width = 32u32;
    let height = 32u32;
    let input_data = [0.2f32, 0.2, 0.2, 1.0].repeat((width * height) as usize);
    let img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
        .expect("Failed to create input image");

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
    let mut stack = EditStack::default();
    stack.lens.vignette = 60.0;

    let out = pipeline
        .render_to_rgba8(&gpu.device, &gpu.queue, &img, &stack)
        .expect("lens render failed");

    let center_idx = ((16 * width + 16) * 4) as usize;
    let corner_idx = 0usize;
    assert!(
        out[corner_idx] > out[center_idx] + 15,
        "Positive lens vignette correction must brighten corners (corner={}, center={})",
        out[corner_idx],
        out[center_idx]
    );
}

#[test]
fn test_soft_proofing_gamut_warning() {
    let Some(gpu) = HeadlessGpu::new() else {
        return;
    };

    let width = 8u32;
    let height = 8u32;
    // Pure Rec.2020 saturated green (100% inside Rec.2020, out-of-gamut for sRGB)
    let input_data = [0.0f32, 0.7, 0.0, 1.0].repeat((width * height) as usize);
    let img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
        .expect("Failed to create input image");

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
    let stack = EditStack::default();

    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("soft_proof_test_tex"),
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
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());

    // Render with sRGB soft proof + gamut warning -> should highlight magenta (0.95, 0.20, 0.85)
    pipeline.render_with_proof_options(
        &gpu.device,
        &gpu.queue,
        &img,
        &view,
        &stack,
        false,
        false,
        false,
        Some(beam_core::ExportColorSpace::Srgb),
        true,
        false,
    );

    let bytes_per_row = 256u32;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("soft_proof_readback"),
        size: (bytes_per_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
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
    gpu.queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();

    let data = slice.get_mapped_range().unwrap();
    let r = data[0];
    let g = data[1];
    let b = data[2];
    drop(data);
    readback.unmap();

    assert!(
            r > 200 && g < 100 && b > 180,
            "Out-of-gamut Rec.2020 green under sRGB soft proof must show magenta gamut warning (got RGB={r},{g},{b})"
        );
}

#[test]
fn test_lens_perspective_keystone_correction() {
    let Some(gpu) = HeadlessGpu::new() else {
        return;
    };

    let width = 32u32;
    let height = 32u32;
    let input_data = [0.4f32, 0.4, 0.4, 1.0].repeat((width * height) as usize);
    let img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
        .expect("Failed to create input image");

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
    let mut stack = EditStack::default();
    stack.lens.perspective_v = 80.0;

    let out = pipeline
        .render_to_rgba8(&gpu.device, &gpu.queue, &img, &stack)
        .expect("perspective render failed");

    let top_corner_idx = 0usize;
    let center_idx = ((16 * width + 16) * 4) as usize;
    assert_eq!(
        out[top_corner_idx], 0,
        "Positive vertical perspective must pull top corners out-of-bounds to black"
    );
    assert!(
        out[center_idx] > 100,
        "Center pixel must remain intact under perspective warp (got {})",
        out[center_idx]
    );
}

#[test]
fn test_mask_luminance_range_restriction() {
    let Some(gpu) = HeadlessGpu::new() else {
        return;
    };

    let width = 8u32;
    let height = 8u32;
    // Left half: dark (0.02), Right half: bright (0.50)
    let mut input_data = Vec::with_capacity((width * height * 4) as usize);
    for _y in 0..height {
        for x in 0..width {
            let v = if x < width / 2 { 0.02 } else { 0.50 };
            input_data.extend_from_slice(&[v, v, v, 1.0]);
        }
    }
    let img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
        .expect("Failed to create input image");

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
    let mut stack_unmasked = EditStack::default();
    let baseline = pipeline
        .render_to_rgba8(&gpu.device, &gpu.queue, &img, &stack_unmasked)
        .expect("baseline render failed");

    // Radial mask covering the entire image, restricted to highlights (luma_min = 0.55)
    let mut mask = beam_core::sidecar::MaskLayer::new_radial(1, "Highlights".to_string());
    if let beam_core::sidecar::MaskShape::RadialGradient { radius, .. } = &mut mask.shape {
        *radius = [2.0, 2.0];
    }
    mask.exposure = 1.5;
    mask.luma_min = 0.55;
    mask.luma_max = 1.0;
    mask.luma_smoothness = 0.08;
    stack_unmasked.masks.push(mask);

    let masked = pipeline
        .render_to_rgba8(&gpu.device, &gpu.queue, &img, &stack_unmasked)
        .expect("luma mask render failed");

    let dark_idx = 0usize;
    let bright_idx = ((width - 1) * 4) as usize;
    assert!(
        (masked[dark_idx] as i32 - baseline[dark_idx] as i32).abs() <= 1,
        "Dark pixel should be unaffected by highlight-restricted mask (baseline={}, masked={})",
        baseline[dark_idx],
        masked[dark_idx]
    );
    assert!(
        masked[bright_idx] > baseline[bright_idx] + 20,
        "Bright pixel should be brightened by highlight-restricted mask (baseline={}, masked={})",
        baseline[bright_idx],
        masked[bright_idx]
    );
}

#[test]
fn test_focus_peaking_overlay() {
    let Some(gpu) = HeadlessGpu::new() else {
        return;
    };

    let width = 16u32;
    let height = 16u32;
    // Sharp vertical edge at x = 8 (left dark 0.02, right bright 0.80)
    let mut input_data = Vec::with_capacity((width * height * 4) as usize);
    for _y in 0..height {
        for x in 0..width {
            let v = if x < 8 { 0.02 } else { 0.80 };
            input_data.extend_from_slice(&[v, v, v, 1.0]);
        }
    }
    let img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
        .expect("Failed to create input image");

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
    let stack = EditStack::default();

    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("focus_peaking_test_tex"),
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
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());

    pipeline.render_with_proof_options(
        &gpu.device,
        &gpu.queue,
        &img,
        &view,
        &stack,
        false,
        false,
        false,
        None,
        false,
        true,
    );

    let bytes_per_row = 256u32;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("focus_peaking_readback"),
        size: (bytes_per_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
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
    gpu.queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();

    let data = slice.get_mapped_range().unwrap();
    let row8_offset = (8 * bytes_per_row) as usize;
    // Flat left pixel at x=1 should stay dark grey (not green)
    let flat_r = data[row8_offset + 4];
    let flat_g = data[row8_offset + 5];
    // Edge pixel at x=8 should be highlighted green (0.15, 0.95, 0.45)
    let edge_r = data[row8_offset + 8 * 4];
    let edge_g = data[row8_offset + 8 * 4 + 1];
    drop(data);
    readback.unmap();

    assert!(
        flat_g < 100 && (flat_r as i32 - flat_g as i32).abs() <= 2,
        "Flat dark region must not trigger focus peaking (got R={flat_r}, G={flat_g})"
    );
    assert!(
        edge_g > 200 && edge_r < 80,
        "Sharp step edge must trigger green focus peaking overlay (got R={edge_r}, G={edge_g})"
    );
}

#[test]
fn test_mask_brush_and_color_range() {
    let Some(gpu) = HeadlessGpu::new() else {
        return;
    };

    let width = 16u32;
    let height = 16u32;
    // Left half: pure red, Right half: pure blue
    let mut input_data = Vec::with_capacity((width * height * 4) as usize);
    for _y in 0..height {
        for x in 0..width {
            if x < 8 {
                input_data.extend_from_slice(&[0.5, 0.05, 0.05, 1.0]);
            } else {
                input_data.extend_from_slice(&[0.05, 0.05, 0.5, 1.0]);
            }
        }
    }
    let img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
        .expect("Failed to create input image");

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
    let mut stack = EditStack::default();
    let baseline = pipeline
        .render_to_rgba8(&gpu.device, &gpu.queue, &img, &stack)
        .expect("baseline render failed");

    // Brush covering center (0.5, 0.5) with radius 0.6, restricted to Red hue (0 deg)
    let mut mask = beam_core::sidecar::MaskLayer::new_brush(1, "RedBrush".to_string());
    if let beam_core::sidecar::MaskShape::Brush {
        strokes, radius, ..
    } = &mut mask.shape
    {
        strokes.push([0.5, 0.5]);
        *radius = 0.6;
    }
    mask.exposure = 1.5;
    mask.color_range_enabled = true;
    mask.color_range_hue = 0.0;
    mask.color_range_tolerance = 30.0;
    stack.masks.push(mask);

    let masked = pipeline
        .render_to_rgba8(&gpu.device, &gpu.queue, &img, &stack)
        .expect("brush+color render failed");

    let red_idx = ((8 * width + 4) * 4) as usize;
    let blue_idx = ((8 * width + 12) * 4) as usize;
    assert!(
        masked[red_idx] > baseline[red_idx] + 20,
        "Red pixel inside brush must brighten (baseline={}, masked={})",
        baseline[red_idx],
        masked[red_idx]
    );
    assert!(
        (masked[blue_idx + 2] as i32 - baseline[blue_idx + 2] as i32).abs() <= 1,
        "Blue pixel outside red hue range must remain unchanged (baseline={}, masked={})",
        baseline[blue_idx + 2],
        masked[blue_idx + 2]
    );
}

#[test]
fn test_camera_profile_monochrome() {
    let Some(gpu) = HeadlessGpu::new() else {
        return;
    };

    let width = 8u32;
    let height = 8u32;
    let input_data = [0.6f32, 0.2, 0.1, 1.0].repeat((width * height) as usize);
    let img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
        .expect("Failed to create input image");

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
    let stack = EditStack {
        camera_profile: beam_core::sidecar::CameraProfile::Monochrome,
        ..Default::default()
    };

    let out = pipeline
        .render_to_rgba8(&gpu.device, &gpu.queue, &img, &stack)
        .expect("monochrome render failed");

    assert!(
        (out[0] as i32 - out[1] as i32).abs() <= 1 && (out[1] as i32 - out[2] as i32).abs() <= 1,
        "Monochrome camera profile must produce neutral grey (got RGB={},{},{})",
        out[0],
        out[1],
        out[2]
    );
}

#[test]
fn test_ai_denoise_smooths_high_frequency_noise() {
    let Some(gpu) = HeadlessGpu::new() else {
        return;
    };

    let width = 16u32;
    let height = 16u32;
    // High-frequency checkerboard noise around 0.25
    let mut input_data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let v = if (x + y) % 2 == 0 { 0.22 } else { 0.28 };
            input_data.extend_from_slice(&[v, v, v, 1.0]);
        }
    }
    let img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, width, height, &input_data)
        .expect("Failed to create input image");

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
    let baseline = pipeline
        .render_to_rgba8(&gpu.device, &gpu.queue, &img, &EditStack::default())
        .expect("baseline render failed");

    let mut stack_nr = EditStack::default();
    stack_nr.detail.ai_denoise = 1.0;
    let denoised = pipeline
        .render_to_rgba8(&gpu.device, &gpu.queue, &img, &stack_nr)
        .expect("ai denoise render failed");

    let idx0 = ((8 * width + 8) * 4) as usize;
    let idx1 = ((8 * width + 9) * 4) as usize;
    let base_diff = (baseline[idx0] as i32 - baseline[idx1] as i32).abs();
    let nr_diff = (denoised[idx0] as i32 - denoised[idx1] as i32).abs();
    assert!(
            nr_diff < base_diff,
            "AI denoise must reduce high-frequency noise amplitude (base_diff={base_diff}, nr_diff={nr_diff})"
        );
}

#[test]
fn test_render_tiled_matches_monolithic_with_lens_warp() {
    let Some(gpu) = HeadlessGpu::new() else {
        return;
    };
    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");

    let w = 320u32;
    let h = 320u32;
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            data.push((x as f32 / w as f32) * 0.8 + 0.05);
            data.push((y as f32 / h as f32) * 0.8 + 0.05);
            data.push((((x ^ y) & 31) as f32 / 31.0) * 0.5 + 0.1);
            data.push(1.0);
        }
    }

    let mut stack = EditStack::default();
    stack.lens.enabled = true;
    stack.lens.distortion = 35.0;
    stack.lens.ca_red_cyan = 50.0;
    stack.lens.perspective_v = 25.0;

    let gpu_img = GpuImage::from_rgba32f(&gpu.device, &gpu.queue, w, h, &data)
        .expect("upload lens test image");
    let monolithic = pipeline
        .render_to_rgba8_with_colorspace(
            &gpu.device,
            &gpu.queue,
            &gpu_img,
            &stack,
            beam_core::ExportColorSpace::Srgb,
        )
        .expect("monolithic lens render");
    let tiled = pipeline
        .render_tiled_to_rgba8(
            &gpu.device,
            &gpu.queue,
            w,
            h,
            &data,
            &stack,
            beam_core::ExportColorSpace::Srgb,
            256,
        )
        .expect("tiled lens render");

    assert_eq!(monolithic.len(), tiled.len());
    let max_diff = monolithic
        .iter()
        .zip(tiled.iter())
        .map(|(&a, &b)| (a as i32 - b as i32).abs())
        .max()
        .unwrap_or(0);
    assert!(
        max_diff <= 2,
        "Tiled lens warp diverged from monolithic render (max_diff={max_diff})"
    );
}
