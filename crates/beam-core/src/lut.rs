//! Safe parser for industry-standard `.cube` 3D LUT files.

use crate::error::{BeamError, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Maximum supported 3D LUT grid dimension (65x65x65 = 274,625 entries).
pub const MAX_LUT_SIZE: usize = 65;

/// Parsed 3D Look-Up Table in RGBA32F layout ready for GPU 3D texture upload.
#[derive(Debug, Clone)]
pub struct CubeLut3D {
    pub title: Option<String>,
    pub size: usize,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    /// Flat RGBA32F data of length `size * size * size * 4`.
    pub data: Vec<f32>,
}

/// Parses a `.cube` 3D LUT file from disk with strict bounds and validation.
pub fn parse_cube_file(path: &Path) -> Result<CubeLut3D> {
    let file = File::open(path).map_err(|e| {
        BeamError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to open LUT file {}: {e}", path.display()),
        ))
    })?;

    let metadata = file.metadata()?;
    if metadata.len() > 20 * 1024 * 1024 {
        return Err(BeamError::Sidecar(
            "LUT file exceeds 20 MB maximum size limit".into(),
        ));
    }

    let reader = BufReader::new(file);
    let mut title = None;
    let mut lut_size: Option<usize> = None;
    let mut domain_min = [0.0f32; 3];
    let mut domain_max = [1.0f32; 3];
    let mut entries: Vec<f32> = Vec::new();

    for line_res in reader.lines() {
        let line = line_res?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("TITLE") {
            let t = rest.trim().trim_matches('"').to_string();
            if !t.is_empty() {
                title = Some(t);
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("LUT_3D_SIZE") {
            let sz: usize = rest
                .trim()
                .parse()
                .map_err(|_| BeamError::Sidecar("Invalid LUT_3D_SIZE value".into()))?;
            if !(2..=MAX_LUT_SIZE).contains(&sz) {
                return Err(BeamError::Sidecar(format!(
                    "Unsupported LUT_3D_SIZE {sz}; expected 2..={MAX_LUT_SIZE}"
                )));
            }
            lut_size = Some(sz);
            entries.reserve(sz * sz * sz * 4);
            continue;
        }

        if trimmed.starts_with("LUT_1D_SIZE") {
            return Err(BeamError::Sidecar(
                "1D .cube LUTs are not supported; please use a 3D .cube LUT".into(),
            ));
        }

        if let Some(rest) = trimmed.strip_prefix("DOMAIN_MIN") {
            let vals = parse_vec3(rest)?;
            domain_min = vals;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("DOMAIN_MAX") {
            let vals = parse_vec3(rest)?;
            domain_max = vals;
            continue;
        }

        // Data triplet: R G B
        let rgb = parse_vec3(trimmed)?;
        entries.push(rgb[0].clamp(0.0, 2.0));
        entries.push(rgb[1].clamp(0.0, 2.0));
        entries.push(rgb[2].clamp(0.0, 2.0));
        entries.push(1.0);
    }

    let size = lut_size
        .ok_or_else(|| BeamError::Sidecar("Missing LUT_3D_SIZE header in .cube file".into()))?;

    let expected_floats = size * size * size * 4;
    if entries.len() != expected_floats {
        return Err(BeamError::Sidecar(format!(
            "LUT entry count mismatch: expected {} RGB triplets, got {}",
            size * size * size,
            entries.len() / 4
        )));
    }

    Ok(CubeLut3D {
        title,
        size,
        domain_min,
        domain_max,
        data: entries,
    })
}

fn parse_vec3(s: &str) -> Result<[f32; 3]> {
    let mut parts = s.split_whitespace();
    let r: f32 = parts
        .next()
        .and_then(|v| v.parse().ok())
        .filter(|v: &f32| v.is_finite())
        .ok_or_else(|| BeamError::Sidecar("Invalid float in .cube LUT".into()))?;
    let g: f32 = parts
        .next()
        .and_then(|v| v.parse().ok())
        .filter(|v: &f32| v.is_finite())
        .ok_or_else(|| BeamError::Sidecar("Invalid float in .cube LUT".into()))?;
    let b: f32 = parts
        .next()
        .and_then(|v| v.parse().ok())
        .filter(|v: &f32| v.is_finite())
        .ok_or_else(|| BeamError::Sidecar("Invalid float in .cube LUT".into()))?;
    Ok([r, g, b])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_parse_identity_cube_lut() {
        let dir = std::env::temp_dir().join("beam_cube_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("identity.cube");

        let mut f = File::create(&path).unwrap();
        writeln!(f, "TITLE \"Identity 2x2x2\"").unwrap();
        writeln!(f, "LUT_3D_SIZE 2").unwrap();
        for b in [0.0, 1.0] {
            for g in [0.0, 1.0] {
                for r in [0.0, 1.0] {
                    writeln!(f, "{r:.1} {g:.1} {b:.1}").unwrap();
                }
            }
        }

        let lut = parse_cube_file(&path).expect("Should parse valid 2x2x2 .cube LUT");
        assert_eq!(lut.size, 2);
        assert_eq!(lut.title.as_deref(), Some("Identity 2x2x2"));
        assert_eq!(lut.data.len(), 2 * 2 * 2 * 4);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
