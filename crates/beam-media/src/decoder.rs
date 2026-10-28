//! RAW image decoding using `rawler` and proxy generation.

use beam_core::error::{BeamError, Result};
use rawler::imgop::develop::{Intermediate, ProcessingStep, RawDevelop};
use std::path::{Path, PathBuf};

/// Default maximum dimension for the interactive editing proxy
pub const PROXY_MAX_DIMENSION: u32 = 1920;

/// Decoded linear image representation containing full-resolution and fast proxy buffers.
#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// RGBA32F linear pixel data at full resolution (4 * width * height floats in [0.0, 1.0]).
    pub data: Vec<f32>,
    /// Fast proxy width (<= PROXY_MAX_DIMENSION)
    pub proxy_width: u32,
    /// Fast proxy height (<= PROXY_MAX_DIMENSION)
    pub proxy_height: u32,
    /// RGBA32F linear proxy pixel data for real-time 60 FPS slider preview.
    pub proxy_data: Vec<f32>,
    pub metadata: ImageMetadata,
}

#[derive(Debug, Clone, Default)]
pub struct ImageMetadata {
    pub file_path: PathBuf,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub iso: Option<u32>,
    pub shutter_speed: Option<String>,
    pub aperture: Option<f32>,
    pub focal_length: Option<f32>,
    pub original_dimensions: (u32, u32),
}

/// Downscale an RGBA32F linear image to fit within max_dim using bilinear sampling.
pub fn generate_proxy(
    src_w: u32,
    src_h: u32,
    src_data: &[f32],
    max_dim: u32,
) -> (u32, u32, Vec<f32>) {
    let max_side = src_w.max(src_h);
    if max_side <= max_dim {
        return (src_w, src_h, src_data.to_vec());
    }

    let scale = max_dim as f32 / max_side as f32;
    let dst_w = ((src_w as f32 * scale).round() as u32).max(1);
    let dst_h = ((src_h as f32 * scale).round() as u32).max(1);

    let mut dst_data = Vec::with_capacity((dst_w * dst_h * 4) as usize);

    let x_ratio = (src_w - 1) as f32 / (dst_w - 1).max(1) as f32;
    let y_ratio = (src_h - 1) as f32 / (dst_h - 1).max(1) as f32;

    for dy in 0..dst_h {
        let sy_f = dy as f32 * y_ratio;
        let y0 = sy_f.floor() as u32;
        let y1 = (y0 + 1).min(src_h - 1);
        let y_weight = sy_f - y0 as f32;

        let row0_idx = (y0 * src_w * 4) as usize;
        let row1_idx = (y1 * src_w * 4) as usize;

        for dx in 0..dst_w {
            let sx_f = dx as f32 * x_ratio;
            let x0 = sx_f.floor() as u32;
            let x1 = (x0 + 1).min(src_w - 1);
            let x_weight = sx_f - x0 as f32;

            let c00 = row0_idx + (x0 * 4) as usize;
            let c01 = row0_idx + (x1 * 4) as usize;
            let c10 = row1_idx + (x0 * 4) as usize;
            let c11 = row1_idx + (x1 * 4) as usize;

            for ch in 0..4 {
                let top = src_data[c00 + ch] * (1.0 - x_weight) + src_data[c01 + ch] * x_weight;
                let bottom = src_data[c10 + ch] * (1.0 - x_weight) + src_data[c11 + ch] * x_weight;
                let interpolated = top * (1.0 - y_weight) + bottom * y_weight;
                dst_data.push(interpolated);
            }
        }
    }

    (dst_w, dst_h, dst_data)
}

/// Decode an image file (camera RAW formats via rawler, or raster image fallback via image crate).
pub fn decode_image(path: &Path) -> Result<DecodedImage> {
    if !path.exists() {
        return Err(BeamError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("File not found: {}", path.display()),
        )));
    }

    // Try decoding with rawler first
    match decode_raw_with_rawler(path) {
        Ok(image) => Ok(image),
        Err(raw_err) => {
            log::warn!(
                "Rawler decode unsuccessful for {}: {raw_err}. Attempting raster fallback...",
                path.display()
            );
            // Fallback to image crate (e.g. for JPEG, PNG, TIFF fallback)
            decode_raster_fallback(path).map_err(|img_err| {
                BeamError::RawDecode(format!(
                    "Failed to decode file '{}'. RAW error: {raw_err}. Raster fallback error: {img_err}",
                    path.display()
                ))
            })
        }
    }
}

/// Decode camera RAW using rawler
fn decode_raw_with_rawler(path: &Path) -> Result<DecodedImage> {
    let rawimage = rawler::decode_file(path)
        .map_err(|e| BeamError::RawDecode(format!("RAW decode failed: {e:?}")))?;

    let orig_w = rawimage.width as u32;
    let orig_h = rawimage.height as u32;

    let camera_make = rawimage.camera.make.clone();
    let camera_model = rawimage.camera.model.clone();

    // Use RawDevelop to rescale (black/white levels) and demosaic into linear RGB
    let develop = RawDevelop {
        steps: vec![
            ProcessingStep::Rescale,
            ProcessingStep::Demosaic,
            ProcessingStep::FujiRotate,
            ProcessingStep::CropActiveArea,
        ],
    };

    let intermediate = develop
        .develop_intermediate(&rawimage)
        .map_err(|e| BeamError::RawDecode(format!("RAW development/demosaicing failed: {e:?}")))?;

    let (width, height, mut rgba_floats) = match intermediate {
        Intermediate::ThreeColor(pixels) => {
            let dim = pixels.dim();
            let count = dim.w * dim.h;
            let mut rgba = Vec::with_capacity(count * 4);
            let flat = pixels.flatten(); // &[f32] length 3 * count
            for chunk in flat.as_chunks::<3>().0 {
                rgba.push(chunk[0]);
                rgba.push(chunk[1]);
                rgba.push(chunk[2]);
                rgba.push(1.0);
            }
            (dim.w as u32, dim.h as u32, rgba)
        }
        Intermediate::FourColor(pixels) => {
            let dim = pixels.dim();
            let count = dim.w * dim.h;
            let mut rgba = Vec::with_capacity(count * 4);
            let flat = pixels.flatten();
            for chunk in flat.as_chunks::<4>().0 {
                rgba.push(chunk[0]);
                rgba.push((chunk[1] + chunk[3]) * 0.5);
                rgba.push(chunk[2]);
                rgba.push(1.0);
            }
            (dim.w as u32, dim.h as u32, rgba)
        }
        Intermediate::Monochrome(pixels) => {
            let dim = pixels.dim();
            let count = dim.w * dim.h;
            let mut rgba = Vec::with_capacity(count * 4);
            for &gray in &pixels.data {
                rgba.push(gray);
                rgba.push(gray);
                rgba.push(gray);
                rgba.push(1.0);
            }
            (dim.w as u32, dim.h as u32, rgba)
        }
    };

    // Ensure values are clamped to non-negative
    for val in &mut rgba_floats {
        if *val < 0.0 {
            *val = 0.0;
        }
    }

    let (proxy_w, proxy_h, proxy_data) =
        generate_proxy(width, height, &rgba_floats, PROXY_MAX_DIMENSION);

    Ok(DecodedImage {
        width,
        height,
        data: rgba_floats,
        proxy_width: proxy_w,
        proxy_height: proxy_h,
        proxy_data,
        metadata: ImageMetadata {
            file_path: path.to_path_buf(),
            camera_make: if camera_make.is_empty() {
                None
            } else {
                Some(camera_make)
            },
            camera_model: if camera_model.is_empty() {
                None
            } else {
                Some(camera_model)
            },
            iso: None,
            shutter_speed: None,
            aperture: None,
            focal_length: None,
            original_dimensions: (orig_w, orig_h),
        },
    })
}

/// Fallback decoder for regular image formats (JPEG, PNG, etc.)
fn decode_raster_fallback(path: &Path) -> Result<DecodedImage> {
    let img = image::open(path).map_err(|e| BeamError::UnsupportedFormat(e.to_string()))?;
    let rgba8 = img.to_rgba8();
    let (width, height) = (rgba8.width(), rgba8.height());

    // Convert sRGB u8 to linear f32
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for pixel in rgba8.pixels() {
        data.push(beam_core::color::srgb_to_linear(pixel[0] as f32 / 255.0));
        data.push(beam_core::color::srgb_to_linear(pixel[1] as f32 / 255.0));
        data.push(beam_core::color::srgb_to_linear(pixel[2] as f32 / 255.0));
        data.push(pixel[3] as f32 / 255.0);
    }

    let (proxy_w, proxy_h, proxy_data) = generate_proxy(width, height, &data, PROXY_MAX_DIMENSION);

    Ok(DecodedImage {
        width,
        height,
        data,
        proxy_width: proxy_w,
        proxy_height: proxy_h,
        proxy_data,
        metadata: ImageMetadata {
            file_path: path.to_path_buf(),
            camera_make: None,
            camera_model: None,
            iso: None,
            shutter_speed: None,
            aperture: None,
            focal_length: None,
            original_dimensions: (width, height),
        },
    })
}

/// Generate an 8-bit sRGB thumbnail from an image file (RAW or raster) with max dimension.
pub fn generate_thumbnail(path: &Path, max_dim: u32) -> Result<image::RgbaImage> {
    let decoded = decode_image(path)?;
    let (thumb_w, thumb_h, thumb_floats) = generate_proxy(
        decoded.proxy_width,
        decoded.proxy_height,
        &decoded.proxy_data,
        max_dim,
    );

    let mut rgba_bytes = Vec::with_capacity((thumb_w * thumb_h * 4) as usize);
    for chunk in thumb_floats.as_chunks::<4>().0 {
        let [sr, sg, sb] =
            beam_core::color::rec2020_linear_to_srgb_display([chunk[0], chunk[1], chunk[2]]);
        rgba_bytes.push((sr.clamp(0.0, 1.0) * 255.0).round() as u8);
        rgba_bytes.push((sg.clamp(0.0, 1.0) * 255.0).round() as u8);
        rgba_bytes.push((sb.clamp(0.0, 1.0) * 255.0).round() as u8);
        rgba_bytes.push((chunk[3].clamp(0.0, 1.0) * 255.0).round() as u8);
    }

    image::RgbaImage::from_raw(thumb_w, thumb_h, rgba_bytes).ok_or_else(|| {
        BeamError::ImageProcessing("Failed to construct thumbnail image buffer".to_string())
    })
}

/// Compute persistent thumbnail path in cache directory using hash of canonical path.
pub fn thumbnail_cache_path(image_path: &Path, cache_dir: &Path) -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    image_path.to_string_lossy().hash(&mut hasher);
    let hash = hasher.finish();

    let filename = image_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("thumb");
    cache_dir.join(format!("{hash:016x}_{filename}.jpg"))
}

/// Save an RGBA image as a thumbnail on disk.
pub fn save_thumbnail(image: &image::RgbaImage, target_path: &Path) -> Result<()> {
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    image
        .save(target_path)
        .map_err(|e| BeamError::ImageProcessing(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn test_proxy_generation() {
        let w = 4000;
        let h = 3000;
        let data = vec![0.5f32; (w * h * 4) as usize];
        let (pw, ph, pdata) = generate_proxy(w, h, &data, 1920);

        assert_eq!(pw, 1920);
        assert_eq!(ph, 1440);
        assert_eq!(pdata.len(), (1920 * 1440 * 4) as usize);
        // Midpoint value should remain ~0.5
        assert!((pdata[0] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn test_raster_fallback_decode() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("beam_test_image.png");

        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(10, 10, Rgba([255, 128, 0, 255]));
        img.save(&test_path).expect("failed to create test image");

        let decoded = decode_image(&test_path).expect("failed to decode raster image");
        assert_eq!(decoded.width, 10);
        assert_eq!(decoded.height, 10);
        assert_eq!(decoded.proxy_width, 10);
        assert_eq!(decoded.data.len(), 10 * 10 * 4);

        let _ = std::fs::remove_file(test_path);
    }

    #[test]
    fn test_nonexistent_file_handling() {
        let result = decode_image(Path::new("/nonexistent/file/test.ARW"));
        assert!(result.is_err());
    }

    #[test]
    fn test_thumbnail_generation() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("beam_test_thumb_source.png");
        let thumb_path = temp_dir.join("beam_test_thumb_output.png");

        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(64, 48, Rgba([100, 150, 200, 255]));
        img.save(&test_path).expect("failed to save test image");

        let thumb = generate_thumbnail(&test_path, 32).expect("failed to generate thumbnail");
        assert_eq!(thumb.width(), 32);
        assert_eq!(thumb.height(), 24);

        save_thumbnail(&thumb, &thumb_path).expect("failed to save thumbnail");
        assert!(thumb_path.exists());

        let _ = std::fs::remove_file(test_path);
        let _ = std::fs::remove_file(thumb_path);
    }
}
