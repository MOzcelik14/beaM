//! RAW image decoding using `rawler`.

use beam_core::error::{BeamError, Result};
use rawler::imgop::develop::{Intermediate, ProcessingStep, RawDevelop};
use std::path::{Path, PathBuf};

/// Decoded linear image representation ready for GPU texture upload.
#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// RGBA32F linear pixel data (4 * width * height floats in [0.0, 1.0]).
    pub data: Vec<f32>,
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
                // Approximate 4-color CFA (e.g. RGBE or dual green) to RGB
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

    Ok(DecodedImage {
        width,
        height,
        data: rgba_floats,
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

    Ok(DecodedImage {
        width,
        height,
        data,
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

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
        assert_eq!(decoded.data.len(), 10 * 10 * 4);

        let _ = std::fs::remove_file(test_path);
    }

    #[test]
    fn test_nonexistent_file_handling() {
        let result = decode_image(Path::new("/nonexistent/file/test.ARW"));
        assert!(result.is_err());
    }
}
