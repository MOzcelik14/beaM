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
    /// Temperature offset in Kelvins (-100.0 to +100.0 or 2000K to 12000K, default 0.0)
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

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EditStack {
    pub white_balance: WhiteBalanceAdjustment,
    pub exposure: ExposureAdjustment,
    pub contrast: ContrastAdjustment,
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

    pub fn reset_contrast(&mut self) {
        self.contrast = ContrastAdjustment::default();
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
            .depth_limit(4)
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

        // Placeholders for future version migrations (e.g. version 1 -> 2)
        // while (self.format_version < CURRENT_FORMAT_VERSION) { ... }

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

        let ron_str = original.to_ron_string().expect("Serialization failed");
        assert!(ron_str.contains("format_version: 1"));
        assert!(ron_str.contains("ev: 1.35"));

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
                contrast: (enabled: true, contrast: 0.0),
            )
        )"#;

        let err = Sidecar::from_ron_str(future_ron);
        assert!(err.is_err());
    }
}
