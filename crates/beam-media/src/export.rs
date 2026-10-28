//! Full-resolution image export and batch processing.

use beam_core::error::{BeamError, Result};
use beam_core::sidecar::EditStack;
use beam_gpu::pipeline::{EditPipeline, GpuImage, HeadlessGpu};
use image::codecs::jpeg::JpegEncoder;
use image::ImageEncoder;
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

/// Output destination and naming configuration.
#[derive(Clone, Debug)]
pub struct ExportConfig {
    pub output_dir: PathBuf,
    pub format: ExportFormat,
    pub custom_suffix: Option<String>,
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
    format: &ExportFormat,
) -> Result<PathBuf> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let gpu_image = GpuImage::from_rgba32f(
        &gpu.device,
        &gpu.queue,
        decoded.width,
        decoded.height,
        &decoded.data,
    )?;

    let rgba_bytes = pipeline.render_to_rgba8(&gpu.device, &gpu.queue, &gpu_image, edit_stack)?;

    let rgba_image = image::RgbaImage::from_raw(decoded.width, decoded.height, rgba_bytes)
        .ok_or_else(|| {
            BeamError::ImageProcessing("Failed to construct RgbaImage from raw pixel buffer".into())
        })?;

    match format {
        ExportFormat::Jpeg { quality } => {
            let quality = (*quality).clamp(1, 100);
            let rgb_image = image::DynamicImage::ImageRgba8(rgba_image).to_rgb8();
            let file = std::fs::File::create(output_path)?;
            let mut writer = std::io::BufWriter::new(file);
            let encoder = JpegEncoder::new_with_quality(&mut writer, quality);
            encoder
                .write_image(
                    rgb_image.as_raw(),
                    decoded.width,
                    decoded.height,
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|e| BeamError::ImageProcessing(format!("JPEG encode error: {e}")))?;
        }
        ExportFormat::Png => {
            rgba_image
                .save_with_format(output_path, image::ImageFormat::Png)
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
    format: &ExportFormat,
) -> Result<PathBuf> {
    let decoded = crate::decoder::decode_image(input_path)?;
    export_decoded_image(gpu, pipeline, &decoded, edit_stack, output_path, format)
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

        let out_path = generate_output_path(
            input_path,
            &config.output_dir,
            config.custom_suffix.as_deref(),
            &config.format,
        );

        match export_image(
            &gpu,
            &mut pipeline,
            input_path,
            edit_stack,
            &out_path,
            &config.format,
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

        let temp_dir = std::env::temp_dir().join("beam_test_export");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // 1. Test JPEG export
        let jpeg_path = temp_dir.join("test_out.jpg");
        let format_jpeg = ExportFormat::Jpeg { quality: 85 };
        let res_jpeg = export_decoded_image(
            &gpu,
            &mut pipeline,
            &decoded,
            &edit_stack,
            &jpeg_path,
            &format_jpeg,
        );
        assert!(res_jpeg.is_ok());
        assert!(jpeg_path.exists());
        assert!(std::fs::metadata(&jpeg_path).unwrap().len() > 0);

        // Verify it is a valid JPEG
        let loaded_jpeg = image::open(&jpeg_path).expect("Failed to open exported JPEG");
        assert_eq!(loaded_jpeg.width(), width);
        assert_eq!(loaded_jpeg.height(), height);

        // 2. Test PNG export
        let png_path = temp_dir.join("test_out.png");
        let format_png = ExportFormat::Png;
        let res_png = export_decoded_image(
            &gpu,
            &mut pipeline,
            &decoded,
            &edit_stack,
            &png_path,
            &format_png,
        );
        assert!(res_png.is_ok());
        assert!(png_path.exists());
        assert!(std::fs::metadata(&png_path).unwrap().len() > 0);

        // Verify it is a valid PNG
        let loaded_png = image::open(&png_path).expect("Failed to open exported PNG");
        assert_eq!(loaded_png.width(), width);
        assert_eq!(loaded_png.height(), height);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_batch_export_and_cancellation() {
        let temp_dir = std::env::temp_dir().join("beam_test_batch");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create two synthetic small test PNG images
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
        };

        // Test cancellation flag
        let cancel_flag = AtomicBool::new(true); // pre-cancelled
        let results = export_batch(&items, &config, &cancel_flag, |_| {});
        assert!(results.is_ok());
        assert_eq!(results.unwrap().len(), 0);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
