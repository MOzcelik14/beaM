//! Non-destructive sidecar file format for beaM.
//!
//! Stores the full edit stack in a human-readable RON file (`photo.ARW.beam.ron`).

use crate::error::{BeamError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const CURRENT_FORMAT_VERSION: u32 = 1;
pub const SIDECAR_EXTENSION: &str = "beam.ron";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WhiteBalanceAdjustment {
    pub enabled: bool,
    /// Temperature offset in Kelvins (-100.0 to +100.0, default 0.0)
    pub temperature: f32,
    /// Tint offset (-100.0 to +100.0, default 0.0)
    pub tint: f32,
}

impl Default for WhiteBalanceAdjustment {
    fn default() -> Self {
        Self {
            enabled: true,
            temperature: 0.0,
            tint: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExposureAdjustment {
    pub enabled: bool,
    /// Exposure value in EV stops (-5.0 to +5.0, default 0.0)
    pub ev: f32,
}

impl Default for ExposureAdjustment {
    fn default() -> Self {
        Self {
            enabled: true,
            ev: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContrastAdjustment {
    pub enabled: bool,
    /// Contrast adjustment factor (-100.0 to +100.0, default 0.0)
    pub contrast: f32,
}

impl Default for ContrastAdjustment {
    fn default() -> Self {
        Self {
            enabled: true,
            contrast: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HighlightsShadowsAdjustment {
    pub enabled: bool,
    /// Highlights adjustment (-100.0 to +100.0, default 0.0)
    pub highlights: f32,
    /// Shadows adjustment (-100.0 to +100.0, default 0.0)
    pub shadows: f32,
}

impl Default for HighlightsShadowsAdjustment {
    fn default() -> Self {
        Self {
            enabled: true,
            highlights: 0.0,
            shadows: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ToneCurvePoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToneCurveAdjustment {
    pub enabled: bool,
    pub points: Vec<ToneCurvePoint>,
}

impl Default for ToneCurveAdjustment {
    fn default() -> Self {
        Self {
            enabled: true,
            points: vec![
                ToneCurvePoint { x: 0.0, y: 0.0 },
                ToneCurvePoint { x: 0.25, y: 0.25 },
                ToneCurvePoint { x: 0.5, y: 0.5 },
                ToneCurvePoint { x: 0.75, y: 0.75 },
                ToneCurvePoint { x: 1.0, y: 1.0 },
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct HslBand {
    /// Hue shift (-100.0 to +100.0, default 0.0)
    pub hue: f32,
    /// Saturation factor (-100.0 to +100.0, default 0.0)
    pub saturation: f32,
    /// Luminance factor (-100.0 to +100.0, default 0.0)
    pub luminance: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HslAdjustment {
    pub enabled: bool,
    /// 8 bands: [Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta]
    pub bands: [HslBand; 8],
}

impl Default for HslAdjustment {
    fn default() -> Self {
        Self {
            enabled: true,
            bands: [HslBand::default(); 8],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EditStack {
    pub white_balance: WhiteBalanceAdjustment,
    pub exposure: ExposureAdjustment,
    pub highlights_shadows: HighlightsShadowsAdjustment,
    pub contrast: ContrastAdjustment,
    pub tone_curve: ToneCurveAdjustment,
    pub hsl: HslAdjustment,
}

impl EditStack {
    pub fn reset_all(&mut self) {
        *self = Self::default();
    }

    pub fn reset_white_balance(&mut self) {
        self.white_balance = WhiteBalanceAdjustment::default();
    }

    pub fn reset_exposure(&mut self) {
        self.exposure = ExposureAdjustment::default();
    }

    pub fn reset_highlights_shadows(&mut self) {
        self.highlights_shadows = HighlightsShadowsAdjustment::default();
    }

    pub fn reset_contrast(&mut self) {
        self.contrast = ContrastAdjustment::default();
    }

    pub fn reset_tone_curve(&mut self) {
        self.tone_curve = ToneCurveAdjustment::default();
    }

    pub fn reset_hsl(&mut self) {
        self.hsl = HslAdjustment::default();
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sidecar {
    pub format_version: u32,
    pub source_filename: String,
    pub edit_stack: EditStack,
}

impl Sidecar {
    pub fn new(source_filename: impl Into<String>) -> Self {
        Self {
            format_version: CURRENT_FORMAT_VERSION,
            source_filename: source_filename.into(),
            edit_stack: EditStack::default(),
        }
    }

    /// Compute sidecar path next to the target image (e.g. `image.ARW.beam.ron`).
    pub fn path_for(image_path: &Path) -> PathBuf {
        let filename = image_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("image");
        let sidecar_name = format!("{filename}.{SIDECAR_EXTENSION}");
        match image_path.parent() {
            Some(parent) => parent.join(sidecar_name),
            None => PathBuf::from(sidecar_name),
        }
    }

    /// Serialize this sidecar to pretty RON string.
    pub fn to_ron_string(&self) -> Result<String> {
        let pretty_config = ron::ser::PrettyConfig::new()
            .depth_limit(5)
            .separate_tuple_members(true)
            .enumerate_arrays(true);
        ron::ser::to_string_pretty(self, pretty_config).map_err(BeamError::from)
    }

    /// Parse sidecar from RON string and automatically run migrations if necessary.
    pub fn from_ron_str(content: &str) -> Result<Self> {
        let mut sidecar: Sidecar = ron::from_str(content).map_err(BeamError::from)?;
        sidecar.migrate_if_needed()?;
        Ok(sidecar)
    }

    /// Save sidecar to disk next to the given image path or explicit sidecar path.
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let ron_str = self.to_ron_string()?;
        fs::write(path, ron_str)?;
        Ok(())
    }

    /// Load sidecar from disk.
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::from_ron_str(&content)
    }

    /// Schema migration handler for backward compatibility.
    pub fn migrate_if_needed(&mut self) -> Result<()> {
        if self.format_version > CURRENT_FORMAT_VERSION {
            return Err(BeamError::SidecarMigration(format!(
                "Sidecar version {} is newer than supported version {}",
                self.format_version, CURRENT_FORMAT_VERSION
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sidecar_path_generation() {
        let img = Path::new("/photos/landscape/DSC00123.ARW");
        let sidecar_path = Sidecar::path_for(img);
        assert_eq!(
            sidecar_path,
            PathBuf::from("/photos/landscape/DSC00123.ARW.beam.ron")
        );
    }

    #[test]
    fn test_sidecar_roundtrip_ron() {
        let mut original = Sidecar::new("DSC00123.ARW");
        original.edit_stack.exposure.ev = 1.35;
        original.edit_stack.white_balance.temperature = 12.0;
        original.edit_stack.contrast.contrast = 25.0;
        original.edit_stack.highlights_shadows.highlights = -30.0;
        original.edit_stack.highlights_shadows.shadows = 40.0;
        original.edit_stack.hsl.bands[5].saturation = 20.0; // Boost blue saturation

        let ron_str = original.to_ron_string().expect("Serialization failed");
        assert!(ron_str.contains("format_version: 1"));
        assert!(ron_str.contains("highlights: -30"));

        let loaded = Sidecar::from_ron_str(&ron_str).expect("Deserialization failed");
        assert_eq!(original, loaded);
    }

    #[test]
    fn test_reject_future_version() {
        let future_ron = r#"(
            format_version: 999,
            source_filename: "photo.ARW",
            edit_stack: (
                white_balance: (enabled: true, temperature: 0.0, tint: 0.0),
                exposure: (enabled: true, ev: 0.0),
                highlights_shadows: (enabled: true, highlights: 0.0, shadows: 0.0),
                contrast: (enabled: true, contrast: 0.0),
                tone_curve: (enabled: true, points: []),
                hsl: (enabled: true, bands: [
                    (hue: 0.0, saturation: 0.0, luminance: 0.0),
                    (hue: 0.0, saturation: 0.0, luminance: 0.0),
                    (hue: 0.0, saturation: 0.0, luminance: 0.0),
                    (hue: 0.0, saturation: 0.0, luminance: 0.0),
                    (hue: 0.0, saturation: 0.0, luminance: 0.0),
                    (hue: 0.0, saturation: 0.0, luminance: 0.0),
                    (hue: 0.0, saturation: 0.0, luminance: 0.0),
                    (hue: 0.0, saturation: 0.0, luminance: 0.0),
                ]),
            )
        )"#;

        let err = Sidecar::from_ron_str(future_ron);
        assert!(err.is_err());
    }
}
