//! RAW image decoding using `rawler` and proxy generation.

use beam_core::error::{BeamError, Result};
use beam_core::sidecar::{EditStack, Sidecar};
use exif::{In, Reader, Tag, Value};
use image::ImageDecoder;
use moxcms::{ColorProfile, Layout, TransformOptions};
use rawler::imgop::develop::{Intermediate, ProcessingStep, RawDevelop};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

/// Default maximum dimension for the interactive editing proxy
pub const PROXY_MAX_DIMENSION: u32 = 1920;

/// Maximum allowed image dimension (width or height) in pixels (32,768 px).
pub const MAX_IMAGE_DIMENSION: u32 = 32_768;

/// Maximum allowed total pixels in a decoded image (150 megapixels).
pub const MAX_IMAGE_PIXELS: u64 = 150_000_000;

/// Maximum allowed image file size for decoding (500 MB).
pub const MAX_IMAGE_FILE_SIZE: u64 = 500 * 1024 * 1024;

/// Maximum allowed memory allocation for image decoding (1 GB).
pub const MAX_IMAGE_DECODE_ALLOC: u64 = 1024 * 1024 * 1024;

/// Maximum allowed ICC profile size (4 MB).
pub const MAX_ICC_PROFILE_SIZE: usize = 4 * 1024 * 1024;

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
    if src_w == 0 || src_h == 0 || src_data.is_empty() {
        return (0, 0, Vec::new());
    }

    let max_side = src_w.max(src_h);
    if max_side <= max_dim {
        return (src_w, src_h, src_data.to_vec());
    }

    let scale = max_dim as f32 / max_side as f32;
    let dst_w = ((src_w as f32 * scale).round() as u32).clamp(1, max_dim);
    let dst_h = ((src_h as f32 * scale).round() as u32).clamp(1, max_dim);

    let mut dst_data = Vec::with_capacity((dst_w * dst_h * 4) as usize);

    let x_ratio = if dst_w > 1 {
        src_w.saturating_sub(1) as f32 / (dst_w - 1) as f32
    } else {
        0.0
    };
    let y_ratio = if dst_h > 1 {
        src_h.saturating_sub(1) as f32 / (dst_h - 1) as f32
    } else {
        0.0
    };

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
/// Check if the file path has a regular raster image extension (JPEG, PNG, TIFF, WebP).
pub fn is_raster_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "jpg" | "jpeg" | "png" | "tif" | "tiff" | "webp"
    )
}

/// Decode an image file (camera RAW formats via rawler, or raster image formats via image crate).
pub fn decode_image(path: &Path) -> Result<DecodedImage> {
    if !path.exists() {
        return Err(BeamError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("File not found: {}", path.display()),
        )));
    }

    let file_meta = std::fs::metadata(path)?;
    if file_meta.len() > MAX_IMAGE_FILE_SIZE {
        return Err(BeamError::ImageProcessing(format!(
            "File size exceeds safety limit ({} MB > {} MB)",
            file_meta.len() / (1024 * 1024),
            MAX_IMAGE_FILE_SIZE / (1024 * 1024)
        )));
    }

    if is_raster_file(path) {
        return decode_raster_image(path);
    }

    // Try decoding with rawler first
    match decode_raw_with_rawler(path) {
        Ok(image) => Ok(image),
        Err(raw_err) => {
            log::warn!(
                "Rawler decode unsuccessful for {}: {raw_err}. Attempting raster fallback...",
                path.display()
            );
            decode_raster_image(path).map_err(|img_err| {
                BeamError::RawDecode(format!(
                    "Failed to decode file '{}'. RAW error: {raw_err}. Raster fallback error: {img_err}",
                    path.display()
                ))
            })
        }
    }
}

/// Validate TIFF header and IFD structures to prevent third-party parser unbounded memory allocation (OOM).
pub fn validate_tiff_raw_structure(head: &[u8], total_file_size: usize) -> Result<()> {
    if head.len() < 8 {
        if head.starts_with(b"II") || head.starts_with(b"MM") {
            return Err(BeamError::RawDecode(
                "Corrupted TIFF header: truncated (< 8 bytes)".into(),
            ));
        }
        return Ok(());
    }
    let is_le = match &head[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Ok(()),
    };
    let magic = if is_le {
        u16::from_le_bytes([head[2], head[3]])
    } else {
        u16::from_be_bytes([head[2], head[3]])
    };
    if !matches!(magic, 42 | 85 | 21330 | 20306) {
        return Ok(());
    }

    let mut current_ifd = if is_le {
        u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize
    } else {
        u32::from_be_bytes([head[4], head[5], head[6], head[7]]) as usize
    };

    if current_ifd == 0 {
        return Err(BeamError::RawDecode(
            "Corrupted TIFF header: missing root IFD (offset is 0)".into(),
        ));
    }

    let mut visited_ifds = std::collections::HashSet::new();
    let mut ifd_depth = 0;

    while current_ifd != 0 {
        ifd_depth += 1;
        if ifd_depth > 16 {
            return Err(BeamError::RawDecode(
                "Corrupted TIFF header: IFD chain exceeded max depth of 16".into(),
            ));
        }

        if current_ifd < 8 || current_ifd.saturating_add(2) > total_file_size {
            return Err(BeamError::RawDecode(format!(
                "Corrupted TIFF header: IFD offset {current_ifd} invalid or exceeds file length {total_file_size}"
            )));
        }

        if !visited_ifds.insert(current_ifd) {
            return Err(BeamError::RawDecode(
                "Corrupted TIFF header: cycle detected in IFD chain".into(),
            ));
        }

        if current_ifd.saturating_add(2) > head.len() {
            // IFD is beyond our buffered head, but its offset is within file bounds
            break;
        }

        let entry_count = if is_le {
            u16::from_le_bytes([head[current_ifd], head[current_ifd + 1]]) as usize
        } else {
            u16::from_be_bytes([head[current_ifd], head[current_ifd + 1]]) as usize
        };

        if entry_count == 0 || entry_count > 4096 {
            return Err(BeamError::RawDecode(format!(
                "Corrupted TIFF header: invalid IFD entry count ({entry_count})"
            )));
        }

        let entries_end = current_ifd
            .saturating_add(2)
            .saturating_add(entry_count.saturating_mul(12));
        if entries_end.saturating_add(4) > total_file_size {
            return Err(BeamError::RawDecode(format!(
                "Corrupted TIFF header: {entry_count} IFD entries require at least {entries_end} bytes, file has {total_file_size}"
            )));
        }

        let mut ifd_width: Option<u32> = None;
        let mut ifd_height: Option<u32> = None;

        for i in 0..entry_count {
            let entry_offset = current_ifd
                .saturating_add(2)
                .saturating_add(i.saturating_mul(12));
            if entry_offset.saturating_add(12) > head.len() {
                break;
            }

            let tag = if is_le {
                u16::from_le_bytes([head[entry_offset], head[entry_offset + 1]])
            } else {
                u16::from_be_bytes([head[entry_offset], head[entry_offset + 1]])
            };

            let type_code = if is_le {
                u16::from_le_bytes([head[entry_offset + 2], head[entry_offset + 3]])
            } else {
                u16::from_be_bytes([head[entry_offset + 2], head[entry_offset + 3]])
            };
            let type_size = match type_code {
                1 | 2 | 6 | 7 => 1,
                3 | 8 => 2,
                4 | 9 | 11 | 13 => 4,
                5 | 10 | 12 => 8,
                _ => 1,
            };

            let count = if is_le {
                u32::from_le_bytes([
                    head[entry_offset + 4],
                    head[entry_offset + 5],
                    head[entry_offset + 6],
                    head[entry_offset + 7],
                ]) as usize
            } else {
                u32::from_be_bytes([
                    head[entry_offset + 4],
                    head[entry_offset + 5],
                    head[entry_offset + 6],
                    head[entry_offset + 7],
                ]) as usize
            };

            if matches!(tag, 0x014A | 0x8769 | 0x8825 | 0x8773)
                && (count == 0 || type_code == 0 || type_code > 13)
            {
                return Err(BeamError::RawDecode(format!(
                    "Corrupted TIFF IFD entry: SubIFD tag 0x{tag:04X} has invalid count ({count}) or type ({type_code})"
                )));
            }

            let data_size = count.saturating_mul(type_size);
            if data_size > total_file_size || count > total_file_size {
                return Err(BeamError::RawDecode(format!(
                    "Corrupted TIFF IFD entry: data size {data_size} exceeds file length {total_file_size}"
                )));
            }

            // Pre-decode dimension & pixel-budget guard for TIFF/DNG/NEF/ARW/CR2 headers:
            // Reject oversized ImageWidth (0x0100) / ImageLength (0x0101) before rawler allocates.
            if matches!(tag, 0x0100 | 0x0101) && count == 1 && matches!(type_code, 3 | 4) {
                let dim_val = if type_code == 3 {
                    if is_le {
                        u16::from_le_bytes([head[entry_offset + 8], head[entry_offset + 9]]) as u32
                    } else {
                        u16::from_be_bytes([head[entry_offset + 8], head[entry_offset + 9]]) as u32
                    }
                } else if is_le {
                    u32::from_le_bytes([
                        head[entry_offset + 8],
                        head[entry_offset + 9],
                        head[entry_offset + 10],
                        head[entry_offset + 11],
                    ])
                } else {
                    u32::from_be_bytes([
                        head[entry_offset + 8],
                        head[entry_offset + 9],
                        head[entry_offset + 10],
                        head[entry_offset + 11],
                    ])
                };
                if dim_val > MAX_IMAGE_DIMENSION {
                    return Err(BeamError::RawDecode(format!(
                        "RAW header dimension ({dim_val} px) exceeds maximum allowed ({MAX_IMAGE_DIMENSION} px)"
                    )));
                }
                if tag == 0x0100 {
                    ifd_width = Some(dim_val);
                } else {
                    ifd_height = Some(dim_val);
                }
                if let (Some(w), Some(h)) = (ifd_width, ifd_height) {
                    let pixels = (w as u64) * (h as u64);
                    if pixels > MAX_IMAGE_PIXELS {
                        return Err(BeamError::RawDecode(format!(
                            "RAW header resolution ({w}x{h} = {pixels} px) exceeds maximum allowed ({MAX_IMAGE_PIXELS} px)"
                        )));
                    }
                }
            }

            if matches!(tag, 0x014A | 0x8769 | 0x8825) && data_size <= 4 {
                let sub_offset = if is_le {
                    u32::from_le_bytes([
                        head[entry_offset + 8],
                        head[entry_offset + 9],
                        head[entry_offset + 10],
                        head[entry_offset + 11],
                    ]) as usize
                } else {
                    u32::from_be_bytes([
                        head[entry_offset + 8],
                        head[entry_offset + 9],
                        head[entry_offset + 10],
                        head[entry_offset + 11],
                    ]) as usize
                };
                if sub_offset < 8 || sub_offset.saturating_add(2) > total_file_size {
                    return Err(BeamError::RawDecode(format!(
                        "Corrupted TIFF IFD entry: SubIFD tag 0x{tag:04X} offset {sub_offset} invalid or exceeds file length {total_file_size}"
                    )));
                }
            }

            if data_size > 4 {
                let val_offset = if is_le {
                    u32::from_le_bytes([
                        head[entry_offset + 8],
                        head[entry_offset + 9],
                        head[entry_offset + 10],
                        head[entry_offset + 11],
                    ]) as usize
                } else {
                    u32::from_be_bytes([
                        head[entry_offset + 8],
                        head[entry_offset + 9],
                        head[entry_offset + 10],
                        head[entry_offset + 11],
                    ]) as usize
                };
                if val_offset.saturating_add(data_size) > total_file_size {
                    return Err(BeamError::RawDecode(format!(
                        "Corrupted TIFF IFD entry data at {val_offset}..{} exceeds file length {total_file_size}",
                        val_offset.saturating_add(data_size)
                    )));
                }
            }
        }

        // Read next IFD pointer if within buffered head
        if entries_end.saturating_add(4) <= head.len() {
            let next_offset = if is_le {
                u32::from_le_bytes([
                    head[entries_end],
                    head[entries_end + 1],
                    head[entries_end + 2],
                    head[entries_end + 3],
                ]) as usize
            } else {
                u32::from_be_bytes([
                    head[entries_end],
                    head[entries_end + 1],
                    head[entries_end + 2],
                    head[entries_end + 3],
                ]) as usize
            };
            current_ifd = next_offset;
        } else {
            break;
        }
    }

    Ok(())
}

fn validate_ciff_ifd(buf: &[u8], start: usize, end: usize) -> Result<()> {
    let mut visited = std::collections::HashSet::new();
    visited.insert(start);
    validate_ciff_ifd_inner(buf, start, end, 1, &mut visited)
}

fn validate_ciff_ifd_inner(
    buf: &[u8],
    start: usize,
    end: usize,
    depth: u32,
    visited: &mut std::collections::HashSet<usize>,
) -> Result<()> {
    if depth > 10 {
        return Ok(());
    }
    if end < 4 || end > buf.len() || start >= end {
        return Err(BeamError::RawDecode(
            "Corrupted CIFF: invalid block range".into(),
        ));
    }
    let valuedata_size =
        u32::from_le_bytes([buf[end - 4], buf[end - 3], buf[end - 2], buf[end - 1]]) as usize;
    let dir_offset = start.saturating_add(valuedata_size);
    if dir_offset.saturating_add(2) > end {
        return Err(BeamError::RawDecode(
            "Corrupted CIFF: directory offset out of range".into(),
        ));
    }
    let dircount = u16::from_le_bytes([buf[dir_offset], buf[dir_offset + 1]]) as usize;
    if dircount > 1024
        || dir_offset
            .saturating_add(2)
            .saturating_add(dircount.saturating_mul(10))
            > end
    {
        return Err(BeamError::RawDecode(
            "Corrupted CIFF: directory entries exceed bounds".into(),
        ));
    }
    for i in 0..dircount {
        let entry_offset = dir_offset + 2 + i * 10;
        let p = u16::from_le_bytes([buf[entry_offset], buf[entry_offset + 1]]);
        let datalocation = p & 0xc000;
        let typ = p & 0x3800;
        let (bytesize, data_offset) = match datalocation {
            0x0000 => (
                u32::from_le_bytes([
                    buf[entry_offset + 2],
                    buf[entry_offset + 3],
                    buf[entry_offset + 4],
                    buf[entry_offset + 5],
                ]) as usize,
                (u32::from_le_bytes([
                    buf[entry_offset + 6],
                    buf[entry_offset + 7],
                    buf[entry_offset + 8],
                    buf[entry_offset + 9],
                ]) as usize)
                    .saturating_add(start),
            ),
            0x4000 => (8, entry_offset + 2),
            _ => continue,
        };
        if data_offset.saturating_add(bytesize) > buf.len() {
            return Err(BeamError::RawDecode(
                "Corrupted CIFF: entry data exceeds file length".into(),
            ));
        }
        if typ == 0x2800 || typ == 0x3000 {
            if data_offset <= start {
                return Err(BeamError::RawDecode(
                    "Corrupted CIFF: subIFD offset cannot precede or equal parent offset".into(),
                ));
            }
            if !visited.insert(data_offset) {
                return Err(BeamError::RawDecode(
                    "Corrupted CIFF: cycle detected in subIFD hierarchy".into(),
                ));
            }
            validate_ciff_ifd_inner(
                buf,
                data_offset,
                data_offset.saturating_add(bytesize),
                depth + 1,
                visited,
            )?;
        }
    }
    Ok(())
}

fn validate_bmff_container_with_size(bytes: &[u8], total_file_size: usize) -> Result<()> {
    if bytes.len() < 16 {
        return Err(BeamError::RawDecode(
            "Corrupted BMFF container: truncated".into(),
        ));
    }
    if &bytes[4..8] != b"ftyp" {
        return Err(BeamError::RawDecode(
            "Corrupted BMFF: first box must be ftyp".into(),
        ));
    }

    let mut offset: usize = 0;
    while offset.saturating_add(8) <= bytes.len() {
        let size32 = u32::from_be_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;

        let box_size = if size32 == 1 {
            if offset.saturating_add(16) > bytes.len() {
                return Err(BeamError::RawDecode(
                    "Corrupted BMFF box header: 64-bit size truncated".into(),
                ));
            }
            let size64 = u64::from_be_bytes([
                bytes[offset + 8],
                bytes[offset + 9],
                bytes[offset + 10],
                bytes[offset + 11],
                bytes[offset + 12],
                bytes[offset + 13],
                bytes[offset + 14],
                bytes[offset + 15],
            ]);
            if size64 < 16 || size64 > total_file_size as u64 {
                return Err(BeamError::RawDecode(
                    "Corrupted BMFF box: 64-bit size out of range".into(),
                ));
            }
            size64 as usize
        } else if size32 < 8 {
            return Err(BeamError::RawDecode(
                "Corrupted BMFF box: box size less than 8 bytes".into(),
            ));
        } else {
            size32
        };

        let tag = &bytes[offset + 4..offset + 8];
        if tag == b"ftyp" && (offset != 0 || box_size < 16 || box_size % 4 != 0) {
            return Err(BeamError::RawDecode(
                "Corrupted BMFF ftyp box: invalid ftyp box".into(),
            ));
        }

        if offset.saturating_add(box_size) > total_file_size {
            return Err(BeamError::RawDecode(
                "Corrupted BMFF box: size exceeds file length".into(),
            ));
        }
        offset = offset.saturating_add(box_size);
    }
    Ok(())
}

/// Validate RAW container bounds (TIFF, X3F, BMFF, etc.) to prevent third-party parser panics and unbounded allocations.
pub fn validate_raw_container(bytes: &[u8]) -> Result<()> {
    validate_raw_container_with_size(bytes, bytes.len())
}

/// Validate RAW container bounds using a buffered header slice and the actual total file size.
pub fn validate_raw_container_with_size(bytes: &[u8], total_file_size: usize) -> Result<()> {
    if bytes.len() < 8 {
        return Err(BeamError::RawDecode(
            "RAW container truncated (< 8 bytes)".into(),
        ));
    }

    let is_known_container = bytes.starts_with(b"\0MRM")
        || bytes.starts_with(b"FUJIFILM")
        || bytes.starts_with(b"ARRI")
        || bytes.starts_with(b"qktk")
        || bytes.starts_with(b"qktn")
        || (bytes.len() >= 14 && &bytes[6..14] == b"HEAPCCDR")
        || bytes.starts_with(&[0xFF, 0xD8, 0xFF, 0xE1])
        || bytes.starts_with(b"FOVb")
        || (bytes.len() >= 8 && &bytes[4..8] == b"ftyp")
        || bytes.starts_with(b"II")
        || bytes.starts_with(b"MM");

    if !is_known_container {
        return Err(BeamError::RawDecode(
            "Unsupported or unrecognized RAW container format".into(),
        ));
    }

    validate_tiff_raw_structure(bytes, total_file_size)?;

    if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
        validate_bmff_container_with_size(bytes, total_file_size)?;
    }

    if bytes.starts_with(b"FOVb") {
        if bytes.len() < 16 {
            return Err(BeamError::RawDecode(
                "Corrupted X3F container: header truncated".into(),
            ));
        }
        let offset = u32::from_le_bytes([
            bytes[bytes.len() - 4],
            bytes[bytes.len() - 3],
            bytes[bytes.len() - 2],
            bytes[bytes.len() - 1],
        ]) as usize;
        if offset < 4 || offset.saturating_add(12) > bytes.len() {
            return Err(BeamError::RawDecode(
                "Corrupted X3F header: directory offset invalid or exceeds file length".into(),
            ));
        }
        let entries = u32::from_le_bytes([
            bytes[offset + 8],
            bytes[offset + 9],
            bytes[offset + 10],
            bytes[offset + 11],
        ]) as usize;
        if entries > 4096 {
            return Err(BeamError::RawDecode(
                "Corrupted X3F header: excessive directory entries".into(),
            ));
        }
        let required = offset
            .saturating_add(12)
            .saturating_add(entries.saturating_mul(12));
        if required > bytes.len() {
            return Err(BeamError::RawDecode(
                "Corrupted X3F header: directory entries exceed file length".into(),
            ));
        }
        for i in 0..entries {
            let entry_offset = offset + 12 + i * 12;
            let dir_offset = u32::from_le_bytes([
                bytes[entry_offset],
                bytes[entry_offset + 1],
                bytes[entry_offset + 2],
                bytes[entry_offset + 3],
            ]) as usize;
            let dir_len = u32::from_le_bytes([
                bytes[entry_offset + 4],
                bytes[entry_offset + 5],
                bytes[entry_offset + 6],
                bytes[entry_offset + 7],
            ]) as usize;
            if dir_offset.saturating_add(dir_len) > bytes.len() {
                return Err(BeamError::RawDecode(
                    "Corrupted X3F header: directory entry data exceeds file length".into(),
                ));
            }
            if &bytes[entry_offset + 8..entry_offset + 12] == b"IMA2"
                && dir_offset.saturating_add(28) > bytes.len()
            {
                return Err(BeamError::RawDecode(
                    "Corrupted X3F header: IMA2 entry truncated".into(),
                ));
            }
        }
    }

    if bytes.len() >= 14 && &bytes[6..14] == b"HEAPCCDR" {
        if bytes.len() < 26 {
            return Err(BeamError::RawDecode(
                "Corrupted CIFF container: truncated".into(),
            ));
        }
        if !bytes.starts_with(b"II\x1a\0\0\0") && !bytes.starts_with(b"MM\0\x1a\0\0") {
            return Err(BeamError::RawDecode(
                "Corrupted CIFF container: invalid header marker".into(),
            ));
        }
        let start = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as usize;
        validate_ciff_ifd(bytes, start, bytes.len())?;
    }

    if bytes.starts_with(b"FUJIFILM") {
        if bytes.len() < 104 {
            return Err(BeamError::RawDecode(
                "Corrupted RAF container: truncated header".into(),
            ));
        }
        let tiff1_offset =
            u32::from_be_bytes([bytes[84], bytes[85], bytes[86], bytes[87]]) as usize;
        let tiff1_start = tiff1_offset.saturating_add(12);
        if tiff1_start >= total_file_size {
            return Err(BeamError::RawDecode(
                "Corrupted RAF header: TIFF offset exceeds file".into(),
            ));
        }
        if tiff1_start < bytes.len() {
            validate_tiff_raw_structure(&bytes[tiff1_start..], total_file_size - tiff1_start)?;
        }

        let tiff2_offset =
            u32::from_be_bytes([bytes[100], bytes[101], bytes[102], bytes[103]]) as usize;
        if tiff2_offset < bytes.len() {
            validate_tiff_raw_structure(&bytes[tiff2_offset..], total_file_size - tiff2_offset)?;
        }
    }

    if bytes.starts_with(&[0xFF, 0xD8, 0xFF, 0xE1]) {
        if bytes.len() < 22 {
            return Err(BeamError::RawDecode(
                "Corrupted JPEG/EXIF container: truncated".into(),
            ));
        }
        let app1_len = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
        // APP1 must contain: 2 bytes len + 6 bytes "Exif\0\0" + 8 bytes TIFF header + 2 bytes IFD entry count = 18 bytes minimum
        if app1_len < 18 || app1_len.saturating_add(4) > total_file_size {
            return Err(BeamError::RawDecode(
                "Corrupted JPEG/EXIF container: APP1 length out of range".into(),
            ));
        }
        const APP1_EXIF_MARKER: &[u8] = b"Exif\0\0";
        if &bytes[6..12] != APP1_EXIF_MARKER {
            return Err(BeamError::RawDecode(
                "Corrupted JPEG/EXIF container: missing Exif marker".into(),
            ));
        }
        let tiff_start = 12;
        if !bytes[tiff_start..].starts_with(b"II*\0") && !bytes[tiff_start..].starts_with(b"MM\0*")
        {
            return Err(BeamError::RawDecode(
                "Corrupted JPEG/EXIF container: invalid TIFF header in EXIF APP1".into(),
            ));
        }
        let tiff_len = app1_len.saturating_add(4).saturating_sub(tiff_start);
        if tiff_len < 10 {
            return Err(BeamError::RawDecode(
                "Corrupted JPEG/EXIF container: TIFF payload too short".into(),
            ));
        }
        let avail_len = bytes.len().saturating_sub(tiff_start).min(tiff_len);
        validate_tiff_raw_structure(&bytes[tiff_start..tiff_start + avail_len], tiff_len)?;
    }

    if bytes.starts_with(b"\0MRM") {
        if bytes.len() < 40 {
            return Err(BeamError::RawDecode(
                "Corrupted MRW container: truncated".into(),
            ));
        }
        let data_offset = (u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize)
            .saturating_add(8);
        if data_offset > total_file_size {
            return Err(BeamError::RawDecode(
                "Corrupted MRW container: data offset exceeds file length".into(),
            ));
        }
        let mut currpos: usize = 8;
        let mut tiffpos: usize = 0;
        while currpos.saturating_add(8) <= data_offset.min(bytes.len()) {
            let tag = u32::from_be_bytes([
                bytes[currpos],
                bytes[currpos + 1],
                bytes[currpos + 2],
                bytes[currpos + 3],
            ]);
            let len = u32::from_be_bytes([
                bytes[currpos + 4],
                bytes[currpos + 5],
                bytes[currpos + 6],
                bytes[currpos + 7],
            ]) as usize;
            if tag == 0x545457 {
                tiffpos = currpos.saturating_add(8);
            }
            if len == 0 || currpos.saturating_add(len.saturating_add(8)) <= currpos {
                break;
            }
            currpos = currpos.saturating_add(len.saturating_add(8));
        }
        if tiffpos == 0 {
            return Err(BeamError::RawDecode(
                "Corrupted MRW container: missing TTW block".into(),
            ));
        }
        let ifd_target = tiffpos.saturating_add(8);
        if ifd_target.saturating_add(2) > bytes.len() {
            return Err(BeamError::RawDecode(
                "Corrupted MRW container: IFD offset out of range".into(),
            ));
        }
        let entry_count = u16::from_be_bytes([bytes[ifd_target], bytes[ifd_target + 1]]) as usize;
        if entry_count > 4096
            || ifd_target
                .saturating_add(2)
                .saturating_add(entry_count.saturating_mul(12))
                > bytes.len()
        {
            return Err(BeamError::RawDecode(
                "Corrupted MRW container: IFD entries exceed file length".into(),
            ));
        }

        for i in 0..entry_count {
            let entry_offset = ifd_target + 2 + i * 12;
            let type_code = u16::from_be_bytes([bytes[entry_offset + 2], bytes[entry_offset + 3]]);
            let type_size = match type_code {
                1 | 2 | 6 | 7 => 1,
                3 | 8 => 2,
                4 | 9 | 11 | 13 => 4,
                5 | 10 | 12 => 8,
                _ => 1,
            };
            let count = u32::from_be_bytes([
                bytes[entry_offset + 4],
                bytes[entry_offset + 5],
                bytes[entry_offset + 6],
                bytes[entry_offset + 7],
            ]) as usize;
            if count.saturating_mul(type_size) > bytes.len() || count > bytes.len() {
                return Err(BeamError::RawDecode(
                    "Corrupted MRW IFD entry data size out of range".into(),
                ));
            }
        }
    }

    if bytes.starts_with(b"qktk") || bytes.starts_with(b"qktn") {
        if bytes.len() < 750 {
            return Err(BeamError::RawDecode(
                "Corrupted QTK container: truncated".into(),
            ));
        }
        let height = u16::from_be_bytes([bytes[544], bytes[545]]) as usize;
        let width = u16::from_be_bytes([bytes[546], bytes[547]]) as usize;
        let (min_dim, max_dim) = if height > width {
            (width, height)
        } else {
            (height, width)
        };
        if (max_dim != 640 || min_dim != 480) && (max_dim != 320 || min_dim != 240) {
            return Err(BeamError::RawDecode(format!(
                "Corrupted QTK header: invalid QuickTake dimensions {width}x{height}"
            )));
        }
        let hint = u16::from_be_bytes([bytes[552], bytes[553]]);
        let offset = if hint == 30 { 738 } else { 736 };
        let min_required = offset + (max_dim * min_dim * 3) / 8;
        if bytes.len() < min_required {
            return Err(BeamError::RawDecode(
                "Corrupted QTK container: truncated payload".into(),
            ));
        }
    }

    Ok(())
}

/// Validate TIFF header bounds to prevent third-party parser unbounded memory allocation (OOM).
pub fn validate_tiff_raw_header(bytes: &[u8]) -> Result<()> {
    validate_raw_container(bytes)
}

/// Decode camera RAW using rawler
fn decode_raw_with_rawler(path: &Path) -> Result<DecodedImage> {
    // Read up to first 64KB to validate container header structure if applicable
    if let Ok(mut f) = std::fs::File::open(path) {
        use std::io::Read;
        let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) as usize;
        let mut head = vec![0u8; 65536.min(file_size.max(16))];
        if let Ok(n) = f.read(&mut head) {
            validate_tiff_raw_structure(&head[..n], file_size)?;
            if file_size <= 65536 {
                validate_raw_container(&head[..n])?;
            } else if head.starts_with(b"FOVb") || (head.len() >= 14 && &head[6..14] == b"HEAPCCDR")
            {
                if let Ok(whole) = std::fs::read(path) {
                    validate_raw_container(&whole)?;
                }
            } else {
                validate_raw_container_with_size(&head[..n], file_size)?;
            }
        }
    }

    // Tier 2 pre-decode dimension guard: ask rawler for dummy metadata/dimensions before full
    // pixel decompression. Note: in rawler 0.8.0, most decoders (ARW, CR2, NEF, RAF, ORF, RW2, PEF)
    // skip pixel allocation when dummy=true, whereas DngDecoder::raw_image reads strip buffers before
    // checking dummy; Tier 1 (validate_tiff_raw_structure) guards TIFF/DNG IFD dimensions first.
    let preflight = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let source = rawler::rawsource::RawSource::new(path).ok()?;
        let decoder = rawler::get_decoder(&source).ok()?;
        decoder
            .raw_image(&source, &rawler::decoders::RawDecodeParams::default(), true)
            .ok()
    }));
    match preflight {
        Err(_) => {
            return Err(BeamError::RawDecode(
                "RAW parser panicked during pre-decode header inspection".into(),
            ));
        }
        Ok(Some(meta_img)) => {
            let w = meta_img.width as u32;
            let h = meta_img.height as u32;
            if w == 0 || h == 0 {
                return Err(BeamError::RawDecode(
                    "RAW image has zero width or height".into(),
                ));
            }
            if w > MAX_IMAGE_DIMENSION || h > MAX_IMAGE_DIMENSION {
                return Err(BeamError::RawDecode(format!(
                    "RAW image dimensions ({w}x{h}) exceed maximum allowed ({MAX_IMAGE_DIMENSION} px)"
                )));
            }
            let total_px = (w as u64) * (h as u64);
            if total_px > MAX_IMAGE_PIXELS {
                return Err(BeamError::RawDecode(format!(
                    "RAW image exceeds maximum resolution limit ({total_px} pixels > {MAX_IMAGE_PIXELS} pixels)"
                )));
            }
        }
        Ok(None) => {}
    }

    let decode_result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rawler::decode_file(path)));

    let rawimage = match decode_result {
        Ok(Ok(img)) => img,
        Ok(Err(e)) => return Err(BeamError::RawDecode(format!("RAW decode failed: {e:?}"))),
        Err(_) => {
            return Err(BeamError::RawDecode(
                "RAW parser panicked on corrupted input".into(),
            ))
        }
    };

    let orig_w = rawimage.width as u32;
    let orig_h = rawimage.height as u32;

    if orig_w == 0 || orig_h == 0 {
        return Err(BeamError::RawDecode(
            "RAW image has zero width or height".into(),
        ));
    }
    if orig_w > MAX_IMAGE_DIMENSION || orig_h > MAX_IMAGE_DIMENSION {
        return Err(BeamError::RawDecode(format!(
            "RAW image dimensions ({orig_w}x{orig_h}) exceed maximum allowed ({MAX_IMAGE_DIMENSION} px)"
        )));
    }
    let total_pixels = (orig_w as u64) * (orig_h as u64);
    if total_pixels > MAX_IMAGE_PIXELS {
        return Err(BeamError::RawDecode(format!(
            "RAW image exceeds maximum resolution limit ({total_pixels} pixels > {MAX_IMAGE_PIXELS} pixels)"
        )));
    }

    let camera_make = rawimage.camera.make.clone();
    let camera_model = rawimage.camera.model.clone();

    // Use RawDevelop with WhiteBalance + Calibrate to produce D65 Linear sRGB,
    // then transform into wide-gamut Linear Rec.2020 working space.
    let develop = RawDevelop {
        steps: vec![
            ProcessingStep::Rescale,
            ProcessingStep::Demosaic,
            ProcessingStep::FujiRotate,
            ProcessingStep::CropActiveArea,
            ProcessingStep::WhiteBalance,
            ProcessingStep::Calibrate,
        ],
    };

    let develop_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        develop.develop_intermediate(&rawimage)
    }));
    let intermediate = match develop_result {
        Ok(Ok(inter)) => inter,
        Ok(Err(e)) => {
            return Err(BeamError::RawDecode(format!(
                "RAW development/demosaicing failed: {e:?}"
            )))
        }
        Err(_) => {
            return Err(BeamError::RawDecode(
                "RAW development panicked on corrupted input".into(),
            ))
        }
    };

    let (width, height, mut rgba_floats) = match intermediate {
        Intermediate::ThreeColor(pixels) => {
            let dim = pixels.dim();
            let count = dim.w * dim.h;
            let mut rgba = Vec::with_capacity(count * 4);
            let flat = pixels.flatten(); // &[f32] length 3 * count (Linear sRGB after Calibrate)
            for chunk in flat.as_chunks::<3>().0 {
                let rec = beam_core::color::mat3_mul_vec3(
                    &beam_core::color::SRGB_TO_REC2020,
                    [chunk[0], chunk[1], chunk[2]],
                );
                rgba.push(rec[0].max(0.0));
                rgba.push(rec[1].max(0.0));
                rgba.push(rec[2].max(0.0));
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
                let rec = beam_core::color::mat3_mul_vec3(
                    &beam_core::color::SRGB_TO_REC2020,
                    [chunk[0], (chunk[1] + chunk[3]) * 0.5, chunk[2]],
                );
                rgba.push(rec[0].max(0.0));
                rgba.push(rec[1].max(0.0));
                rgba.push(rec[2].max(0.0));
                rgba.push(1.0);
            }
            (dim.w as u32, dim.h as u32, rgba)
        }
        Intermediate::Monochrome(pixels) => {
            let dim = pixels.dim();
            let count = dim.w * dim.h;
            let mut rgba = Vec::with_capacity(count * 4);
            for &gray in &pixels.data {
                let g = gray.max(0.0);
                rgba.push(g);
                rgba.push(g);
                rgba.push(g);
                rgba.push(1.0);
            }
            (dim.w as u32, dim.h as u32, rgba)
        }
    };

    // Ensure values are finite and non-negative
    for val in &mut rgba_floats {
        if !val.is_finite() || *val < 0.0 {
            *val = 0.0;
        }
    }

    // Apply EXIF orientation if needed
    let orient_u16 = rawimage.orientation.to_u16();
    let (oriented_w, oriented_h, oriented_data) = if orient_u16 > 1 && orient_u16 <= 8 {
        apply_orientation_f32(width, height, &rgba_floats, orient_u16)
    } else {
        (width, height, rgba_floats)
    };

    let (proxy_w, proxy_h, proxy_data) =
        generate_proxy(oriented_w, oriented_h, &oriented_data, PROXY_MAX_DIMENSION);

    let exif = extract_exif_metadata(path);

    Ok(DecodedImage {
        width: oriented_w,
        height: oriented_h,
        data: oriented_data,
        proxy_width: proxy_w,
        proxy_height: proxy_h,
        proxy_data,
        metadata: ImageMetadata {
            file_path: path.to_path_buf(),
            camera_make: if camera_make.is_empty() {
                exif.make
            } else {
                Some(camera_make)
            },
            camera_model: if camera_model.is_empty() {
                exif.model
            } else {
                Some(camera_model)
            },
            iso: exif.iso,
            shutter_speed: exif.shutter,
            aperture: exif.aperture,
            focal_length: exif.focal_length,
            original_dimensions: (orig_w, orig_h),
            orientation: 1,
        },
    })
}

/// Converts non-linear sRGB (0.0..=1.0) to wide-gamut linear Rec.2020.
#[inline]
pub fn srgb_to_rec2020_linear(r: f32, g: f32, b: f32) -> [f32; 3] {
    let lin_r = beam_core::color::srgb_to_linear(r.clamp(0.0, 1.0));
    let lin_g = beam_core::color::srgb_to_linear(g.clamp(0.0, 1.0));
    let lin_b = beam_core::color::srgb_to_linear(b.clamp(0.0, 1.0));
    beam_core::color::mat3_mul_vec3(&beam_core::color::SRGB_TO_REC2020, [lin_r, lin_g, lin_b])
}

/// Parsed EXIF metadata from an image file.
#[derive(Debug, Clone, Default)]
pub struct ExifMetadata {
    pub orientation: u16,
    pub make: Option<String>,
    pub model: Option<String>,
    pub iso: Option<u32>,
    pub shutter: Option<String>,
    pub aperture: Option<f32>,
    pub focal_length: Option<f32>,
}

/// Extract fast image dimensions from header without full pixel decoding.
pub fn extract_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    if is_raster_file(path) {
        return image::image_dimensions(path).ok();
    }
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_IMAGE_FILE_SIZE {
        return None;
    }
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let source = rawler::rawsource::RawSource::new(path).ok()?;
        let decoder = rawler::get_decoder(&source).ok()?;
        let raw = decoder
            .raw_image(&source, &rawler::decoders::RawDecodeParams::default(), true)
            .ok()?;
        let (w, h) = (raw.width as u32, raw.height as u32);
        if w == 0 || h == 0 {
            None
        } else {
            Some((w, h))
        }
    }));
    res.ok().flatten()
}

/// Extract EXIF metadata (orientation, make, model, iso, shutter, aperture, focal_length) using kamadak-exif.
pub fn extract_exif_metadata(path: &Path) -> ExifMetadata {
    let mut meta = ExifMetadata {
        orientation: 1,
        ..Default::default()
    };

    if let Ok(file) = File::open(path) {
        let mut buf_reader = BufReader::new(file);
        if let Ok(exif) = Reader::new().read_from_container(&mut buf_reader) {
            if let Some(field) = exif.get_field(Tag::Orientation, In::PRIMARY) {
                if let Value::Short(ref v) = field.value {
                    if let Some(&orient) = v.first() {
                        meta.orientation = orient;
                    }
                }
            }
            if let Some(field) = exif.get_field(Tag::Make, In::PRIMARY) {
                let s = field.display_value().to_string();
                let clean = s.trim_matches('"').trim();
                if !clean.is_empty() {
                    meta.make = Some(clean.to_string());
                }
            }
            if let Some(field) = exif.get_field(Tag::Model, In::PRIMARY) {
                let s = field.display_value().to_string();
                let clean = s.trim_matches('"').trim();
                if !clean.is_empty() {
                    meta.model = Some(clean.to_string());
                }
            }
            if let Some(field) = exif.get_field(Tag::PhotographicSensitivity, In::PRIMARY) {
                if let Value::Short(ref v) = field.value {
                    meta.iso = v.first().map(|&x| x as u32);
                }
            }
            if let Some(field) = exif.get_field(Tag::ExposureTime, In::PRIMARY) {
                meta.shutter = Some(field.display_value().to_string());
            }
            if let Some(field) = exif.get_field(Tag::FNumber, In::PRIMARY) {
                if let Value::Rational(ref v) = field.value {
                    if let Some(r) = v.first() {
                        if r.denom != 0 {
                            meta.aperture = Some(r.num as f32 / r.denom as f32);
                        }
                    }
                }
            }
            if let Some(field) = exif.get_field(Tag::FocalLength, In::PRIMARY) {
                if let Value::Rational(ref v) = field.value {
                    if let Some(r) = v.first() {
                        if r.denom != 0 {
                            meta.focal_length = Some(r.num as f32 / r.denom as f32);
                        }
                    }
                }
            }
        }
    }

    meta
}

/// Extract embedded ICC profile byte array from image container if present.
pub fn extract_icc_profile(path: &Path) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut image_reader = image::ImageReader::new(reader);
    if let Ok(format) = image::ImageFormat::from_path(path) {
        image_reader.set_format(format);
    }
    let mut decoder = image_reader
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    let icc = decoder.icc_profile().ok().flatten()?;
    if icc.len() > MAX_ICC_PROFILE_SIZE {
        log::warn!(
            "Embedded ICC profile in {} exceeds maximum size limit ({} bytes), skipping",
            path.display(),
            icc.len()
        );
        return None;
    }
    Some(icc)
}

/// Transform non-linear pixel buffer from embedded ICC profile to linear Rec.2020 working space using moxcms.
pub fn transform_icc_to_linear_rec2020(
    icc_bytes: &[u8],
    pixels: &mut [f32],
) -> std::result::Result<(), String> {
    if icc_bytes.len() > MAX_ICC_PROFILE_SIZE {
        return Err(format!(
            "ICC profile exceeds maximum allowed size ({} bytes > {MAX_ICC_PROFILE_SIZE} bytes)",
            icc_bytes.len()
        ));
    }
    let src_profile = ColorProfile::new_from_slice(icc_bytes)
        .map_err(|e| format!("Invalid ICC profile: {e:?}"))?;

    let dst_profile = ColorProfile::new_from_cicp(moxcms::CicpProfile {
        color_primaries: moxcms::CicpColorPrimaries::Bt2020,
        transfer_characteristics: moxcms::TransferCharacteristics::Linear,
        matrix_coefficients: moxcms::MatrixCoefficients::Identity,
        full_range: true,
    });
    let transform = src_profile
        .create_transform_f32(
            Layout::Rgba,
            &dst_profile,
            Layout::Rgba,
            TransformOptions::default(),
        )
        .map_err(|e| format!("Failed to create ICC transform: {e:?}"))?;

    // Transform in 64K-pixel chunks to avoid doubling peak RAM on 50-100 MP images
    const CHUNK_FLOATS: usize = 65_536 * 4;
    let mut scratch = vec![0.0f32; pixels.len().min(CHUNK_FLOATS)];
    for chunk in pixels.chunks_mut(CHUNK_FLOATS) {
        let len = chunk.len();
        transform
            .transform(chunk, &mut scratch[..len])
            .map_err(|e| format!("ICC transform execution failed: {e:?}"))?;
        for px in scratch[..len].as_chunks_mut::<4>().0 {
            px[0] = if px[0].is_finite() {
                px[0].max(0.0)
            } else {
                0.0
            };
            px[1] = if px[1].is_finite() {
                px[1].max(0.0)
            } else {
                0.0
            };
            px[2] = if px[2].is_finite() {
                px[2].max(0.0)
            } else {
                0.0
            };
        }
        chunk.copy_from_slice(&scratch[..len]);
    }
    Ok(())
}

/// Apply EXIF orientation transform (1..=8) to full-resolution float RGBA buffers.
pub fn apply_orientation_f32(
    src_w: u32,
    src_h: u32,
    src_data: &[f32],
    orientation: u16,
) -> (u32, u32, Vec<f32>) {
    if src_w == 0 || src_h == 0 || src_data.is_empty() {
        return (0, 0, Vec::new());
    }

    match orientation {
        2 => {
            // Flip horizontal
            let mut dst = vec![0.0f32; src_data.len()];
            for y in 0..src_h {
                for x in 0..src_w {
                    let src_idx = ((y * src_w + x) * 4) as usize;
                    let dst_idx = ((y * src_w + (src_w - 1 - x)) * 4) as usize;
                    dst[dst_idx..dst_idx + 4].copy_from_slice(&src_data[src_idx..src_idx + 4]);
                }
            }
            (src_w, src_h, dst)
        }
        3 => {
            // Rotate 180
            let mut dst = vec![0.0f32; src_data.len()];
            for y in 0..src_h {
                for x in 0..src_w {
                    let src_idx = ((y * src_w + x) * 4) as usize;
                    let dst_idx = (((src_h - 1 - y) * src_w + (src_w - 1 - x)) * 4) as usize;
                    dst[dst_idx..dst_idx + 4].copy_from_slice(&src_data[src_idx..src_idx + 4]);
                }
            }
            (src_w, src_h, dst)
        }
        4 => {
            // Flip vertical
            let mut dst = vec![0.0f32; src_data.len()];
            for y in 0..src_h {
                for x in 0..src_w {
                    let src_idx = ((y * src_w + x) * 4) as usize;
                    let dst_idx = (((src_h - 1 - y) * src_w + x) * 4) as usize;
                    dst[dst_idx..dst_idx + 4].copy_from_slice(&src_data[src_idx..src_idx + 4]);
                }
            }
            (src_w, src_h, dst)
        }
        5 => {
            // Transpose (flip horizontal & rotate 270 CW)
            let mut dst = vec![0.0f32; src_data.len()];
            for y in 0..src_h {
                for x in 0..src_w {
                    let src_idx = ((y * src_w + x) * 4) as usize;
                    let dst_idx = ((x * src_h + y) * 4) as usize;
                    dst[dst_idx..dst_idx + 4].copy_from_slice(&src_data[src_idx..src_idx + 4]);
                }
            }
            (src_h, src_w, dst)
        }
        6 => {
            // Rotate 90 CW: (x, y) -> (src_h - 1 - y, x)
            let mut dst = vec![0.0f32; src_data.len()];
            for y in 0..src_h {
                for x in 0..src_w {
                    let src_idx = ((y * src_w + x) * 4) as usize;
                    let dst_idx = ((x * src_h + (src_h - 1 - y)) * 4) as usize;
                    dst[dst_idx..dst_idx + 4].copy_from_slice(&src_data[src_idx..src_idx + 4]);
                }
            }
            (src_h, src_w, dst)
        }
        7 => {
            // Transverse (flip horizontal & rotate 90 CW)
            let mut dst = vec![0.0f32; src_data.len()];
            for y in 0..src_h {
                for x in 0..src_w {
                    let src_idx = ((y * src_w + x) * 4) as usize;
                    let dst_idx = (((src_w - 1 - x) * src_h + (src_h - 1 - y)) * 4) as usize;
                    dst[dst_idx..dst_idx + 4].copy_from_slice(&src_data[src_idx..src_idx + 4]);
                }
            }
            (src_h, src_w, dst)
        }
        8 => {
            // Rotate 270 CW: (x, y) -> (y, src_w - 1 - x)
            let mut dst = vec![0.0f32; src_data.len()];
            for y in 0..src_h {
                for x in 0..src_w {
                    let src_idx = ((y * src_w + x) * 4) as usize;
                    let dst_idx = (((src_w - 1 - x) * src_h + y) * 4) as usize;
                    dst[dst_idx..dst_idx + 4].copy_from_slice(&src_data[src_idx..src_idx + 4]);
                }
            }
            (src_h, src_w, dst)
        }
        _ => (src_w, src_h, src_data.to_vec()),
    }
}

/// Decode regular raster image formats (JPEG, PNG, TIFF 8/16-bit, WebP).
/// Preserves 16-bit precision, applies embedded ICC color management,
/// and converts into linear Rec.2020 working space.
pub fn decode_raster_image(path: &Path) -> Result<DecodedImage> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut image_reader = image::ImageReader::new(reader);
    if let Ok(format) = image::ImageFormat::from_path(path) {
        image_reader.set_format(format);
    }
    let mut image_reader = image_reader
        .with_guessed_format()
        .map_err(|e| BeamError::UnsupportedFormat(e.to_string()))?;

    // Enforce decoding limits (dimensions and memory budget) to block decompression bombs
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_IMAGE_DECODE_ALLOC);
    image_reader.limits(limits);

    // 1. Extract embedded ICC profile before decoding
    let icc_bytes = extract_icc_profile(path);

    // 2. Extract EXIF metadata & orientation
    let exif_meta = extract_exif_metadata(path);
    let orientation = exif_meta.orientation;

    // 3. Decode image with full bit depth
    let dyn_img = image_reader
        .decode()
        .map_err(|e| BeamError::UnsupportedFormat(e.to_string()))?;

    let (width, height) = (dyn_img.width(), dyn_img.height());

    if width == 0 || height == 0 {
        return Err(BeamError::ImageProcessing(
            "Image has zero width or height".into(),
        ));
    }
    let total_pixels = (width as u64) * (height as u64);
    if total_pixels > MAX_IMAGE_PIXELS {
        return Err(BeamError::ImageProcessing(format!(
            "Image exceeds maximum allowed resolution ({total_pixels} pixels > {MAX_IMAGE_PIXELS} pixels)"
        )));
    }

    // 4. Convert pixels to normalized f32 RGBA, strictly keeping 16-bit precision when present
    let mut rgba_floats = match dyn_img {
        image::DynamicImage::ImageRgb16(rgb16) => {
            let mut data = Vec::with_capacity((width * height * 4) as usize);
            for p in rgb16.pixels() {
                data.push(p[0] as f32 / 65535.0);
                data.push(p[1] as f32 / 65535.0);
                data.push(p[2] as f32 / 65535.0);
                data.push(1.0);
            }
            data
        }
        image::DynamicImage::ImageRgba16(rgba16) => {
            let mut data = Vec::with_capacity((width * height * 4) as usize);
            for p in rgba16.pixels() {
                data.push(p[0] as f32 / 65535.0);
                data.push(p[1] as f32 / 65535.0);
                data.push(p[2] as f32 / 65535.0);
                data.push(p[3] as f32 / 65535.0);
            }
            data
        }
        image::DynamicImage::ImageLuma16(luma16) => {
            let mut data = Vec::with_capacity((width * height * 4) as usize);
            for p in luma16.pixels() {
                let v = p[0] as f32 / 65535.0;
                data.push(v);
                data.push(v);
                data.push(v);
                data.push(1.0);
            }
            data
        }
        image::DynamicImage::ImageLumaA16(luma_a16) => {
            let mut data = Vec::with_capacity((width * height * 4) as usize);
            for p in luma_a16.pixels() {
                let v = p[0] as f32 / 65535.0;
                let a = p[1] as f32 / 65535.0;
                data.push(v);
                data.push(v);
                data.push(v);
                data.push(a);
            }
            data
        }
        other => {
            let rgba8 = other.to_rgba8();
            let mut data = Vec::with_capacity((width * height * 4) as usize);
            for p in rgba8.pixels() {
                data.push(p[0] as f32 / 255.0);
                data.push(p[1] as f32 / 255.0);
                data.push(p[2] as f32 / 255.0);
                data.push(p[3] as f32 / 255.0);
            }
            data
        }
    };

    // 5. Color management: Apply embedded ICC profile or fall back to standard sRGB
    let mut icc_applied = false;
    if let Some(ref icc) = icc_bytes {
        if !icc.is_empty() {
            match transform_icc_to_linear_rec2020(icc, &mut rgba_floats) {
                Ok(()) => {
                    icc_applied = true;
                }
                Err(err) => {
                    log::warn!(
                        "Embedded ICC profile in {} unsupported/failed: {err}. Falling back to sRGB.",
                        path.display()
                    );
                }
            }
        }
    }

    if !icc_applied {
        for chunk in rgba_floats.as_chunks_mut::<4>().0 {
            let lin = srgb_to_rec2020_linear(chunk[0], chunk[1], chunk[2]);
            chunk[0] = lin[0].max(0.0);
            chunk[1] = lin[1].max(0.0);
            chunk[2] = lin[2].max(0.0);
        }
    }

    // 6. Apply EXIF orientation transform if needed
    let (oriented_w, oriented_h, oriented_data) = if orientation > 1 && orientation <= 8 {
        apply_orientation_f32(width, height, &rgba_floats, orientation)
    } else {
        (width, height, rgba_floats)
    };

    let (proxy_w, proxy_h, proxy_data) =
        generate_proxy(oriented_w, oriented_h, &oriented_data, PROXY_MAX_DIMENSION);

    Ok(DecodedImage {
        width: oriented_w,
        height: oriented_h,
        data: oriented_data,
        proxy_width: proxy_w,
        proxy_height: proxy_h,
        proxy_data,
        metadata: ImageMetadata {
            file_path: path.to_path_buf(),
            camera_make: exif_meta.make,
            camera_model: exif_meta.model,
            iso: exif_meta.iso,
            shutter_speed: exif_meta.shutter,
            aperture: exif_meta.aperture,
            focal_length: exif_meta.focal_length,
            original_dimensions: (oriented_w, oriented_h),
            orientation: 1, // Oriented in pixel buffer
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
    let has_edits = edit_stack
        .map(|s| s != &EditStack::default())
        .unwrap_or(false);
    if !has_edits && is_raster_file(path) {
        if let Ok(mut reader) = image::ImageReader::open(path) {
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
            limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
            limits.max_alloc = Some(MAX_IMAGE_DECODE_ALLOC);
            reader.limits(limits);
            if let Ok(img) = reader.decode() {
                let thumb = img.thumbnail(max_dim, max_dim).to_rgba8();
                let exif = extract_exif_metadata(path);
                let oriented = apply_orientation(thumb, exif.orientation);
                return Ok(oriented);
            }
        }
    } else if !has_edits {
        let embedded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let meta = std::fs::metadata(path).ok()?;
            if meta.len() > MAX_IMAGE_FILE_SIZE {
                return None;
            }
            let source = rawler::rawsource::RawSource::new(path).ok()?;
            let decoder = rawler::get_decoder(&source).ok()?;
            let params = rawler::decoders::RawDecodeParams::default();
            decoder
                .thumbnail_image(&source, &params)
                .ok()
                .flatten()
                .or_else(|| decoder.preview_image(&source, &params).ok().flatten())
        }));
        if let Ok(Some(img)) = embedded {
            let thumb = img.thumbnail(max_dim, max_dim).to_rgba8();
            let exif = extract_exif_metadata(path);
            let oriented = apply_orientation(thumb, exif.orientation);
            return Ok(oriented);
        }
    }

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

    let oriented = apply_orientation(img, decoded.metadata.orientation);
    if let Some(stack) = edit_stack {
        if stack.crop.is_active() {
            let cropped = crate::export::apply_crop_and_orientation(&oriented, &stack.crop);
            return Ok(cropped);
        }
    }

    Ok(oriented)
}

/// Generate an 8-bit sRGB thumbnail without edits (or using default settings).
pub fn generate_thumbnail(path: &Path, max_dim: u32) -> Result<image::RgbaImage> {
    generate_thumbnail_with_edits(path, None, max_dim)
}

/// Compute persistent thumbnail path in cache directory using hash of canonical path,
/// modification time, file size, and sidecar modification time/size for robust cache invalidation.
pub fn thumbnail_cache_path(image_path: &Path, cache_dir: &Path) -> PathBuf {
    #[inline]
    fn fnv1a_update(state: &mut u64, bytes: &[u8]) {
        const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;
        for &b in bytes {
            *state ^= b as u64;
            *state = state.wrapping_mul(FNV_PRIME);
        }
    }

    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let canonical = image_path
        .canonicalize()
        .unwrap_or_else(|_| image_path.to_path_buf());
    fnv1a_update(&mut hash, canonical.to_string_lossy().as_bytes());

    // Hash source file size and modification time
    if let Ok(meta) = std::fs::metadata(image_path) {
        fnv1a_update(&mut hash, &meta.len().to_le_bytes());
        if let Ok(mtime) = meta.modified() {
            if let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH) {
                fnv1a_update(&mut hash, &dur.as_millis().to_le_bytes());
            }
        }
    }

    // Hash sidecar file size and modification time if present
    let sidecar_p =
        Sidecar::find_sidecar_for(image_path).unwrap_or_else(|| Sidecar::path_for(image_path));
    if let Ok(sc_meta) = std::fs::metadata(&sidecar_p) {
        fnv1a_update(&mut hash, &sc_meta.len().to_le_bytes());
        if let Ok(sc_mtime) = sc_meta.modified() {
            if let Ok(dur) = sc_mtime.duration_since(std::time::UNIX_EPOCH) {
                fnv1a_update(&mut hash, &dur.as_millis().to_le_bytes());
            }
        }
    }

    let filename = image_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("thumb");
    cache_dir.join(format!("{hash:016x}_{filename}.jpg"))
}

/// Save an RGBA image as a thumbnail on disk (supports JPEG and PNG).
/// Save an RGBA image as a thumbnail on disk atomically (supports JPEG and PNG).
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

    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = match target_path.file_name() {
        Some(name) => {
            if ext.is_empty() {
                target_path.with_file_name(format!("{}.{pid}_{nonce}.tmp", name.to_string_lossy()))
            } else {
                target_path.with_file_name(format!(
                    "{}.{pid}_{nonce}.tmp.{ext}",
                    name.to_string_lossy()
                ))
            }
        }
        None => target_path.with_extension("tmp"),
    };

    let write_res = if ext == "jpg" || ext == "jpeg" {
        (|| -> Result<()> {
            let rgb = image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();
            let file = std::fs::File::create(&tmp_path)?;
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
            Ok(())
        })()
    } else {
        image
            .save(&tmp_path)
            .map_err(|e| BeamError::ImageProcessing(e.to_string()))
    };

    if let Err(e) = write_res {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    if let Err(e) = std::fs::rename(&tmp_path, target_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e.into());
    }

    Ok(())
}

#[cfg(test)]
#[path = "decoder_tests.rs"]
mod tests;
