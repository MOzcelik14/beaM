//! RAW image decoding using `rawler` and proxy generation.

use beam_core::error::{BeamError, Result};
use beam_core::sidecar::{EditStack, Sidecar};
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

#[derive(Debug, Clone)]
pub struct ImageMetadata {
    pub file_path: PathBuf,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub iso: Option<u32>,
    pub shutter_speed: Option<String>,
    pub aperture: Option<f32>,
    pub focal_length: Option<f32>,
    pub original_dimensions: (u32, u32),
    pub orientation: u16,
}

impl Default for ImageMetadata {
    fn default() -> Self {
        Self {
            file_path: PathBuf::new(),
            camera_make: None,
            camera_model: None,
            iso: None,
            shutter_speed: None,
            aperture: None,
            focal_length: None,
            original_dimensions: (0, 0),
            orientation: 1,
        }
    }
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
            orientation: rawimage.orientation.to_u16(),
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
            orientation: 1,
        },
    })
}

/// Apply EXIF orientation transform (1..=8) to an RgbaImage.
pub fn apply_orientation(img: image::RgbaImage, orientation: u16) -> image::RgbaImage {
    use image::imageops;
    match orientation {
        1 => img,
        2 => imageops::flip_horizontal(&img),
        3 => imageops::rotate180(&img),
        4 => imageops::flip_vertical(&img),
        5 => {
            let flipped = imageops::flip_horizontal(&img);
            imageops::rotate270(&flipped)
        }
        6 => imageops::rotate90(&img),
        7 => {
            let flipped = imageops::flip_horizontal(&img);
            imageops::rotate90(&flipped)
        }
        8 => imageops::rotate270(&img),
        _ => img,
    }
}

/// Apply edit adjustments in CPU space for instant, lightweight thumbnail generation.
pub fn apply_cpu_edits(_width: u32, _height: u32, input: &[f32], stack: &EditStack) -> Vec<f32> {
    let wb_gains = if stack.white_balance.enabled {
        beam_core::color::white_balance_gains(
            stack.white_balance.temperature,
            stack.white_balance.tint,
        )
    } else {
        [1.0, 1.0, 1.0]
    };

    let ev_multiplier = if stack.exposure.enabled {
        2.0f32.powf(stack.exposure.ev)
    } else {
        1.0
    };

    let contrast_slope = if stack.contrast.enabled {
        beam_core::color::contrast_slope(stack.contrast.contrast)
    } else {
        1.0
    };

    let curve_lut = if stack.tone_curve.enabled {
        Some(beam_core::color::generate_tone_curve_lut(
            &stack.tone_curve.points,
        ))
    } else {
        None
    };

    let highlights_shadows = if stack.highlights_shadows.enabled {
        Some((
            stack.highlights_shadows.highlights / 100.0,
            stack.highlights_shadows.shadows / 100.0,
        ))
    } else {
        None
    };

    let mut output = Vec::with_capacity(input.len());
    for chunk in input.as_chunks::<4>().0 {
        let mut r = chunk[0];
        let mut g = chunk[1];
        let mut b = chunk[2];
        let a = chunk[3];

        // 1. White balance
        r *= wb_gains[0];
        g *= wb_gains[1];
        b *= wb_gains[2];

        // 2. Exposure
        r *= ev_multiplier;
        g *= ev_multiplier;
        b *= ev_multiplier;

        // 3. Highlights & Shadows
        if let Some((high, shad)) = highlights_shadows {
            let lum = (0.2627 * r + 0.6780 * g + 0.0593 * b).max(0.0);
            if lum > 0.0001 {
                let shadow_factor = (1.0 - (lum / 0.18).min(1.0)).powi(2) * shad;
                let highlight_factor = ((lum - 0.18).max(0.0) / 0.82).min(1.0).powi(2) * high;
                let factor = (1.0 + shadow_factor + highlight_factor).max(0.0);
                r *= factor;
                g *= factor;
                b *= factor;
            }
        }

        // 4. Contrast
        if stack.contrast.enabled {
            r = ((r - 0.18) * contrast_slope + 0.18).max(0.0);
            g = ((g - 0.18) * contrast_slope + 0.18).max(0.0);
            b = ((b - 0.18) * contrast_slope + 0.18).max(0.0);
        }

        // 5. Tone curve
        if let Some(lut) = &curve_lut {
            let eval_curve = |val: f32| -> f32 {
                let clamped = val.clamp(0.0, 1.0);
                let idx_f = clamped * 255.0;
                let idx = (idx_f.floor() as usize).min(254);
                let frac = idx_f - idx as f32;
                lut[idx] * (1.0 - frac) + lut[idx + 1] * frac
            };
            r = eval_curve(r);
            g = eval_curve(g);
            b = eval_curve(b);
        }

        output.push(r.max(0.0));
        output.push(g.max(0.0));
        output.push(b.max(0.0));
        output.push(a);
    }

    output
}

/// Generate an 8-bit sRGB thumbnail from an image file (RAW or raster) with max dimension,
/// optionally applying non-destructive edits and respecting EXIF orientation.
pub fn generate_thumbnail_with_edits(
    path: &Path,
    edit_stack: Option<&EditStack>,
    max_dim: u32,
) -> Result<image::RgbaImage> {
    let decoded = decode_image(path)?;
    let (thumb_w, thumb_h, thumb_floats) = generate_proxy(
        decoded.proxy_width,
        decoded.proxy_height,
        &decoded.proxy_data,
        max_dim,
    );

    let processed_floats = if let Some(stack) = edit_stack {
        apply_cpu_edits(thumb_w, thumb_h, &thumb_floats, stack)
    } else {
        thumb_floats
    };

    let mut rgba_bytes = Vec::with_capacity((thumb_w * thumb_h * 4) as usize);
    for chunk in processed_floats.as_chunks::<4>().0 {
        let [sr, sg, sb] =
            beam_core::color::rec2020_linear_to_srgb_display([chunk[0], chunk[1], chunk[2]]);
        rgba_bytes.push((sr.clamp(0.0, 1.0) * 255.0).round() as u8);
        rgba_bytes.push((sg.clamp(0.0, 1.0) * 255.0).round() as u8);
        rgba_bytes.push((sb.clamp(0.0, 1.0) * 255.0).round() as u8);
        rgba_bytes.push((chunk[3].clamp(0.0, 1.0) * 255.0).round() as u8);
    }

    let img = image::RgbaImage::from_raw(thumb_w, thumb_h, rgba_bytes).ok_or_else(|| {
        BeamError::ImageProcessing("Failed to construct thumbnail image buffer".to_string())
    })?;

    Ok(apply_orientation(img, decoded.metadata.orientation))
}

/// Generate an 8-bit sRGB thumbnail without edits (or using default settings).
pub fn generate_thumbnail(path: &Path, max_dim: u32) -> Result<image::RgbaImage> {
    generate_thumbnail_with_edits(path, None, max_dim)
}

/// Compute persistent thumbnail path in cache directory using hash of canonical path,
/// modification time, file size, and sidecar modification time/size for robust cache invalidation.
pub fn thumbnail_cache_path(image_path: &Path, cache_dir: &Path) -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    let canonical = image_path
        .canonicalize()
        .unwrap_or_else(|_| image_path.to_path_buf());
    canonical.to_string_lossy().hash(&mut hasher);

    // Hash source file size and modification time
    if let Ok(meta) = std::fs::metadata(image_path) {
        meta.len().hash(&mut hasher);
        if let Ok(mtime) = meta.modified() {
            if let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH) {
                dur.as_millis().hash(&mut hasher);
            }
        }
    }

    // Hash sidecar file size and modification time if present
    let sidecar_p = Sidecar::path_for(image_path);
    if let Ok(sc_meta) = std::fs::metadata(&sidecar_p) {
        sc_meta.len().hash(&mut hasher);
        if let Ok(sc_mtime) = sc_meta.modified() {
            if let Ok(dur) = sc_mtime.duration_since(std::time::UNIX_EPOCH) {
                dur.as_millis().hash(&mut hasher);
            }
        }
    }

    let hash = hasher.finish();
    let filename = image_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("thumb");
    cache_dir.join(format!("{hash:016x}_{filename}.jpg"))
}

/// Save an RGBA image as a thumbnail on disk (supports JPEG and PNG).
pub fn save_thumbnail(image: &image::RgbaImage, target_path: &Path) -> Result<()> {
    use image::ImageEncoder;
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let ext = target_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "jpg" || ext == "jpeg" {
        let rgb = image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();
        let file = std::fs::File::create(target_path)?;
        let mut writer = std::io::BufWriter::new(file);
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 85);
        encoder
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| BeamError::ImageProcessing(e.to_string()))?;
    } else {
        image
            .save(target_path)
            .map_err(|e| BeamError::ImageProcessing(e.to_string()))?;
    }
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

    #[test]
    fn test_save_thumbnail_jpg_and_png() {
        let temp_dir = std::env::temp_dir();
        let thumb_jpg = temp_dir.join("test_save_thumb.jpg");
        let thumb_png = temp_dir.join("test_save_thumb.png");
        let img = image::RgbaImage::new(32, 32);

        assert!(save_thumbnail(&img, &thumb_jpg).is_ok());
        assert!(thumb_jpg.exists());
        assert!(image::open(&thumb_jpg).is_ok());

        assert!(save_thumbnail(&img, &thumb_png).is_ok());
        assert!(thumb_png.exists());
        assert!(image::open(&thumb_png).is_ok());

        let _ = std::fs::remove_file(thumb_jpg);
        let _ = std::fs::remove_file(thumb_png);
    }

    #[test]
    fn test_thumbnail_cache_key_invalidation() {
        let temp_dir = std::env::temp_dir().join("beam_test_cache_key");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let cache_dir = temp_dir.join("cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let img_path = temp_dir.join("photo.png");
        let img = image::RgbaImage::new(16, 16);
        img.save(&img_path).unwrap();

        let path1 = thumbnail_cache_path(&img_path, &cache_dir);
        let path2 = thumbnail_cache_path(&img_path, &cache_dir);
        assert_eq!(
            path1, path2,
            "Cache path must be deterministic for unchanged file"
        );

        // Touch the file with different content
        std::thread::sleep(std::time::Duration::from_millis(15));
        let larger_img = image::RgbaImage::new(32, 32);
        larger_img.save(&img_path).unwrap();

        let path3 = thumbnail_cache_path(&img_path, &cache_dir);
        assert_ne!(
            path1, path3,
            "Cache path must change when source file changes"
        );

        // Create a sidecar file
        let sidecar_p = Sidecar::path_for(&img_path);
        let mut sidecar = Sidecar::new("photo.png");
        sidecar.save_to_file(&sidecar_p).unwrap();

        let path4 = thumbnail_cache_path(&img_path, &cache_dir);
        assert_ne!(
            path3, path4,
            "Cache path must change when sidecar is created"
        );

        // Modify sidecar file
        std::thread::sleep(std::time::Duration::from_millis(15));
        sidecar.edit_stack.exposure.ev = 1.5;
        sidecar.save_to_file(&sidecar_p).unwrap();

        let path5 = thumbnail_cache_path(&img_path, &cache_dir);
        assert_ne!(
            path4, path5,
            "Cache path must change when sidecar is edited"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_thumbnail_exif_orientation() {
        // Create an asymmetric 4x2 image
        let mut img = image::RgbaImage::new(4, 2);
        // Put a marker pixel at top-left (0, 0)
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));

        // 1 = Normal
        let r1 = apply_orientation(img.clone(), 1);
        assert_eq!(r1.dimensions(), (4, 2));
        assert_eq!(r1.get_pixel(0, 0), &image::Rgba([255, 0, 0, 255]));

        // 3 = Rotate 180 (top-left moves to bottom-right: x = 3, y = 1)
        let r3 = apply_orientation(img.clone(), 3);
        assert_eq!(r3.dimensions(), (4, 2));
        assert_eq!(r3.get_pixel(3, 1), &image::Rgba([255, 0, 0, 255]));

        // 6 = Rotate 90 CW (dimensions become 2x4, top-left moves to top-right: x = 1, y = 0)
        let r6 = apply_orientation(img.clone(), 6);
        assert_eq!(r6.dimensions(), (2, 4));
        assert_eq!(r6.get_pixel(1, 0), &image::Rgba([255, 0, 0, 255]));

        // 8 = Rotate 270 CW (dimensions become 2x4, top-left moves to bottom-left: x = 0, y = 3)
        let r8 = apply_orientation(img.clone(), 8);
        assert_eq!(r8.dimensions(), (2, 4));
        assert_eq!(r8.get_pixel(0, 3), &image::Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn test_generate_thumbnail_with_edits() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("beam_test_thumb_edits.png");
        let img = image::ImageBuffer::from_pixel(64, 64, image::Rgba([50u8, 50u8, 50u8, 255u8]));
        img.save(&test_path).unwrap();

        // Default thumbnail
        let thumb_default = generate_thumbnail_with_edits(&test_path, None, 32).unwrap();
        let p_default = thumb_default.get_pixel(16, 16)[0];

        // Edit with +2 EV exposure
        let mut stack = EditStack::default();
        stack.exposure.ev = 2.0;
        let thumb_bright = generate_thumbnail_with_edits(&test_path, Some(&stack), 32).unwrap();
        let p_bright = thumb_bright.get_pixel(16, 16)[0];

        assert!(
            p_bright > p_default,
            "Thumbnail with +2 EV exposure must be brighter than default: {p_bright} > {p_default}"
        );

        let _ = std::fs::remove_file(test_path);
    }
}
