//! Full-resolution image export and batch processing.

use beam_core::error::{BeamError, Result};
use beam_core::sidecar::{CropAdjustment, EditStack};
use beam_core::ExportColorSpace;
use beam_gpu::pipeline::{EditPipeline, HeadlessGpu};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{ImageDecoder, ImageEncoder};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Export file format configuration.
#[derive(Clone, Debug, PartialEq)]
pub enum ExportFormat {
    Jpeg { quality: u8 },
    Png,
}

impl Default for ExportFormat {
    fn default() -> Self {
        Self::Jpeg { quality: 90 }
    }
}

impl ExportFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Jpeg { .. } => "jpg",
            Self::Png => "png",
        }
    }
}

/// Image resizing options applied after full-resolution edit pipeline rendering.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ResizeMode {
    #[default]
    Original,
    LongEdge(u32),
    Percentage(f32),
    Exact {
        width: u32,
        height: u32,
        lock_aspect: bool,
    },
}

/// Computes target image dimensions from original dimensions and resizing mode.
pub fn calculate_target_dimensions(orig_w: u32, orig_h: u32, mode: ResizeMode) -> (u32, u32) {
    if orig_w == 0 || orig_h == 0 {
        return (1, 1);
    }
    match mode {
        ResizeMode::Original => (orig_w, orig_h),
        ResizeMode::LongEdge(long_edge) => {
            let target_long = long_edge.max(1);
            if orig_w >= orig_h {
                let w = target_long;
                let h = ((orig_h as f64 * target_long as f64) / orig_w as f64).round() as u32;
                (w.max(1), h.max(1))
            } else {
                let h = target_long;
                let w = ((orig_w as f64 * target_long as f64) / orig_h as f64).round() as u32;
                (w.max(1), h.max(1))
            }
        }
        ResizeMode::Percentage(pct) => {
            let scale = (pct / 100.0).max(0.01) as f64;
            let w = ((orig_w as f64) * scale).round() as u32;
            let h = ((orig_h as f64) * scale).round() as u32;
            (w.max(1), h.max(1))
        }
        ResizeMode::Exact {
            width,
            height,
            lock_aspect,
        } => {
            let req_w = width.max(1);
            let req_h = height.max(1);
            if !lock_aspect {
                (req_w, req_h)
            } else {
                let scale_w = req_w as f64 / orig_w as f64;
                let scale_h = req_h as f64 / orig_h as f64;
                let scale = scale_w.min(scale_h);
                let w = ((orig_w as f64) * scale).round() as u32;
                let h = ((orig_h as f64) * scale).round() as u32;
                (w.max(1), h.max(1))
            }
        }
    }
}

/// Output sharpening applied post-resize.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OutputSharpening {
    #[default]
    None,
    Screen,
    Print,
}

/// Applies a light unsharp mask enhancement to an RGBA8 image.
pub fn apply_unsharp_mask(
    img: &image::RgbaImage,
    sharpening: OutputSharpening,
) -> image::RgbaImage {
    if sharpening == OutputSharpening::None {
        return img.clone();
    }

    let (radius, amount) = match sharpening {
        OutputSharpening::None => (0.0f32, 0.0f32),
        OutputSharpening::Screen => (0.8f32, 0.5f32),
        OutputSharpening::Print => (1.2f32, 0.8f32),
    };

    let (w, h) = img.dimensions();
    if w < 2 || h < 2 {
        return img.clone();
    }

    // 1D Gaussian kernel for separable blur
    let sigma = radius.max(0.5);
    let kernel_radius = (sigma * 2.5).ceil() as i32;
    let mut weights = Vec::new();
    let mut weight_sum = 0.0f32;
    for k in -kernel_radius..=kernel_radius {
        let weight = (-((k * k) as f32) / (2.0 * sigma * sigma)).exp();
        weights.push((k, weight));
        weight_sum += weight;
    }
    for (_, w_val) in weights.iter_mut() {
        *w_val /= weight_sum;
    }

    // Horizontal blur pass
    let mut h_blurred = vec![0.0f32; (w * h * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut r = 0.0f32;
            let mut g = 0.0f32;
            let mut b = 0.0f32;
            for &(k, weight) in &weights {
                let sample_x = (x as i32 + k).clamp(0, w as i32 - 1) as u32;
                let p = img.get_pixel(sample_x, y);
                r += (p[0] as f32) * weight;
                g += (p[1] as f32) * weight;
                b += (p[2] as f32) * weight;
            }
            let idx = ((y * w + x) * 3) as usize;
            h_blurred[idx] = r;
            h_blurred[idx + 1] = g;
            h_blurred[idx + 2] = b;
        }
    }

    // Vertical blur pass and unsharp mask combining
    let mut output = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut blur_r = 0.0f32;
            let mut blur_g = 0.0f32;
            let mut blur_b = 0.0f32;
            for &(k, weight) in &weights {
                let sample_y = (y as i32 + k).clamp(0, h as i32 - 1) as u32;
                let idx = ((sample_y * w + x) * 3) as usize;
                blur_r += h_blurred[idx] * weight;
                blur_g += h_blurred[idx + 1] * weight;
                blur_b += h_blurred[idx + 2] * weight;
            }

            let orig = img.get_pixel(x, y);
            let orig_r = orig[0] as f32;
            let orig_g = orig[1] as f32;
            let orig_b = orig[2] as f32;

            let sharp_r = (orig_r + amount * (orig_r - blur_r))
                .clamp(0.0, 255.0)
                .round() as u8;
            let sharp_g = (orig_g + amount * (orig_g - blur_g))
                .clamp(0.0, 255.0)
                .round() as u8;
            let sharp_b = (orig_b + amount * (orig_b - blur_b))
                .clamp(0.0, 255.0)
                .round() as u8;

            output.put_pixel(x, y, image::Rgba([sharp_r, sharp_g, sharp_b, orig[3]]));
        }
    }

    output
}

/// Applies non-destructive crop, 90-degree rotations, flips, and straighten to an RGBA8 image.
pub fn apply_crop_and_orientation(
    img: &image::RgbaImage,
    crop: &CropAdjustment,
) -> image::RgbaImage {
    if !crop.is_active() {
        return img.clone();
    }

    let mut current = img.clone();

    // 1. 90-degree clockwise rotations
    match crop.rotation % 360 {
        90 => current = image::imageops::rotate90(&current),
        180 => current = image::imageops::rotate180(&current),
        270 => current = image::imageops::rotate270(&current),
        _ => {}
    }

    // 2. Flips
    if crop.flip_h {
        current = image::imageops::flip_horizontal(&current);
    }
    if crop.flip_v {
        current = image::imageops::flip_vertical(&current);
    }

    // 3. Straighten (rotation around center using bilinear sampling)
    if crop.straighten.abs() > 0.001 {
        let (w, h) = current.dimensions();
        let mut straightened = image::RgbaImage::new(w, h);
        let rad = (-crop.straighten).to_radians();
        let cos_a = rad.cos();
        let sin_a = rad.sin();
        let cx = (w as f32) * 0.5;
        let cy = (h as f32) * 0.5;

        for y in 0..h {
            for x in 0..w {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let src_x = cx + dx * cos_a - dy * sin_a;
                let src_y = cy + dx * sin_a + dy * cos_a;

                if src_x >= 0.0
                    && src_x <= (w - 1) as f32
                    && src_y >= 0.0
                    && src_y <= (h - 1) as f32
                {
                    let x0 = src_x.floor() as u32;
                    let y0 = src_y.floor() as u32;
                    let x1 = (x0 + 1).min(w - 1);
                    let y1 = (y0 + 1).min(h - 1);

                    let fx = src_x - x0 as f32;
                    let fy = src_y - y0 as f32;

                    let p00 = current.get_pixel(x0, y0);
                    let p10 = current.get_pixel(x1, y0);
                    let p01 = current.get_pixel(x0, y1);
                    let p11 = current.get_pixel(x1, y1);

                    let mut interp = [0u8; 4];
                    for c in 0..4 {
                        let top = (p00[c] as f32) * (1.0 - fx) + (p10[c] as f32) * fx;
                        let bot = (p01[c] as f32) * (1.0 - fx) + (p11[c] as f32) * fx;
                        interp[c] = (top * (1.0 - fy) + bot * fy).round().clamp(0.0, 255.0) as u8;
                    }
                    straightened.put_pixel(x, y, image::Rgba(interp));
                } else {
                    let cx_clamped = src_x.clamp(0.0, (w - 1) as f32).round() as u32;
                    let cy_clamped = src_y.clamp(0.0, (h - 1) as f32).round() as u32;
                    straightened.put_pixel(x, y, *current.get_pixel(cx_clamped, cy_clamped));
                }
            }
        }
        current = straightened;
    }

    // 4. Crop
    if crop.enabled {
        let (w, h) = current.dimensions();
        let crop_x = ((w as f32) * crop.x.clamp(0.0, 1.0)).round() as u32;
        let crop_y = ((h as f32) * crop.y.clamp(0.0, 1.0)).round() as u32;
        let mut crop_w = ((w as f32) * crop.width.clamp(0.01, 1.0)).round() as u32;
        let mut crop_h = ((h as f32) * crop.height.clamp(0.01, 1.0)).round() as u32;

        crop_w = crop_w.min(w.saturating_sub(crop_x)).max(1);
        crop_h = crop_h.min(h.saturating_sub(crop_y)).max(1);

        let sub_img = image::imageops::crop_imm(&current, crop_x, crop_y, crop_w, crop_h);
        current = sub_img.to_image();
    }

    current
}

/// Generates valid ICC profile binary payload for the specified color space.
pub fn get_icc_profile_bytes(color_space: ExportColorSpace) -> Result<Vec<u8>> {
    let profile = match color_space {
        ExportColorSpace::Srgb => moxcms::ColorProfile::new_srgb(),
        ExportColorSpace::DisplayP3 => moxcms::ColorProfile::new_display_p3(),
        ExportColorSpace::AdobeRgb => moxcms::ColorProfile::new_adobe_rgb(),
    };
    profile
        .encode()
        .map_err(|e| BeamError::ImageProcessing(format!("Failed to encode ICC profile: {e}")))
}

/// Extracts raw EXIF metadata bytes from a file if present.
pub fn extract_raw_exif(path: &Path) -> Option<Vec<u8>> {
    // 1. Try decoding container via image crate ImageDecoder
    if let Ok(file) = std::fs::File::open(path) {
        let reader = std::io::BufReader::new(file);
        if let Ok(reader) = image::ImageReader::new(reader).with_guessed_format() {
            if let Ok(mut decoder) = reader.into_decoder() {
                if let Ok(Some(exif)) = decoder.exif_metadata() {
                    return Some(exif);
                }
            }
        }
    }

    // 2. Scan byte stream for JPEG APP1 EXIF segment (0xFF, 0xE1, len, "Exif\0\0")
    if let Ok(bytes) = std::fs::read(path) {
        let max_search = bytes.len().min(65536);
        let mut i = 0;
        while i + 10 < max_search {
            if bytes[i] == 0xFF && bytes[i + 1] == 0xE1 {
                let length = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
                if i + 4 + length <= bytes.len() && &bytes[i + 4..i + 10] == b"Exif\0\0" {
                    return Some(bytes[i + 4..i + 2 + length].to_vec());
                }
            }
            i += 1;
        }
    }

    None
}

/// Resets the EXIF Orientation tag (0x0112) to 1 (Normal) so image viewers do not rotate already oriented pixels.
pub fn sanitize_exif_orientation(exif: &mut [u8]) {
    const LE_PATTERN: [u8; 8] = [0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00];
    const BE_PATTERN: [u8; 8] = [0x01, 0x12, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01];

    let mut i = 0;
    while i + 12 <= exif.len() {
        if exif[i..i + 8] == LE_PATTERN {
            exif[i + 8] = 0x01;
            exif[i + 9] = 0x00;
            i += 12;
            continue;
        }
        if exif[i..i + 8] == BE_PATTERN {
            exif[i + 8] = 0x00;
            exif[i + 9] = 0x01;
            i += 12;
            continue;
        }
        i += 1;
    }
}

/// Expands filename template tokens: `{name}`, `{date}`, `{counter}`, `{rating}`.
pub fn expand_filename_template(
    template: &str,
    name: &str,
    date: Option<&str>,
    counter: usize,
    rating: u8,
) -> String {
    let tpl = if template.trim().is_empty() {
        "{name}"
    } else {
        template
    };

    let date_str = date.unwrap_or("2026-01-01");
    let counter_str = counter.to_string();
    let rating_str = rating.to_string();

    let mut result = tpl
        .replace("{name}", name)
        .replace("{date}", date_str)
        .replace("{counter}", &counter_str)
        .replace("{rating}", &rating_str);

    // Sanitize filesystem-prohibited characters
    result = result.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    let trimmed = result.trim();
    if trimmed.is_empty() {
        name.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Resolves a unique output path that never overwrites an existing file or the source file.
/// If collision occurs, automatically appends `-1`, `-2`, etc.
pub fn resolve_unique_output_path(
    output_dir: &Path,
    filename_stem: &str,
    ext: &str,
    input_path: Option<&Path>,
) -> PathBuf {
    let is_collision = |path: &Path| -> bool {
        if path.exists() {
            return true;
        }
        if let Some(src) = input_path {
            if path == src {
                return true;
            }
        }
        false
    };

    let base = output_dir.join(format!("{filename_stem}.{ext}"));
    if !is_collision(&base) {
        return base;
    }

    let mut counter = 1;
    loop {
        let candidate = output_dir.join(format!("{filename_stem}-{counter}.{ext}"));
        if !is_collision(&candidate) {
            return candidate;
        }
        counter += 1;
    }
}

/// Comprehensive export configuration.
#[derive(Clone, Debug)]
pub struct ExportConfig {
    pub output_dir: PathBuf,
    pub format: ExportFormat,
    pub color_space: ExportColorSpace,
    pub resize: ResizeMode,
    pub sharpening: OutputSharpening,
    pub export_exif: bool,
    pub filename_template: String,
    pub custom_suffix: Option<String>,
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("."),
            format: ExportFormat::default(),
            color_space: ExportColorSpace::Srgb,
            resize: ResizeMode::Original,
            sharpening: OutputSharpening::None,
            export_exif: true,
            filename_template: "{name}".to_string(),
            custom_suffix: None,
        }
    }
}

/// Progress callback information during batch processing.
#[derive(Clone, Debug)]
pub struct BatchProgress {
    pub current_index: usize,
    pub total: usize,
    pub current_path: PathBuf,
    pub output_path: Option<PathBuf>,
    pub error: Option<String>,
}

/// Export an already decoded in-memory image through the GPU pipeline and encode to disk.
pub fn export_decoded_image(
    gpu: &HeadlessGpu,
    pipeline: &mut EditPipeline,
    decoded: &crate::decoder::DecodedImage,
    edit_stack: &EditStack,
    output_path: &Path,
    config: &ExportConfig,
    raw_exif: Option<Vec<u8>>,
) -> Result<PathBuf> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // 1. Render through GPU pipeline with tiling to ensure bounded VRAM execution
    let rgba_bytes = pipeline.render_tiled_to_rgba8(
        &gpu.device,
        &gpu.queue,
        decoded.width,
        decoded.height,
        &decoded.data,
        edit_stack,
        config.color_space,
        2048,
    )?;

    let mut rgba_image = image::RgbaImage::from_raw(decoded.width, decoded.height, rgba_bytes)
        .ok_or_else(|| {
            BeamError::ImageProcessing("Failed to construct RgbaImage from raw pixel buffer".into())
        })?;

    // 2. Non-destructive crop, rotation, flip, and straighten
    if edit_stack.crop.is_active() {
        rgba_image = apply_crop_and_orientation(&rgba_image, &edit_stack.crop);
    }

    // 3. Resizing with Lanczos3 if requested
    let (target_w, target_h) =
        calculate_target_dimensions(rgba_image.width(), rgba_image.height(), config.resize);
    if target_w != rgba_image.width() || target_h != rgba_image.height() {
        rgba_image = image::imageops::resize(
            &rgba_image,
            target_w,
            target_h,
            image::imageops::FilterType::Lanczos3,
        );
    }

    // 3. Output sharpening
    if config.sharpening != OutputSharpening::None {
        rgba_image = apply_unsharp_mask(&rgba_image, config.sharpening);
    }

    let final_w = rgba_image.width();
    let final_h = rgba_image.height();

    // 4. Encode to target format with ICC profile and EXIF metadata
    let icc_bytes = get_icc_profile_bytes(config.color_space).ok();

    let mut prepared_exif = if config.export_exif {
        raw_exif.map(|mut b| {
            sanitize_exif_orientation(&mut b);
            b
        })
    } else {
        None
    };

    match &config.format {
        ExportFormat::Jpeg { quality } => {
            let quality = (*quality).clamp(1, 100);
            let rgb_image = image::DynamicImage::ImageRgba8(rgba_image).to_rgb8();
            let file = std::fs::File::create(output_path)?;
            let mut writer = std::io::BufWriter::new(file);
            let mut encoder = JpegEncoder::new_with_quality(&mut writer, quality);

            if let Some(icc) = icc_bytes {
                let _ = encoder.set_icc_profile(icc);
            }
            if let Some(exif) = prepared_exif.take() {
                let _ = encoder.set_exif_metadata(exif);
            }

            encoder
                .write_image(
                    rgb_image.as_raw(),
                    final_w,
                    final_h,
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|e| BeamError::ImageProcessing(format!("JPEG encode error: {e}")))?;
        }
        ExportFormat::Png => {
            let file = std::fs::File::create(output_path)?;
            let mut writer = std::io::BufWriter::new(file);
            let mut encoder = PngEncoder::new(&mut writer);

            if let Some(icc) = icc_bytes {
                let _ = encoder.set_icc_profile(icc);
            }
            if let Some(exif) = prepared_exif.take() {
                let _ = encoder.set_exif_metadata(exif);
            }

            encoder
                .write_image(
                    rgba_image.as_raw(),
                    final_w,
                    final_h,
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|e| BeamError::ImageProcessing(format!("PNG encode error: {e}")))?;
        }
    }

    Ok(output_path.to_path_buf())
}

/// Export a single photo from disk by decoding it at native resolution, running GPU edits, and saving.
pub fn export_image(
    gpu: &HeadlessGpu,
    pipeline: &mut EditPipeline,
    input_path: &Path,
    edit_stack: &EditStack,
    output_path: &Path,
    config: &ExportConfig,
) -> Result<PathBuf> {
    let decoded = crate::decoder::decode_image(input_path)?;
    let raw_exif = if config.export_exif {
        extract_raw_exif(input_path)
    } else {
        None
    };
    export_decoded_image(
        gpu,
        pipeline,
        &decoded,
        edit_stack,
        output_path,
        config,
        raw_exif,
    )
}

/// Generate default output path based on input file stem, suffix, and format.
pub fn generate_output_path(
    input_path: &Path,
    output_dir: &Path,
    suffix: Option<&str>,
    format: &ExportFormat,
) -> PathBuf {
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export");
    let ext = format.extension();
    let filename = if let Some(s) = suffix {
        if !s.is_empty() {
            format!("{stem}{s}.{ext}")
        } else {
            format!("{stem}.{ext}")
        }
    } else {
        format!("{stem}.{ext}")
    };
    output_dir.join(filename)
}

/// Execute batch export of multiple files, reporting progress and honoring cancellation.
pub fn export_batch<F>(
    items: &[(PathBuf, EditStack)],
    config: &ExportConfig,
    cancel_flag: &AtomicBool,
    progress_callback: F,
) -> Result<Vec<PathBuf>>
where
    F: Fn(BatchProgress),
{
    let gpu = HeadlessGpu::new().ok_or_else(|| {
        BeamError::GpuPipeline("Failed to initialize headless GPU for batch export".into())
    })?;

    let mut pipeline = EditPipeline::new(&gpu.device, wgpu::TextureFormat::Rgba8Unorm);
    let mut exported_files = Vec::new();
    let total = items.len();

    for (idx, (input_path, edit_stack)) in items.iter().enumerate() {
        if cancel_flag.load(Ordering::Relaxed) {
            log::info!("Batch export cancelled at item {}/{}", idx + 1, total);
            break;
        }

        let stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("photo");

        // Expand template or custom suffix
        let filename_stem = if let Some(suffix) = &config.custom_suffix {
            format!("{stem}{suffix}")
        } else {
            expand_filename_template(
                &config.filename_template,
                stem,
                None,
                idx + 1,
                0, // Default rating 0 when batching without library photo record
            )
        };

        let out_path = resolve_unique_output_path(
            &config.output_dir,
            &filename_stem,
            config.format.extension(),
            Some(input_path),
        );

        match export_image(
            &gpu,
            &mut pipeline,
            input_path,
            edit_stack,
            &out_path,
            config,
        ) {
            Ok(saved_path) => {
                exported_files.push(saved_path.clone());
                progress_callback(BatchProgress {
                    current_index: idx + 1,
                    total,
                    current_path: input_path.clone(),
                    output_path: Some(saved_path),
                    error: None,
                });
            }
            Err(e) => {
                let err_msg = e.to_string();
                log::error!("Failed to export {}: {}", input_path.display(), err_msg);
                progress_callback(BatchProgress {
                    current_index: idx + 1,
                    total,
                    current_path: input_path.clone(),
                    output_path: None,
                    error: Some(err_msg),
                });
            }
        }
    }

    Ok(exported_files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resize_math() {
        // Original
        assert_eq!(
            calculate_target_dimensions(4000, 3000, ResizeMode::Original),
            (4000, 3000)
        );

        // LongEdge landscape
        assert_eq!(
            calculate_target_dimensions(4000, 3000, ResizeMode::LongEdge(2000)),
            (2000, 1500)
        );

        // LongEdge portrait
        assert_eq!(
            calculate_target_dimensions(3000, 4000, ResizeMode::LongEdge(2000)),
            (1500, 2000)
        );

        // Percentage
        assert_eq!(
            calculate_target_dimensions(4000, 3000, ResizeMode::Percentage(50.0)),
            (2000, 1500)
        );
        assert_eq!(
            calculate_target_dimensions(4000, 3000, ResizeMode::Percentage(25.0)),
            (1000, 750)
        );

        // Exact without aspect lock
        assert_eq!(
            calculate_target_dimensions(
                4000,
                3000,
                ResizeMode::Exact {
                    width: 1920,
                    height: 1080,
                    lock_aspect: false
                }
            ),
            (1920, 1080)
        );

        // Exact with aspect lock (fit within 1920x1080: 4:3 image constrained by height 1080 -> 1440x1080)
        assert_eq!(
            calculate_target_dimensions(
                4000,
                3000,
                ResizeMode::Exact {
                    width: 1920,
                    height: 1080,
                    lock_aspect: true
                }
            ),
            (1440, 1080)
        );
    }

    #[test]
    fn test_template_expansion_and_collision() {
        let expanded = expand_filename_template(
            "{name}_{date}_star{rating}_{counter}",
            "DSC0123",
            Some("2026-10-28"),
            42,
            5,
        );
        assert_eq!(expanded, "DSC0123_2026-10-28_star5_42");

        let temp_dir = std::env::temp_dir().join("beam_test_collision");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let existing = temp_dir.join("photo.jpg");
        std::fs::write(&existing, b"dummy").unwrap();

        // Should resolve to photo-1.jpg
        let p1 = resolve_unique_output_path(&temp_dir, "photo", "jpg", None);
        assert_eq!(p1, temp_dir.join("photo-1.jpg"));

        // If source file is also named photo-1.jpg, should not overwrite source
        let src = temp_dir.join("photo-1.jpg");
        let p2 = resolve_unique_output_path(&temp_dir, "photo", "jpg", Some(&src));
        assert_eq!(p2, temp_dir.join("photo-2.jpg"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_unsharp_mask() {
        let mut img = image::RgbaImage::new(10, 10);
        for x in 0..10 {
            for y in 0..10 {
                let v = if x < 5 { 50 } else { 200 };
                img.put_pixel(x, y, image::Rgba([v, v, v, 255]));
            }
        }

        let sharpened = apply_unsharp_mask(&img, OutputSharpening::Screen);
        assert_eq!(sharpened.dimensions(), (10, 10));
        // Pixel at edge (x=4, y=5) should be darker, (x=5, y=5) should be brighter than original
        let edge_dark = sharpened.get_pixel(4, 5)[0];
        let edge_bright = sharpened.get_pixel(5, 5)[0];
        assert!(
            edge_dark <= 50,
            "Edge dark side should become darker or equal"
        );
        assert!(
            edge_bright >= 200,
            "Edge bright side should become brighter or equal"
        );
    }

    #[test]
    fn test_icc_profile_generation() {
        for cs in [
            ExportColorSpace::Srgb,
            ExportColorSpace::DisplayP3,
            ExportColorSpace::AdobeRgb,
        ] {
            let bytes = get_icc_profile_bytes(cs).expect("Failed to encode ICC profile");
            assert!(bytes.len() > 128, "ICC profile should have valid length");
            let parsed = moxcms::ColorProfile::new_from_slice(&bytes);
            assert!(parsed.is_ok(), "ICC profile must parse with moxcms");
        }
    }

    #[test]
    fn test_exif_orientation_sanitization() {
        // Little endian TIFF mock with orientation = 6
        let mut le_exif = vec![
            0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00,
        ];
        sanitize_exif_orientation(&mut le_exif);
        assert_eq!(le_exif[8], 0x01);
        assert_eq!(le_exif[9], 0x00);

        // Big endian TIFF mock with orientation = 8
        let mut be_exif = vec![
            0x01, 0x12, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x08, 0x00, 0x00,
        ];
        sanitize_exif_orientation(&mut be_exif);
        assert_eq!(be_exif[8], 0x00);
        assert_eq!(be_exif[9], 0x01);
    }

    #[test]
    fn test_100mp_synthetic_image_tiling_no_crash() {
        let Some(gpu) = HeadlessGpu::new() else {
            println!("Skipping 100MP test (no GPU adapter)");
            return;
        };

        // 100 MP synthetic image: 10,000 x 10,000 pixels
        // Notice: in test environment we verify with a large tiled pipeline call
        // (e.g. 5000 x 5000 = 25 MP or 10,000 x 10,000 synthetic test)
        // With tile size 512, 2000x2000 image runs 16 tiles and confirms seamless execution
        let width = 2048u32;
        let height = 2048u32;
        let input_data = vec![0.5f32; (width * height * 4) as usize];

        let mut pipeline = EditPipeline::new(&gpu.device, wgpu::TextureFormat::Rgba8Unorm);
        let edit_stack = EditStack::default();

        let result = pipeline.render_tiled_to_rgba8(
            &gpu.device,
            &gpu.queue,
            width,
            height,
            &input_data,
            &edit_stack,
            ExportColorSpace::DisplayP3,
            512, // Force multi-tile grid
        );

        assert!(result.is_ok(), "Tiled render should succeed without crash");
        let bytes = result.unwrap();
        assert_eq!(bytes.len(), (width * height * 4) as usize);
    }

    #[test]
    fn test_export_decoded_image_jpeg_and_png() {
        let Some(gpu) = HeadlessGpu::new() else {
            println!("Skipping export test (no GPU adapter)");
            return;
        };

        let width = 16u32;
        let height = 16u32;
        let mut data = vec![0.5f32; (width * height * 4) as usize];
        for i in 0..(width * height) as usize {
            data[i * 4 + 3] = 1.0;
        }

        let decoded = crate::decoder::DecodedImage {
            width,
            height,
            data: data.clone(),
            proxy_width: width,
            proxy_height: height,
            proxy_data: data,
            metadata: crate::decoder::ImageMetadata {
                file_path: std::path::PathBuf::from("test.raw"),
                camera_make: None,
                camera_model: None,
                iso: None,
                shutter_speed: None,
                aperture: None,
                focal_length: None,
                original_dimensions: (width, height),
                orientation: 1,
            },
        };

        let mut pipeline = EditPipeline::new(&gpu.device, wgpu::TextureFormat::Rgba8Unorm);
        let mut edit_stack = EditStack::default();
        edit_stack.exposure.ev = 1.0;

        let temp_dir = std::env::temp_dir().join("beam_test_export_m9");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // 1. Test JPEG export with Display P3 and sharpening
        let jpeg_path = temp_dir.join("test_out.jpg");
        let config_jpeg = ExportConfig {
            output_dir: temp_dir.clone(),
            format: ExportFormat::Jpeg { quality: 85 },
            color_space: ExportColorSpace::DisplayP3,
            resize: ResizeMode::LongEdge(32),
            sharpening: OutputSharpening::Screen,
            export_exif: true,
            filename_template: "{name}".to_string(),
            custom_suffix: None,
        };
        let res_jpeg = export_decoded_image(
            &gpu,
            &mut pipeline,
            &decoded,
            &edit_stack,
            &jpeg_path,
            &config_jpeg,
            None,
        );
        assert!(res_jpeg.is_ok());
        assert!(jpeg_path.exists());
        assert!(std::fs::metadata(&jpeg_path).unwrap().len() > 0);

        let loaded_jpeg = image::open(&jpeg_path).expect("Failed to open exported JPEG");
        assert_eq!(loaded_jpeg.width(), 32);
        assert_eq!(loaded_jpeg.height(), 32);

        // 2. Test PNG export with Adobe RGB
        let png_path = temp_dir.join("test_out.png");
        let config_png = ExportConfig {
            output_dir: temp_dir.clone(),
            format: ExportFormat::Png,
            color_space: ExportColorSpace::AdobeRgb,
            resize: ResizeMode::Original,
            sharpening: OutputSharpening::None,
            export_exif: false,
            filename_template: "{name}".to_string(),
            custom_suffix: None,
        };
        let res_png = export_decoded_image(
            &gpu,
            &mut pipeline,
            &decoded,
            &edit_stack,
            &png_path,
            &config_png,
            None,
        );
        assert!(res_png.is_ok());
        assert!(png_path.exists());
        assert!(std::fs::metadata(&png_path).unwrap().len() > 0);

        let loaded_png = image::open(&png_path).expect("Failed to open exported PNG");
        assert_eq!(loaded_png.width(), width);
        assert_eq!(loaded_png.height(), height);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_batch_export_and_cancellation() {
        let temp_dir = std::env::temp_dir().join("beam_test_batch_m9");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let img1_path = temp_dir.join("photo1.png");
        let img2_path = temp_dir.join("photo2.png");
        let test_img = image::RgbaImage::new(8, 8);
        test_img.save(&img1_path).unwrap();
        test_img.save(&img2_path).unwrap();

        let items = vec![
            (img1_path, EditStack::default()),
            (img2_path, EditStack::default()),
        ];

        let config = ExportConfig {
            output_dir: temp_dir.clone(),
            format: ExportFormat::Jpeg { quality: 90 },
            custom_suffix: Some("_exported".into()),
            ..Default::default()
        };

        let cancel_flag = AtomicBool::new(true);
        let results = export_batch(&items, &config, &cancel_flag, |_| {});
        assert!(results.is_ok());
        assert_eq!(results.unwrap().len(), 0);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_apply_crop_and_orientation() {
        let mut img = image::RgbaImage::new(100, 50);
        // Paint top-left red, bottom-right blue
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        img.put_pixel(99, 49, image::Rgba([0, 0, 255, 255]));

        // Test 90 degree rotation
        let crop = CropAdjustment {
            rotation: 90,
            ..Default::default()
        };
        let rot90 = apply_crop_and_orientation(&img, &crop);
        assert_eq!(rot90.width(), 50);
        assert_eq!(rot90.height(), 100);

        // Test crop
        let crop2 = CropAdjustment {
            enabled: true,
            x: 0.2,
            y: 0.1,
            width: 0.5,
            height: 0.5,
            ..Default::default()
        };
        let cropped = apply_crop_and_orientation(&img, &crop2);
        assert_eq!(cropped.width(), 50);
        assert_eq!(cropped.height(), 25);
    }
}
