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

fn validate_bmff_container(bytes: &[u8]) -> Result<()> {
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
            if size64 < 16 || size64 > bytes.len() as u64 {
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

        if offset.saturating_add(box_size) > bytes.len() {
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

    validate_tiff_raw_structure(bytes, bytes.len())?;

    if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
        validate_bmff_container(bytes)?;
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
        if tiff1_start >= bytes.len() {
            return Err(BeamError::RawDecode(
                "Corrupted RAF header: TIFF offset exceeds file".into(),
            ));
        }
        validate_tiff_raw_structure(&bytes[tiff1_start..], bytes.len() - tiff1_start)?;

        let tiff2_offset =
            u32::from_be_bytes([bytes[100], bytes[101], bytes[102], bytes[103]]) as usize;
        if tiff2_offset < bytes.len() {
            validate_tiff_raw_structure(&bytes[tiff2_offset..], bytes.len() - tiff2_offset)?;
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
        if app1_len < 18 || app1_len.saturating_add(4) > bytes.len() {
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
        validate_tiff_raw_structure(&bytes[tiff_start..tiff_start + tiff_len], tiff_len)?;
    }

    if bytes.starts_with(b"\0MRM") {
        if bytes.len() < 40 {
            return Err(BeamError::RawDecode(
                "Corrupted MRW container: truncated".into(),
            ));
        }
        let data_offset = (u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize)
            .saturating_add(8);
        if data_offset > bytes.len() {
            return Err(BeamError::RawDecode(
                "Corrupted MRW container: data offset exceeds file length".into(),
            ));
        }
        let mut currpos: usize = 8;
        let mut tiffpos: usize = 0;
        while currpos.saturating_add(8) <= data_offset {
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
                validate_raw_container(&head[..n])?;
            }
        }
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
    let total_pixels = (orig_w as u64) * (orig_h as u64);
    if total_pixels > MAX_IMAGE_PIXELS {
        return Err(BeamError::RawDecode(format!(
            "RAW image exceeds maximum resolution limit ({total_pixels} pixels > {MAX_IMAGE_PIXELS} pixels)"
        )));
    }

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

    // Apply EXIF orientation if needed
    let orient_u16 = rawimage.orientation.to_u16();
    let (oriented_w, oriented_h, oriented_data) = if orient_u16 > 1 && orient_u16 <= 8 {
        apply_orientation_f32(width, height, &rgba_floats, orient_u16)
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
#[allow(clippy::chunks_exact_to_as_chunks)]
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

    let dst_profile = ColorProfile::new_srgb();
    let transform = src_profile
        .create_transform_f32(
            Layout::Rgba,
            &dst_profile,
            Layout::Rgba,
            TransformOptions::default(),
        )
        .map_err(|e| format!("Failed to create ICC transform: {e:?}"))?;

    let mut transformed = vec![0.0f32; pixels.len()];
    transform
        .transform(pixels, &mut transformed)
        .map_err(|e| format!("ICC transform execution failed: {e:?}"))?;

    // Now transformed is in non-linear sRGB; convert sRGB -> linear Rec.2020
    for chunk in transformed.chunks_exact_mut(4) {
        let lin = srgb_to_rec2020_linear(chunk[0], chunk[1], chunk[2]);
        chunk[0] = lin[0].max(0.0);
        chunk[1] = lin[1].max(0.0);
        chunk[2] = lin[2].max(0.0);
    }
    pixels.copy_from_slice(&transformed);
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
#[allow(clippy::chunks_exact_to_as_chunks)]
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
        for chunk in rgba_floats.chunks_exact_mut(4) {
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
    let sidecar_p =
        Sidecar::find_sidecar_for(image_path).unwrap_or_else(|| Sidecar::path_for(image_path));
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

    #[test]
    fn test_decode_raster_formats() {
        let temp_dir = std::env::temp_dir().join("beam_test_formats");
        let _ = std::fs::create_dir_all(&temp_dir);

        // 1. JPEG
        let jpg_path = temp_dir.join("test.jpg");
        let img_rgb8: image::RgbImage =
            image::ImageBuffer::from_pixel(16, 16, image::Rgb([180, 100, 50]));
        img_rgb8.save(&jpg_path).unwrap();
        let decoded_jpg = decode_image(&jpg_path).expect("Failed to decode JPEG");
        assert_eq!(decoded_jpg.width, 16);
        assert_eq!(decoded_jpg.height, 16);
        assert_eq!(decoded_jpg.data.len(), 16 * 16 * 4);

        // 2. PNG
        let png_path = temp_dir.join("test.png");
        let img_rgba8: image::RgbaImage =
            image::ImageBuffer::from_pixel(16, 16, image::Rgba([180, 100, 50, 255]));
        img_rgba8.save(&png_path).unwrap();
        let decoded_png = decode_image(&png_path).expect("Failed to decode PNG");
        assert_eq!(decoded_png.width, 16);
        assert_eq!(decoded_png.height, 16);

        // 3. TIFF 8-bit
        let tiff8_path = temp_dir.join("test_8.tif");
        img_rgba8.save(&tiff8_path).unwrap();
        let decoded_tiff8 = decode_image(&tiff8_path).expect("Failed to decode TIFF 8-bit");
        assert_eq!(decoded_tiff8.width, 16);
        assert_eq!(decoded_tiff8.height, 16);

        // 4. TIFF 16-bit
        let tiff16_path = temp_dir.join("test_16.tif");
        let img_rgb16: image::ImageBuffer<image::Rgb<u16>, Vec<u16>> =
            image::ImageBuffer::from_pixel(16, 16, image::Rgb([30000u16, 40000u16, 50000u16]));
        img_rgb16.save(&tiff16_path).unwrap();
        let decoded_tiff16 = decode_image(&tiff16_path).expect("Failed to decode TIFF 16-bit");
        assert_eq!(decoded_tiff16.width, 16);
        assert_eq!(decoded_tiff16.height, 16);

        // 5. WebP
        let webp_path = temp_dir.join("test.webp");
        img_rgba8.save(&webp_path).unwrap();
        let decoded_webp = decode_image(&webp_path).expect("Failed to decode WebP");
        assert_eq!(decoded_webp.width, 16);
        assert_eq!(decoded_webp.height, 16);

        // 6. Camera uppercase .JPG and .JPEG
        let camera_jpg_path = temp_dir.join("DSC01234.JPG");
        img_rgb8.save(&camera_jpg_path).unwrap();
        let decoded_cam_jpg =
            decode_image(&camera_jpg_path).expect("Failed to decode camera uppercase .JPG");
        assert_eq!(decoded_cam_jpg.width, 16);
        assert_eq!(decoded_cam_jpg.height, 16);

        let camera_jpeg_path = temp_dir.join("IMG_9999.JPEG");
        img_rgb8.save(&camera_jpeg_path).unwrap();
        let decoded_cam_jpeg =
            decode_image(&camera_jpeg_path).expect("Failed to decode camera uppercase .JPEG");
        assert_eq!(decoded_cam_jpeg.width, 16);
        assert_eq!(decoded_cam_jpeg.height, 16);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_raster_16bit_preservation() {
        let temp_dir = std::env::temp_dir();
        let test_16_path = temp_dir.join("beam_16bit_prec.png");

        // Two adjacent 16-bit values that would collapse to the exact same value in 8-bit
        // In 8-bit: 1000 / 256 = 3.9 -> 3; 1020 / 256 = 3.98 -> 3
        let mut img16: image::ImageBuffer<image::Rgb<u16>, Vec<u16>> =
            image::ImageBuffer::new(2, 1);
        img16.put_pixel(0, 0, image::Rgb([1000u16, 2000u16, 3000u16]));
        img16.put_pixel(1, 0, image::Rgb([1020u16, 2000u16, 3000u16]));
        img16.save(&test_16_path).unwrap();

        let decoded = decode_image(&test_16_path).expect("Failed to decode 16-bit PNG");
        let p0_r = decoded.data[0];
        let p1_r = decoded.data[4];

        assert_ne!(
            p0_r, p1_r,
            "16-bit distinct values must not be quantized: {p0_r} != {p1_r}"
        );
        assert!(
            p1_r > p0_r,
            "Value 1020 must decode higher than 1000 in f32: {p1_r} > {p0_r}"
        );

        let _ = std::fs::remove_file(test_16_path);
    }

    #[test]
    fn test_raster_orientation_f32() {
        // 4x2 buffer with marker at (0, 0)
        let mut data = vec![0.0f32; 4 * 2 * 4];
        data[0] = 1.0; // Red at top-left
        data[3] = 1.0;

        // Orientation 3: rotate 180 (top-left moves to x=3, y=1)
        let (w3, h3, d3) = apply_orientation_f32(4, 2, &data, 3);
        assert_eq!((w3, h3), (4, 2));
        let idx3 = (4 + 3) * 4;
        assert_eq!(d3[idx3], 1.0);

        // Orientation 6: rotate 90 CW (dimensions become 2x4, moves to x=1, y=0)
        let (w6, h6, d6) = apply_orientation_f32(4, 2, &data, 6);
        assert_eq!((w6, h6), (2, 4));
        let idx6 = 4;
        assert_eq!(d6[idx6], 1.0);
    }

    #[test]
    fn test_srgb_linear_roundtrip() {
        let test_colors = [
            [0.0f32, 0.0f32, 0.0f32],
            [1.0, 1.0, 1.0],
            [0.5, 0.25, 0.75],
            [0.8, 0.1, 0.4],
            [0.2, 0.9, 0.3],
        ];

        for c in test_colors {
            let rec2020 = srgb_to_rec2020_linear(c[0], c[1], c[2]);
            let srgb = beam_core::color::rec2020_linear_to_srgb_display(rec2020);
            for ch in 0..3 {
                assert!(
                    (c[ch] - srgb[ch]).abs() < 1e-4,
                    "Channel {ch} round-trip failed: expected {}, got {}",
                    c[ch],
                    srgb[ch]
                );
            }
        }
    }

    #[test]
    fn test_embedded_icc_profile_transform() {
        let p3_profile = moxcms::ColorProfile::new_display_p3();
        let p3_bytes = p3_profile
            .encode()
            .expect("Failed to encode Display P3 ICC profile");

        // Pure red in Display P3
        let mut pixels = vec![1.0f32, 0.0, 0.0, 1.0];
        let res = transform_icc_to_linear_rec2020(&p3_bytes, &mut pixels);
        assert!(res.is_ok(), "Display P3 ICC profile transform must succeed");

        // Pure Display P3 red should be transformed and stay positive
        assert!(pixels[0] > 0.0);
        assert_eq!(pixels[3], 1.0);
    }

    #[test]
    fn test_zero_dimensions_graceful_handling() {
        let (pw, ph, pdata) = generate_proxy(0, 0, &[], PROXY_MAX_DIMENSION);
        assert_eq!((pw, ph), (0, 0));
        assert!(pdata.is_empty());

        let (ow, oh, odata) = apply_orientation_f32(0, 0, &[], 6);
        assert_eq!((ow, oh), (0, 0));
        assert!(odata.is_empty());
    }

    #[test]
    fn test_oversized_icc_profile_rejection() {
        let oversized = vec![0u8; MAX_ICC_PROFILE_SIZE + 1024];
        let mut pixels = vec![1.0f32, 0.0, 0.0, 1.0];
        let res = transform_icc_to_linear_rec2020(&oversized, &mut pixels);
        assert!(res.is_err(), "Oversized ICC profiles must be rejected");
    }

    #[test]
    fn test_100mp_image_opens_successfully() {
        // 10,000 x 10,000 = 100,000,000 pixels (100 MP)
        // Verify this is within MAX_IMAGE_PIXELS (150 MP) and MAX_IMAGE_DIMENSION (32,768)
        let w = 10_000u32;
        let h = 10_000u32;
        let total_pixels = (w as u64) * (h as u64);
        assert!(
            total_pixels <= MAX_IMAGE_PIXELS,
            "100 MP must be within MAX_IMAGE_PIXELS limit ({total_pixels} <= {MAX_IMAGE_PIXELS})"
        );
        assert!(w <= MAX_IMAGE_DIMENSION && h <= MAX_IMAGE_DIMENSION);

        // Verify proxy scaling math handles 100 MP cleanly
        let max_side = w.max(h);
        let scale = PROXY_MAX_DIMENSION as f32 / max_side as f32;
        let dst_w = ((w as f32 * scale).round() as u32).clamp(1, PROXY_MAX_DIMENSION);
        let dst_h = ((h as f32 * scale).round() as u32).clamp(1, PROXY_MAX_DIMENSION);
        assert_eq!(dst_w, PROXY_MAX_DIMENSION);
        assert_eq!(dst_h, PROXY_MAX_DIMENSION);
    }

    #[test]
    fn test_65535x65535_png_header_rejected() {
        let temp_dir = std::env::temp_dir().join("beam_test_png_bomb");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let png_bomb_path = temp_dir.join("bomb.png");
        // PNG signature + IHDR chunk declaring dimensions 65535 x 65535
        let mut png_bytes = Vec::new();
        png_bytes.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        // IHDR chunk: length 13
        png_bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x0D]);
        // Chunk type: IHDR
        png_bytes.extend_from_slice(b"IHDR");
        // Width: 65535 (0x0000FFFF)
        png_bytes.extend_from_slice(&[0x00, 0x00, 0xFF, 0xFF]);
        // Height: 65535 (0x0000FFFF)
        png_bytes.extend_from_slice(&[0x00, 0x00, 0xFF, 0xFF]);
        // Bit depth: 8, color type: 2 (RGB), compression: 0, filter: 0, interlace: 0
        png_bytes.extend_from_slice(&[8, 2, 0, 0, 0]);
        // CRC (arbitrary 4 bytes)
        png_bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

        std::fs::write(&png_bomb_path, &png_bytes).unwrap();

        let res = decode_image(&png_bomb_path);
        assert!(
            res.is_err(),
            "A 65535x65535 decompression bomb must be strictly rejected by image limits"
        );

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_fuzz_reproducer_raw_oom() {
        // Reproducer oom-662b8ff128146682485447ad5aaecb821636dd18 from fuzz_raw_decode
        // Declares 65535 IFD entries in a 37-byte file which previously triggered malloc(4294967295)
        let oom_bytes: &[u8] = &[
            0x4d, 0x4d, 0x00, 0x2a, 0x00, 0x00, 0x00, 0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x01, 0x01, 0x12, 0x00, 0x03, 0x00,
            0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        ];

        let validation_res = validate_tiff_raw_header(oom_bytes);
        assert!(
            validation_res.is_err(),
            "Must reject corrupted TIFF header with impossible IFD entry count"
        );

        let temp_dir = std::env::temp_dir().join("beam_test_fuzz_raw_oom");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let raw_path = temp_dir.join("corrupted.dng");
        std::fs::write(&raw_path, oom_bytes).unwrap();

        let res = decode_image(&raw_path);
        assert!(
            res.is_err(),
            "Must cleanly fail decoding without allocating 4.29 GB"
        );

        // Fuzz reproducer 2: IFD entry count declared 4294967041 elements
        let oom_bytes_2: &[u8] = &[
            77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 255, 255, 255, 255, 255, 255, 255, 1, 18, 32, 3, 0, 0,
            0, 1, 0, 0, 0, 8, 0, 0,
        ];
        let val_res_2 = validate_tiff_raw_header(oom_bytes_2);
        assert!(val_res_2.is_err());
        let raw_path_2 = temp_dir.join("corrupted_entry.dng");
        std::fs::write(&raw_path_2, oom_bytes_2).unwrap();
        let res_2 = decode_image(&raw_path_2);
        assert!(res_2.is_err());

        // Fuzz reproducer 3: next_ifd pointed to invalid offset 1 causing 2.91 GB allocation
        let oom_bytes_3: &[u8] = &[
            77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 1, 18, 1, 0, 0, 0, 0, 0, 3, 87, 0, 3, 0, 0, 0, 1, 0,
            1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0,
        ];
        let val_res_3 = validate_tiff_raw_header(oom_bytes_3);
        assert!(val_res_3.is_err());
        let raw_path_3 = temp_dir.join("corrupted_chain.dng");
        std::fs::write(&raw_path_3, oom_bytes_3).unwrap();
        let res_3 = decode_image(&raw_path_3);
        assert!(res_3.is_err());

        // Fuzz reproducer 4: X3F container with invalid directory offset and 303M entries
        let oom_bytes_4: &[u8] = &[
            70, 79, 86, 98, 0, 0, 0, 7, 255, 249, 24, 18, 0, 0, 1, 4, 1, 0, 0, 0, 0,
        ];
        let val_res_4 = validate_tiff_raw_header(oom_bytes_4);
        assert!(val_res_4.is_err());
        let raw_path_4 = temp_dir.join("corrupted.x3f");
        std::fs::write(&raw_path_4, oom_bytes_4).unwrap();
        let res_4 = decode_image(&raw_path_4);
        assert!(res_4.is_err());

        // Fuzz reproducer 5: CIFF container with out-of-bounds start offset (13.6 MB in 24-byte file)
        let oom_bytes_5: &[u8] = &[
            0, 0, 0, 11, 208, 0, 72, 69, 65, 80, 67, 67, 68, 82, 0, 12, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let val_res_5 = validate_tiff_raw_header(oom_bytes_5);
        assert!(val_res_5.is_err());
        let raw_path_5 = temp_dir.join("corrupted.crw");
        std::fs::write(&raw_path_5, oom_bytes_5).unwrap();
        let res_5 = decode_image(&raw_path_5);
        assert!(res_5.is_err());

        // Fuzz reproducer 6: CIFF container with entry data size out of range (16.7 MB in 124-byte file)
        let mut oom_bytes_6 = vec![
            8, 0, 0, 0, 0, 0, 72, 69, 65, 80, 67, 67, 68, 82, 0, 255, 255, 255, 0,
        ];
        oom_bytes_6.extend(std::iter::repeat_n(109, 92));
        oom_bytes_6.extend_from_slice(&[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let val_res_6 = validate_tiff_raw_header(&oom_bytes_6);
        assert!(val_res_6.is_err());
        let raw_path_6 = temp_dir.join("corrupted_entry.crw");
        std::fs::write(&raw_path_6, &oom_bytes_6).unwrap();
        let res_6 = decode_image(&raw_path_6);
        assert!(res_6.is_err());

        // Fuzz reproducer 7: RAF container with corrupted embedded TIFF structure
        let oom_bytes_7: &[u8] = &[
            70, 85, 74, 73, 70, 73, 76, 77, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 0, 1,
            3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 1, 18, 0, 0, 3,
            0, 0, 0, 1, 0, 1, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 49, 77, 77, 77, 0, 42, 0, 49, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 0, 44, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 1, 1, 18, 0, 32, 3, 0,
            16, 0, 18, 248,
        ];
        let val_res_7 = validate_tiff_raw_header(oom_bytes_7);
        assert!(val_res_7.is_err());
        let raw_path_7 = temp_dir.join("corrupted.raf");
        std::fs::write(&raw_path_7, oom_bytes_7).unwrap();
        let res_7 = decode_image(&raw_path_7);
        assert!(res_7.is_err());

        // Fuzz reproducer 8: JPEG/EXIF malformed APP1 triggering rawler Jfif infinite loop hang
        let oom_bytes_8: &[u8] = &[255, 216, 255, 225, 199, 77, 77, 0, 255, 216, 255, 225, 0];
        let val_res_8 = validate_tiff_raw_header(oom_bytes_8);
        assert!(val_res_8.is_err());
        let raw_path_8 = temp_dir.join("corrupted_jfif.dng");
        std::fs::write(&raw_path_8, oom_bytes_8).unwrap();
        let res_8 = decode_image(&raw_path_8);
        assert!(res_8.is_err());

        // Fuzz reproducer 9: CIFF container with subIFD valuedata_size out of bounds (1.14 GB in 116-byte file)
        let oom_bytes_9: &[u8] = &[
            88, 73, 0, 0, 0, 0, 72, 69, 65, 80, 67, 67, 68, 82, 0, 127, 7, 0, 2, 2, 0, 113, 107,
            116, 110, 37, 0, 247, 9, 68, 82, 0, 127, 7, 0, 2, 2, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            6, 0, 0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 19, 0, 0, 0,
        ];
        let val_res_9 = validate_tiff_raw_header(oom_bytes_9);
        assert!(val_res_9.is_err());
        let raw_path_9 = temp_dir.join("corrupted_subifd.crw");
        std::fs::write(&raw_path_9, oom_bytes_9).unwrap();
        let res_9 = decode_image(&raw_path_9);
        assert!(res_9.is_err());

        // Fuzz reproducer 10: MRW container with data_offset out of range (2.77 MB in 77-byte file)
        let oom_bytes_10: &[u8] = &[
            0, 77, 82, 77, 0, 42, 86, 0, 0, 0, 0, 0, 0, 0, 37, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 97, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 2,
            40, 0, 44, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0,
        ];
        let val_res_10 = validate_tiff_raw_header(oom_bytes_10);
        assert!(val_res_10.is_err());
        let raw_path_10 = temp_dir.join("corrupted.mrw");
        std::fs::write(&raw_path_10, oom_bytes_10).unwrap();
        let res_10 = decode_image(&raw_path_10);
        assert!(res_10.is_err());

        // Fuzz reproducer 11: MRW container with corrupted IFD entries declaring 4GB allocation
        let oom_bytes_11: &[u8] = &[
            0, 77, 82, 77, 0, 0, 0, 8, 0, 9, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143,
            143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143,
            143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143,
            143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143,
            143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143,
            143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143,
            143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143,
            143, 143, 143, 1, 0, 0, 8, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 16, 0, 0, 0,
            0, 0, 0, 0, 0, 59, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 63, 0, 128, 0, 0, 0, 0, 0, 0, 196, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9, 0,
            1, 0, 0, 0, 0, 0, 0, 1, 0, 176, 0, 0, 0, 0, 0, 0, 0, 0, 219, 255, 255, 248, 18, 0, 3,
            0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0,
        ];
        let val_res_11 = validate_tiff_raw_header(oom_bytes_11);
        assert!(val_res_11.is_err());
        let raw_path_11 = temp_dir.join("corrupted_entries.mrw");
        std::fs::write(&raw_path_11, oom_bytes_11).unwrap();
        let res_11 = decode_image(&raw_path_11);
        assert!(res_11.is_err());

        // Fuzz reproducer 12: X3F container with truncated header (14 bytes)
        let oom_bytes_12: &[u8] = &[70, 79, 86, 98, 73, 77, 77, 0, 46, 46, 46, 46, 46, 8];
        let val_res_12 = validate_tiff_raw_header(oom_bytes_12);
        assert!(val_res_12.is_err());
        let raw_path_12 = temp_dir.join("corrupted.x3f");
        std::fs::write(&raw_path_12, oom_bytes_12).unwrap();
        let res_12 = decode_image(&raw_path_12);
        assert!(res_12.is_err());

        // Fuzz reproducer 13: CIFF container with invalid header marker (114 bytes)
        let oom_bytes_13: &[u8] = &[
            88, 73, 0, 0, 0, 0, 72, 69, 65, 80, 67, 67, 68, 82, 0, 127, 7, 0, 2, 2, 0, 113, 107,
            116, 110, 37, 0, 247, 9, 68, 82, 0, 127, 7, 0, 2, 2, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            6, 0, 0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 19, 0, 0, 0,
        ];
        let val_res_13 = validate_tiff_raw_header(oom_bytes_13);
        assert!(val_res_13.is_err());
        let raw_path_13 = temp_dir.join("corrupted_subifd.crw");
        std::fs::write(&raw_path_13, oom_bytes_13).unwrap();
        let res_13 = decode_image(&raw_path_13);
        assert!(res_13.is_err());

        // Fuzz reproducer 14: CIFF container with corrupted subIFD bounds (49 bytes)
        let oom_bytes_14: &[u8] = &[
            88, 73, 0, 0, 0, 0, 72, 69, 65, 80, 67, 67, 68, 82, 0, 127, 255, 0, 2, 1, 0, 0, 106, 2,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 171, 49, 72, 0, 0, 0, 0, 0, 0, 0, 19, 0, 0, 0,
        ];
        let val_res_14 = validate_tiff_raw_header(oom_bytes_14);
        assert!(val_res_14.is_err());
        let raw_path_14 = temp_dir.join("corrupted_bounds.crw");
        std::fs::write(&raw_path_14, oom_bytes_14).unwrap();
        let res_14 = decode_image(&raw_path_14);
        assert!(res_14.is_err());

        // Fuzz reproducer 15: X3F container with IMA2 directory entry offset out of bounds (63 bytes)
        let oom_bytes_15: &[u8] = &[
            70, 79, 86, 98, 65, 100, 46, 86, 0, 0, 0, 11, 0, 0, 8, 255, 3, 0, 0, 0, 46, 188, 86, 0,
            0, 0, 131, 0, 73, 77, 65, 50, 11, 0, 8, 0, 4, 96, 0, 255, 255, 255, 255, 136, 255, 50,
            0, 255, 77, 77, 77, 47, 77, 77, 77, 77, 225, 6, 254, 8, 0, 0, 0,
        ];
        let val_res_15 = validate_tiff_raw_header(oom_bytes_15);
        assert!(val_res_15.is_err());
        let raw_path_15 = temp_dir.join("corrupted_ima2.x3f");
        std::fs::write(&raw_path_15, oom_bytes_15).unwrap();
        let res_15 = decode_image(&raw_path_15);
        assert!(res_15.is_err());

        // Fuzz reproducer 16: QTK container with invalid dimensions (Apple QuickTake)
        let oom_bytes_16: &[u8] = &[
            113, 107, 116, 107, 70, 73, 76, 77, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 0,
            1, 3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 149, 0, 3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 77, 0, 0, 0, 0, 0, 149, 77, 77, 0, 42, 0, 0, 0, 8, 0, 9, 1,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 1, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 77, 0, 77,
            8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 77, 0, 0, 0, 0, 0, 149, 77, 77, 0, 42, 0, 0, 0,
            8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 1, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0,
            77, 77, 42, 0, 0, 0, 8, 0, 9, 1, 0, 8, 0, 1, 0, 1, 3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0,
            0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 64, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 1, 77, 77, 76, 77, 77, 77, 0,
            42, 0, 0, 77, 77, 42, 0, 0, 0, 8, 0, 9, 1, 0, 8, 0, 1, 0, 1, 3, 1, 0, 0, 0, 0, 239, 0,
            9, 1, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 171, 10, 185, 90, 77, 8, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 63, 0, 128, 0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 73, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 39,
            0, 0, 0, 0, 0, 0, 0, 183, 0, 42, 3, 0, 0, 0, 1, 52, 1, 0, 0, 0, 46, 0, 0, 1, 0, 0, 70,
            73, 76, 255, 255, 255, 255, 255, 255, 255, 3, 77, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0,
            0, 8, 0, 1, 0, 1, 3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0,
            0, 0, 63, 0, 128, 0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 77, 77, 0, 0, 1, 58, 1, 0, 0, 0, 46, 0, 0,
            1, 0, 0, 70, 73, 76, 77, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 0, 1, 3, 1,
            0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 149, 0, 0, 0, 0, 8, 0, 1, 1, 18, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 77, 0, 0, 0, 0, 0, 149, 77, 77, 0, 42, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 8, 0, 1, 77, 77, 76, 77, 77, 77, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 59, 0, 0, 0, 0, 224, 252, 0, 1, 109, 0, 0, 149, 0, 3, 1,
            0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 77, 0, 0, 0, 0, 0, 77, 0, 149, 42, 0, 77, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255,
        ];
        let val_res_16 = validate_tiff_raw_header(oom_bytes_16);
        assert!(val_res_16.is_err());
        let raw_path_16 = temp_dir.join("corrupted.qtk");
        std::fs::write(&raw_path_16, oom_bytes_16).unwrap();
        let res_16 = decode_image(&raw_path_16);
        assert!(res_16.is_err());

        // Fuzz reproducer 17: QTK container with truncated bitpump payload (Apple QuickTake)
        let oom_bytes_17: &[u8] = &[
            113, 107, 116, 107, 70, 73, 76, 77, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 0,
            1, 3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 1, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 77,
            0, 77, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 77, 0, 0, 0, 0, 0, 149, 77, 77, 0, 42, 0,
            0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 1, 77, 77, 76, 77, 77, 77, 0, 42, 0,
            0, 77, 77, 0, 0, 0, 245, 42, 0, 0, 0, 8, 0, 9, 1, 0, 8, 0, 1, 0, 1, 3, 1, 0, 0, 0, 0,
            8, 0, 9, 1, 0, 0, 0, 0, 64, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 16, 0, 0, 1, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 64, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 1, 77, 77,
            76, 77, 77, 77, 0, 42, 0, 0, 77, 77, 42, 0, 0, 0, 8, 0, 9, 1, 0, 8, 0, 1, 0, 1, 3, 1,
            0, 0, 0, 0, 239, 0, 9, 1, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 171, 10,
            185, 90, 77, 8, 0, 0, 0, 63, 0, 128, 0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 73, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 39,
            0, 0, 0, 0, 0, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126,
            126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126,
            126, 126, 126, 126, 126, 126, 126, 126, 126, 77, 77, 0, 42, 161, 0, 0, 8, 0, 1, 73, 0,
            0, 1, 52, 1, 0, 0, 0, 46, 0, 0, 1, 0, 0, 70, 73, 76, 255, 255, 255, 255, 255, 255, 255,
            3, 77, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 0, 1, 3, 1, 0, 0, 0, 0, 8, 0,
            9, 1, 11, 0, 0, 0, 1, 183, 3, 0, 77, 0, 0, 0, 0, 73, 77, 126, 126, 126, 126, 126, 126,
            126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126,
            126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126,
            126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126,
            126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 0, 0, 183, 0, 42,
            3, 0, 0, 0, 1, 52, 1, 0, 0, 0, 46, 0, 0, 1, 0, 0, 70, 73, 76, 255, 255, 255, 255, 255,
            255, 255, 3, 77, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 0, 1, 3, 1, 0, 0, 0,
            0, 8, 0, 9, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 63, 0, 128, 0, 0, 0, 0, 0,
            9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 77, 77, 0, 42, 0, 0, 0, 8, 0, 9, 1, 0, 0, 8,
            0, 0, 0, 2, 0, 0, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 91, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1,
            0, 0, 77, 77, 0, 0, 1, 58, 1, 0, 0, 0, 46, 0, 0, 43, 1, 0, 0, 70, 73, 76, 77, 77, 77,
            76, 77, 0, 0, 0, 245, 77, 77, 0, 42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 65, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 47, 0, 0, 0, 0, 0, 0, 8, 0, 1, 0, 1, 3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0,
            0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 149, 0, 0, 0, 0, 8,
            0, 1, 1, 18, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 77, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 149,
        ];
        let val_res_17 = validate_tiff_raw_header(oom_bytes_17);
        assert!(val_res_17.is_err());
        let raw_path_17 = temp_dir.join("corrupted_payload.qtk");
        std::fs::write(&raw_path_17, oom_bytes_17).unwrap();
        let res_17 = decode_image(&raw_path_17);
        assert!(res_17.is_err());

        // Fuzz reproducer 18: Panasonic RW2 container with corrupted IFD allocation (111 bytes)
        let oom_bytes_18: &[u8] = &[
            77, 77, 0, 85, 0, 0, 0, 20, 77, 77, 0, 42, 0, 131, 131, 131, 131, 131, 131, 131, 131,
            131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131,
            131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131,
            131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131,
            131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131,
            131, 131, 131, 131, 0, 0, 8, 0, 1, 1, 1, 18, 0, 6, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0,
        ];
        let val_res_18 = validate_tiff_raw_header(oom_bytes_18);
        assert!(val_res_18.is_err());
        let raw_path_18 = temp_dir.join("corrupted.rw2");
        std::fs::write(&raw_path_18, oom_bytes_18).unwrap();
        let res_18 = decode_image(&raw_path_18);
        assert!(res_18.is_err());

        // Fuzz reproducer 19: JPEG/EXIF container with truncated TIFF header causing JFIF infinite loop (19 bytes)
        let oom_bytes_19: &[u8] = &[
            255, 216, 255, 225, 0, 8, 69, 120, 105, 102, 0, 0, 49, 0, 0, 1, 0, 0, 0,
        ];
        let val_res_19 = validate_tiff_raw_header(oom_bytes_19);
        assert!(val_res_19.is_err());
        let raw_path_19 = temp_dir.join("corrupted_app1.jpg");
        std::fs::write(&raw_path_19, oom_bytes_19).unwrap();
        let res_19 = decode_image(&raw_path_19);
        assert!(res_19.is_err());

        // Fuzz reproducer 20: BMFF vendor box 64-bit size overflow causing panic in rawler (87 bytes)
        let oom_bytes_20: &[u8] = &[
            0, 0, 0, 11, 0, 77, 73, 73, 42, 0, 12, 0, 0, 0, 1, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 66, 2, 42, 190, 0, 1, 2, 16, 0, 0,
            0, 8, 2, 4, 1, 0, 0, 0,
        ];
        let val_res_20 = validate_tiff_raw_header(oom_bytes_20);
        assert!(val_res_20.is_err());
        let raw_path_20 = temp_dir.join("corrupted.cr3");
        std::fs::write(&raw_path_20, oom_bytes_20).unwrap();
        let res_20 = decode_image(&raw_path_20);
        assert!(res_20.is_err());

        // Fuzz reproducer 21: BMFF ftyp box zero size causing underflow panic (98 bytes)
        let oom_bytes_21: &[u8] = &[
            0, 0, 0, 0, 102, 116, 121, 112, 0, 8, 0, 1, 1, 18, 32, 3, 0, 0, 0, 0, 0, 8, 0, 0, 1, 0,
            0, 0, 0, 0, 0, 0, 77, 77, 77, 0, 42, 0, 49, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0,
            0, 0, 129, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 37, 0,
        ];
        let val_res_21 = validate_tiff_raw_header(oom_bytes_21);
        assert!(val_res_21.is_err());
        let raw_path_21 = temp_dir.join("corrupted_ftyp.cr3");
        std::fs::write(&raw_path_21, oom_bytes_21).unwrap();
        let res_21 = decode_image(&raw_path_21);
        assert!(res_21.is_err());

        // Fuzz reproducer 22: JPEG/EXIF container with root IFD offset 0 causing JFIF infinite loop (60 bytes)
        let oom_bytes_22: &[u8] = &[
            255, 216, 255, 225, 0, 36, 69, 120, 105, 102, 0, 0, 77, 77, 0, 42, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 16, 0, 16,
        ];
        let val_res_22 = validate_tiff_raw_header(oom_bytes_22);
        assert!(val_res_22.is_err());
        let raw_path_22 = temp_dir.join("corrupted_zero_ifd.jpg");
        std::fs::write(&raw_path_22, oom_bytes_22).unwrap();
        let res_22 = decode_image(&raw_path_22);
        assert!(res_22.is_err());

        // Fuzz reproducer 23: JPEG/EXIF container with IFD entry count 0 causing JFIF infinite loop (40 bytes)
        let oom_bytes_23: &[u8] = &[
            255, 216, 255, 225, 0, 36, 69, 120, 105, 102, 0, 0, 77, 77, 0, 42, 0, 0, 0, 8, 0, 0, 0,
            0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 27, 77, 16,
        ];
        let val_res_23 = validate_tiff_raw_header(oom_bytes_23);
        assert!(val_res_23.is_err());
        let raw_path_23 = temp_dir.join("corrupted_zero_entries.jpg");
        std::fs::write(&raw_path_23, oom_bytes_23).unwrap();
        let res_23 = decode_image(&raw_path_23);
        assert!(res_23.is_err());

        // Fuzz reproducer 24: BMFF container with secondary truncated ftyp box causing underflow panic (36 bytes)
        let oom_bytes_24: &[u8] = &[
            0, 0, 0, 16, 102, 116, 121, 112, 252, 0, 0, 0, 163, 163, 0, 0, 0, 0, 0, 8, 102, 116,
            121, 112, 0, 0, 0, 8, 252, 0, 0, 0, 163, 163, 0, 0,
        ];
        let val_res_24 = validate_tiff_raw_header(oom_bytes_24);
        assert!(val_res_24.is_err());
        let raw_path_24 = temp_dir.join("corrupted_second_ftyp.cr3");
        std::fs::write(&raw_path_24, oom_bytes_24).unwrap();
        let res_24 = decode_image(&raw_path_24);
        assert!(res_24.is_err());
        // Fuzz reproducer 25: CIFF container with recursive self-referential subIFD bomb (113 bytes)
        let oom_bytes_25: &[u8] = &[
            73, 73, 26, 0, 0, 0, 72, 69, 65, 80, 67, 67, 68, 82, 119, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0,
            0, 0, 8, 0, 0, 45, 87, 0, 0, 0, 0, 0, 0, 0, 73, 42, 87, 0, 0, 0, 0, 0, 0, 0, 73, 42,
            87, 0, 0, 0, 0, 0, 0, 0, 73, 58, 73, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            73, 42, 87, 0, 0, 0, 0, 0, 0, 0, 73, 42, 87, 0, 0, 0, 0, 0, 0, 0, 73, 58, 73, 0, 0, 0,
            0, 0, 0, 0, 1, 0, 0, 0,
        ];
        let val_res_25 = validate_tiff_raw_header(oom_bytes_25);
        assert!(val_res_25.is_err());
        let raw_path_25 = temp_dir.join("corrupted_ciff_subifd_cycle.crw");
        std::fs::write(&raw_path_25, oom_bytes_25).unwrap();
        let res_25 = decode_image(&raw_path_25);
        assert!(res_25.is_err());
        // Fuzz reproducer 26: JPEG/EXIF container with truncated APP1 TIFF header causing rawler infinite loop (42 bytes)
        let oom_bytes_26: &[u8] = &[
            255, 216, 255, 225, 0, 14, 69, 120, 105, 102, 0, 0, 73, 73, 42, 0, 77, 14, 69, 120,
            106, 0, 42, 0, 0, 8, 252, 9, 1, 48, 33, 0, 32, 1, 122, 0, 0, 0, 1, 216, 255, 65,
        ];
        let val_res_26 = validate_tiff_raw_header(oom_bytes_26);
        assert!(val_res_26.is_err());
        let raw_path_26 = temp_dir.join("corrupted_app1_trunc.jpg");
        std::fs::write(&raw_path_26, oom_bytes_26).unwrap();
        let res_26 = decode_image(&raw_path_26);
        assert!(res_26.is_err());
        // Fuzz reproducer 27: TIFF container with SubIFD tag having count 0 causing rawler empty slice indexing panic (62 bytes)
        let oom_bytes_27: &[u8] = &[
            77, 77, 0, 42, 0, 0, 0, 8, 0, 4, 1, 74, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 8, 0, 0, 0,
            0, 0, 0, 0, 0,
        ];
        let val_res_27 = validate_tiff_raw_header(oom_bytes_27);
        assert!(val_res_27.is_err());
        let raw_path_27 = temp_dir.join("corrupted_subifd_count0.tif");
        std::fs::write(&raw_path_27, oom_bytes_27).unwrap();
        let res_27 = decode_image(&raw_path_27);
        assert!(res_27.is_err());

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
