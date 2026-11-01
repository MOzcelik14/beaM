//! Non-destructive sidecar file format for beaM.
//!
//! Stores the full edit stack in a human-readable RON file (`photo.ARW.beam.ron`).

use crate::error::{BeamError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const CURRENT_FORMAT_VERSION: u32 = 1;
pub const SIDECAR_EXTENSION: &str = "beam";
pub const LEGACY_SIDECAR_EXTENSION: &str = "beam.ron";
pub const MAX_SIDECAR_FILE_SIZE: u64 = 5 * 1024 * 1024;

/// Alias representing a standalone beaM project file.
pub type BeamProject = Sidecar;
/// Alias representing a standalone beaM project file.
pub type Project = Sidecar;

#[inline]
fn sanitize_float(val: &mut f32, min: f32, max: f32, default: f32) {
    if !val.is_finite() {
        *val = default;
    } else {
        *val = val.clamp(min, max);
    }
}

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

/// Presets for crop aspect ratios.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CropAspectPreset {
    #[default]
    Original,
    Square,    // 1:1
    Ratio4x5,  // 4:5
    Ratio3x2,  // 3:2
    Ratio16x9, // 16:9
    Free,      // Unconstrained
}

impl CropAspectPreset {
    /// Return the target width / height aspect ratio, or None if Free.
    pub fn ratio(&self, original_aspect: f32) -> Option<f32> {
        match self {
            Self::Original => Some(original_aspect.max(0.001)),
            Self::Square => Some(1.0),
            Self::Ratio4x5 => {
                if original_aspect >= 1.0 {
                    Some(5.0 / 4.0)
                } else {
                    Some(4.0 / 5.0)
                }
            }
            Self::Ratio3x2 => {
                if original_aspect >= 1.0 {
                    Some(3.0 / 2.0)
                } else {
                    Some(2.0 / 3.0)
                }
            }
            Self::Ratio16x9 => {
                if original_aspect >= 1.0 {
                    Some(16.0 / 9.0)
                } else {
                    Some(9.0 / 16.0)
                }
            }
            Self::Free => None,
        }
    }
}

/// Non-destructive crop, rotate, flip, and straighten adjustments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CropAdjustment {
    pub enabled: bool,
    /// Left offset in normalized [0.0, 1.0] image space
    pub x: f32,
    /// Top offset in normalized [0.0, 1.0] image space
    pub y: f32,
    /// Width in normalized [0.0, 1.0] image space
    pub width: f32,
    /// Height in normalized [0.0, 1.0] image space
    pub height: f32,
    /// Active aspect ratio preset
    pub preset: CropAspectPreset,
    /// Clockwise rotation in degrees: 0, 90, 180, 270
    pub rotation: u32,
    /// Horizontal flip (mirror across Y axis)
    pub flip_h: bool,
    /// Vertical flip (mirror across X axis)
    pub flip_v: bool,
    /// Straighten angle in degrees: -45.0..=45.0
    pub straighten: f32,
}

impl Default for CropAdjustment {
    fn default() -> Self {
        Self {
            enabled: false,
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
            preset: CropAspectPreset::Original,
            rotation: 0,
            flip_h: false,
            flip_v: false,
            straighten: 0.0,
        }
    }
}

impl CropAdjustment {
    /// Returns true if any geometric transformation (crop, rotation, flip, or straighten) is applied.
    pub fn is_active(&self) -> bool {
        self.enabled
            || self.rotation != 0
            || self.flip_h
            || self.flip_v
            || self.straighten.abs() > 0.001
            || (self.x > 0.0001 || self.y > 0.0001 || self.width < 0.9999 || self.height < 0.9999)
    }

    /// Reset crop rectangle to full bounds (0, 0, 1, 1).
    pub fn reset_crop_rect(&mut self) {
        self.x = 0.0;
        self.y = 0.0;
        self.width = 1.0;
        self.height = 1.0;
        self.enabled = false;
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
    #[serde(default)]
    pub crop: CropAdjustment,
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

    pub fn reset_crop(&mut self) {
        self.crop = CropAdjustment::default();
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Validates and sanitizes all numerical and geometric values in the edit stack.
    /// Clamps or replaces non-finite (NaN, +/-Inf) values and enforces valid geometric boundaries.
    pub fn validate_and_sanitize(&mut self) {
        // 1. White balance
        sanitize_float(&mut self.white_balance.temperature, -100.0, 100.0, 0.0);
        sanitize_float(&mut self.white_balance.tint, -100.0, 100.0, 0.0);

        // 2. Exposure
        sanitize_float(&mut self.exposure.ev, -5.0, 5.0, 0.0);

        // 3. Highlights & Shadows
        sanitize_float(&mut self.highlights_shadows.highlights, -100.0, 100.0, 0.0);
        sanitize_float(&mut self.highlights_shadows.shadows, -100.0, 100.0, 0.0);

        // 4. Contrast
        sanitize_float(&mut self.contrast.contrast, -100.0, 100.0, 0.0);

        // 5. Tone curve
        self.tone_curve.points.retain(|p| p.x.is_finite() && p.y.is_finite());
        for p in &mut self.tone_curve.points {
            p.x = p.x.clamp(0.0, 1.0);
            p.y = p.y.clamp(0.0, 1.0);
        }
        self.tone_curve
            .points
            .sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
        if self.tone_curve.points.len() < 2 {
            self.tone_curve.points = ToneCurveAdjustment::default().points;
        }

        // 6. HSL bands
        for band in &mut self.hsl.bands {
            sanitize_float(&mut band.hue, -100.0, 100.0, 0.0);
            sanitize_float(&mut band.saturation, -100.0, 100.0, 0.0);
            sanitize_float(&mut band.luminance, -100.0, 100.0, 0.0);
        }

        // 7. Crop & Geometry
        sanitize_float(&mut self.crop.width, 0.001, 1.0, 1.0);
        sanitize_float(&mut self.crop.height, 0.001, 1.0, 1.0);
        sanitize_float(&mut self.crop.x, 0.0, 1.0 - self.crop.width, 0.0);
        sanitize_float(&mut self.crop.y, 0.0, 1.0 - self.crop.height, 0.0);
        self.crop.rotation = ((self.crop.rotation % 360) / 90) * 90;
        sanitize_float(&mut self.crop.straighten, -45.0, 45.0, 0.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorLabel {
    #[default]
    None,
    Red,
    Yellow,
    Green,
    Blue,
    Purple,
}

impl ColorLabel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Red => "red",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Purple => "purple",
        }
    }

    pub fn translation_key(&self) -> &'static str {
        match self {
            Self::None => "library_color_none",
            Self::Red => "library_color_red",
            Self::Yellow => "library_color_yellow",
            Self::Green => "library_color_green",
            Self::Blue => "library_color_blue",
            Self::Purple => "library_color_purple",
        }
    }

    pub fn from_str_name(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "red" => Self::Red,
            "yellow" => Self::Yellow,
            "green" => Self::Green,
            "blue" => Self::Blue,
            "purple" => Self::Purple,
            _ => Self::None,
        }
    }
}

pub const MAX_HISTORY_STEPS: usize = 50;

/// A single recorded snapshot in the non-destructive edit history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryStep {
    pub name: String,
    pub stack: EditStack,
    #[serde(default)]
    pub timestamp: u64,
}

impl HistoryStep {
    pub fn new(name: impl Into<String>, stack: EditStack) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            name: name.into(),
            stack,
            timestamp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sidecar {
    pub format_version: u32,
    pub source_filename: String,
    #[serde(default)]
    pub rating: u8,
    #[serde(default)]
    pub color_label: ColorLabel,
    pub edit_stack: EditStack,
    #[serde(default)]
    pub history: Vec<HistoryStep>,
    #[serde(default)]
    pub history_index: usize,
}

impl Sidecar {
    pub fn new(source_filename: impl Into<String>) -> Self {
        let default_stack = EditStack::default();
        Self {
            format_version: CURRENT_FORMAT_VERSION,
            source_filename: source_filename.into(),
            rating: 0,
            color_label: ColorLabel::None,
            edit_stack: default_stack.clone(),
            history: vec![HistoryStep::new("Initial", default_stack)],
            history_index: 0,
        }
    }

    pub fn set_rating(&mut self, rating: u8) {
        self.rating = rating.min(5);
    }

    pub fn set_color_label(&mut self, label: ColorLabel) {
        self.color_label = label;
    }

    /// Push a new edit step into the history stack.
    /// If user previously undid steps, prunes any forward redo entries.
    /// Caps the history at `MAX_HISTORY_STEPS` while retaining the Initial state at index 0.
    pub fn push_history_step(&mut self, name: impl Into<String>, stack: EditStack) {
        if self.history.is_empty() {
            self.history
                .push(HistoryStep::new("Initial", stack.clone()));
            self.history_index = 0;
            self.edit_stack = stack;
            return;
        }

        // Branching: prune redo history if not at the head
        if self.history_index + 1 < self.history.len() {
            self.history.truncate(self.history_index + 1);
        }

        self.history.push(HistoryStep::new(name, stack.clone()));
        self.history_index = self.history.len() - 1;
        self.edit_stack = stack;

        // Cap size while preserving initial baseline at index 0
        if self.history.len() > MAX_HISTORY_STEPS {
            self.history.remove(1);
            self.history_index = self.history.len() - 1;
        }
    }

    /// Undo to previous history step if available.
    pub fn undo(&mut self) -> Option<&EditStack> {
        if self.history_index > 0 && !self.history.is_empty() {
            self.history_index -= 1;
            let stack = self.history[self.history_index].stack.clone();
            self.edit_stack = stack;
            Some(&self.edit_stack)
        } else {
            None
        }
    }

    /// Redo to next history step if available.
    pub fn redo(&mut self) -> Option<&EditStack> {
        if self.history_index + 1 < self.history.len() {
            self.history_index += 1;
            let stack = self.history[self.history_index].stack.clone();
            self.edit_stack = stack;
            Some(&self.edit_stack)
        } else {
            None
        }
    }

    /// Jump directly to a step by index.
    pub fn jump_to_history_step(&mut self, index: usize) -> Option<&EditStack> {
        if index < self.history.len() {
            self.history_index = index;
            let stack = self.history[index].stack.clone();
            self.edit_stack = stack;
            Some(&self.edit_stack)
        } else {
            None
        }
    }

    /// Compute primary sidecar path next to the target image (e.g. `image.ARW.beam`).
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

    /// Search for an existing sidecar file on disk, checking `.beam` first,
    /// then legacy `.beam.ron`.
    pub fn find_sidecar_for(image_path: &Path) -> Option<PathBuf> {
        let parent = image_path.parent();
        let filename = image_path.file_name()?.to_str()?;

        // 1. Primary: filename.beam (e.g. DSC00123.ARW.beam)
        let p1 = match parent {
            Some(dir) => dir.join(format!("{filename}.{SIDECAR_EXTENSION}")),
            None => PathBuf::from(format!("{filename}.{SIDECAR_EXTENSION}")),
        };
        if p1.is_file() {
            return Some(p1);
        }

        // 2. Alternative: stem.beam (e.g. DSC00123.beam)
        if let Some(stem) = image_path.file_stem().and_then(|s| s.to_str()) {
            let p2 = match parent {
                Some(dir) => dir.join(format!("{stem}.{SIDECAR_EXTENSION}")),
                None => PathBuf::from(format!("{stem}.{SIDECAR_EXTENSION}")),
            };
            if p2.is_file() {
                return Some(p2);
            }
        }

        // 3. Legacy: filename.beam.ron (e.g. DSC00123.ARW.beam.ron)
        let p3 = match parent {
            Some(dir) => dir.join(format!("{filename}.{LEGACY_SIDECAR_EXTENSION}")),
            None => PathBuf::from(format!("{filename}.{LEGACY_SIDECAR_EXTENSION}")),
        };
        if p3.is_file() {
            return Some(p3);
        }

        // 4. Legacy alternative: stem.beam.ron (e.g. DSC00123.beam.ron)
        if let Some(stem) = image_path.file_stem().and_then(|s| s.to_str()) {
            let p4 = match parent {
                Some(dir) => dir.join(format!("{stem}.{LEGACY_SIDECAR_EXTENSION}")),
                None => PathBuf::from(format!("{stem}.{LEGACY_SIDECAR_EXTENSION}")),
            };
            if p4.is_file() {
                return Some(p4);
            }
        }

        None
    }

    /// Serialize this sidecar to pretty RON string.
    pub fn to_ron_string(&self) -> Result<String> {
        let pretty_config = ron::ser::PrettyConfig::new()
            .depth_limit(5)
            .separate_tuple_members(true)
            .enumerate_arrays(true);
        ron::ser::to_string_pretty(self, pretty_config).map_err(BeamError::from)
    }

    /// Validates and sanitizes sidecar data, bounding history length and clamping all parameters.
    pub fn validate_and_sanitize(&mut self) {
        self.rating = self.rating.min(5);
        self.edit_stack.validate_and_sanitize();

        if self.history.is_empty() {
            self.history
                .push(HistoryStep::new("Initial", self.edit_stack.clone()));
            self.history_index = 0;
        } else {
            if self.history.len() > MAX_HISTORY_STEPS {
                let excess = self.history.len() - MAX_HISTORY_STEPS;
                // Preserve initial step at 0, drain excess from index 1
                self.history.drain(1..=excess);
            }
            for step in &mut self.history {
                step.stack.validate_and_sanitize();
            }
            if self.history_index >= self.history.len() {
                self.history_index = self.history.len() - 1;
            }
        }
    }

    /// Parse sidecar from RON string and automatically run migrations if necessary.
    pub fn from_ron_str(content: &str) -> Result<Self> {
        if content.len() > MAX_SIDECAR_FILE_SIZE as usize {
            return Err(BeamError::Sidecar(format!(
                "Sidecar content length ({} bytes) exceeds limit of {MAX_SIDECAR_FILE_SIZE} bytes",
                content.len()
            )));
        }
        let mut sidecar: Sidecar = ron::from_str(content).map_err(BeamError::from)?;
        if sidecar.history.is_empty() {
            let initial = sidecar.edit_stack.clone();
            sidecar.history.push(HistoryStep::new("Initial", initial));
            sidecar.history_index = 0;
        }
        sidecar.migrate_if_needed()?;
        sidecar.validate_and_sanitize();
        Ok(sidecar)
    }

    /// Save sidecar to disk next to the given image path or explicit sidecar path atomically.
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let ron_str = self.to_ron_string()?;
        crate::fs_util::atomic_write(path, ron_str.as_bytes())?;
        Ok(())
    }

    /// Load sidecar from disk with safety bounds on file size.
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let metadata = fs::metadata(path)?;
        if metadata.len() > MAX_SIDECAR_FILE_SIZE {
            return Err(BeamError::Sidecar(format!(
                "Sidecar file {} exceeds maximum allowed size of {MAX_SIDECAR_FILE_SIZE} bytes",
                path.display()
            )));
        }
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
            PathBuf::from("/photos/landscape/DSC00123.ARW.beam")
        );
    }

    #[test]
    fn test_find_sidecar_backward_compatibility() {
        let temp_dir = std::env::temp_dir().join("beam_test_sidecar_find");
        let _ = std::fs::create_dir_all(&temp_dir);

        let img_path = temp_dir.join("photo.arw");
        let _ = std::fs::write(&img_path, b"test image content");

        // 1. When no sidecar exists
        assert_eq!(Sidecar::find_sidecar_for(&img_path), None);

        // 2. When legacy .beam.ron exists
        let legacy_p = temp_dir.join("photo.arw.beam.ron");
        let _ = std::fs::write(&legacy_p, b"legacy sidecar");
        assert_eq!(Sidecar::find_sidecar_for(&img_path), Some(legacy_p.clone()));

        // 3. When new .beam exists, it takes precedence over legacy
        let modern_p = temp_dir.join("photo.arw.beam");
        let _ = std::fs::write(&modern_p, b"modern sidecar");
        assert_eq!(Sidecar::find_sidecar_for(&img_path), Some(modern_p.clone()));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_beam_project_standalone_save_and_load() {
        let temp_dir = std::env::temp_dir().join("beam_test_project_save");
        let _ = std::fs::create_dir_all(&temp_dir);

        let project_path = temp_dir.join("custom_landscape.beam");
        let mut project = BeamProject::new("mountain.ARW");
        project.set_rating(5);
        project.set_color_label(ColorLabel::Purple);
        project.edit_stack.exposure.ev = 2.0;
        project.edit_stack.contrast.contrast = 35.0;
        project.push_history_step("Boost exposure", project.edit_stack.clone());

        project
            .save_to_file(&project_path)
            .expect("Failed to save .beam project");
        assert!(project_path.exists());

        let loaded =
            BeamProject::load_from_file(&project_path).expect("Failed to load .beam project");
        assert_eq!(loaded.source_filename, "mountain.ARW");
        assert_eq!(loaded.rating, 5);
        assert_eq!(loaded.color_label, ColorLabel::Purple);
        assert_eq!(loaded.edit_stack.exposure.ev, 2.0);
        assert_eq!(loaded.edit_stack.contrast.contrast, 35.0);
        assert_eq!(loaded.history.len(), 2);
        assert_eq!(loaded.history[1].name, "Boost exposure");

        let _ = std::fs::remove_dir_all(&temp_dir);
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

    #[test]
    fn test_sidecar_rating_and_color_label() {
        let mut sidecar = Sidecar::new("photo.jpg");
        sidecar.set_rating(4);
        sidecar.set_color_label(ColorLabel::Yellow);

        let ron_str = sidecar.to_ron_string().expect("Serialization failed");
        assert!(ron_str.contains("rating: 4"));
        assert!(ron_str.contains("color_label: yellow"));

        let loaded = Sidecar::from_ron_str(&ron_str).expect("Deserialization failed");
        assert_eq!(loaded.rating, 4);
        assert_eq!(loaded.color_label, ColorLabel::Yellow);
    }

    #[test]
    fn test_sidecar_backwards_compatibility_without_rating() {
        let sidecar = Sidecar::new("legacy.ARW");
        let ron_str = sidecar.to_ron_string().unwrap();
        // Strip out rating and color_label lines to simulate older sidecar files
        let legacy_ron = ron_str
            .lines()
            .filter(|line| !line.contains("rating:") && !line.contains("color_label:"))
            .collect::<Vec<_>>()
            .join("\n");

        let loaded = Sidecar::from_ron_str(&legacy_ron).expect("Should parse legacy sidecar");
        assert_eq!(loaded.rating, 0);
        assert_eq!(loaded.color_label, ColorLabel::None);
    }

    #[test]
    fn test_history_push_undo_redo_jump() {
        let mut sidecar = Sidecar::new("sample.ARW");
        assert_eq!(sidecar.history.len(), 1);
        assert_eq!(sidecar.history_index, 0);
        assert_eq!(sidecar.history[0].name, "Initial");

        // Push step 1: Exposure
        let mut stack1 = sidecar.edit_stack.clone();
        stack1.exposure.ev = 1.0;
        sidecar.push_history_step("Exposure: +1.00 EV", stack1);
        assert_eq!(sidecar.history.len(), 2);
        assert_eq!(sidecar.history_index, 1);
        assert_eq!(sidecar.edit_stack.exposure.ev, 1.0);

        // Push step 2: Contrast
        let mut stack2 = sidecar.edit_stack.clone();
        stack2.contrast.contrast = 20.0;
        sidecar.push_history_step("Contrast: +20", stack2);
        assert_eq!(sidecar.history.len(), 3);
        assert_eq!(sidecar.history_index, 2);

        // Undo 1: Back to Exposure
        assert!(sidecar.undo().is_some());
        assert_eq!(sidecar.history_index, 1);
        assert_eq!(sidecar.edit_stack.exposure.ev, 1.0);
        assert_eq!(sidecar.edit_stack.contrast.contrast, 0.0);

        // Undo 2: Back to Initial
        assert!(sidecar.undo().is_some());
        assert_eq!(sidecar.history_index, 0);
        assert_eq!(sidecar.edit_stack.exposure.ev, 0.0);

        // Undo 3: No-op at index 0
        assert!(sidecar.undo().is_none());
        assert_eq!(sidecar.history_index, 0);

        // Redo 1: Back to Exposure
        assert!(sidecar.redo().is_some());
        assert_eq!(sidecar.history_index, 1);
        assert_eq!(sidecar.edit_stack.exposure.ev, 1.0);

        // Branching: While at index 1, push a new edit (White Balance)
        let mut stack3 = sidecar.edit_stack.clone();
        stack3.white_balance.temperature = 15.0;
        sidecar.push_history_step("WB Temp: +15", stack3);
        assert_eq!(sidecar.history.len(), 3);
        assert_eq!(sidecar.history_index, 2);
        assert_eq!(sidecar.history[2].name, "WB Temp: +15");
        // Contrast step should have been pruned by branching
        assert!(sidecar.redo().is_none());

        // Jump to Initial (index 0)
        assert!(sidecar.jump_to_history_step(0).is_some());
        assert_eq!(sidecar.history_index, 0);
        assert_eq!(sidecar.edit_stack.exposure.ev, 0.0);
        assert_eq!(sidecar.edit_stack.white_balance.temperature, 0.0);

        // Jump to Step 2
        assert!(sidecar.jump_to_history_step(2).is_some());
        assert_eq!(sidecar.history_index, 2);
        assert_eq!(sidecar.edit_stack.white_balance.temperature, 15.0);

        // Roundtrip to RON preserves history
        let ron_str = sidecar.to_ron_string().expect("Serialization failed");
        assert!(ron_str.contains("WB Temp: +15"));
        let loaded = Sidecar::from_ron_str(&ron_str).expect("Deserialization failed");
        assert_eq!(loaded.history.len(), 3);
        assert_eq!(loaded.history_index, 2);
        assert_eq!(loaded.edit_stack.white_balance.temperature, 15.0);
    }

    #[test]
    fn test_history_capping_at_max() {
        let mut sidecar = Sidecar::new("sample.ARW");
        for i in 1..=60 {
            let mut stack = sidecar.edit_stack.clone();
            stack.exposure.ev = i as f32;
            sidecar.push_history_step(format!("Step {i}"), stack);
        }

        assert_eq!(sidecar.history.len(), MAX_HISTORY_STEPS);
        assert_eq!(sidecar.history[0].name, "Initial");
        assert_eq!(sidecar.history_index, MAX_HISTORY_STEPS - 1);
        assert_eq!(sidecar.history.last().unwrap().name, "Step 60");
    }

    #[test]
    fn test_crop_sidecar_roundtrip_and_backward_compatibility() {
        let mut sidecar = Sidecar::new("crop_test.ARW");
        sidecar.edit_stack.crop.enabled = true;
        sidecar.edit_stack.crop.x = 0.1;
        sidecar.edit_stack.crop.y = 0.2;
        sidecar.edit_stack.crop.width = 0.7;
        sidecar.edit_stack.crop.height = 0.6;
        sidecar.edit_stack.crop.rotation = 90;
        sidecar.edit_stack.crop.flip_h = true;
        sidecar.edit_stack.crop.straighten = 2.5;
        sidecar.edit_stack.crop.preset = CropAspectPreset::Square;

        let ron_str = sidecar.to_ron_string().expect("Serialization failed");
        assert!(ron_str.contains("rotation: 90"));
        assert!(ron_str.contains("flip_h: true"));

        let loaded = Sidecar::from_ron_str(&ron_str).expect("Deserialization failed");
        assert!(loaded.edit_stack.crop.enabled);
        assert_eq!(loaded.edit_stack.crop.rotation, 90);
        assert!(loaded.edit_stack.crop.flip_h);
        assert!(!loaded.edit_stack.crop.flip_v);
        assert_eq!(loaded.edit_stack.crop.straighten, 2.5);
        assert_eq!(loaded.edit_stack.crop.preset, CropAspectPreset::Square);

        // Backward compatibility: sidecar without `crop`
        let base_sidecar = Sidecar::new("legacy.ARW");
        let base_ron = base_sidecar.to_ron_string().unwrap();
        let mut in_crop = false;
        let mut legacy_lines = Vec::new();
        for line in base_ron.lines() {
            if line.trim_start().starts_with("crop:") {
                in_crop = true;
                continue;
            }
            if in_crop {
                if line.trim() == ")," {
                    in_crop = false;
                }
                continue;
            }
            legacy_lines.push(line);
        }
        let legacy_ron = legacy_lines.join("\n");
        let legacy_loaded =
            Sidecar::from_ron_str(&legacy_ron).expect("Legacy deserialization failed");
        assert_eq!(legacy_loaded.edit_stack.crop, CropAdjustment::default());
        assert!(!legacy_loaded.edit_stack.crop.enabled);
    }

    #[test]
    fn test_edit_stack_equality_for_unsaved_changes_detection() {
        let saved = EditStack::default();
        let mut current = EditStack::default();
        assert_eq!(saved, current);

        // Modify exposure
        current.exposure.ev = 0.5;
        assert_ne!(saved, current);

        // Reset back to saved value
        current.exposure.ev = 0.0;
        assert_eq!(saved, current);

        // Modify crop
        current.crop.rotation = 90;
        assert_ne!(saved, current);

        // Reset all
        current.reset_all();
        assert_eq!(saved, current);
    }

    #[test]
    fn test_sidecar_nan_inf_sanitization() {
        let mut stack = EditStack {
            exposure: ExposureAdjustment {
                enabled: true,
                ev: f32::NAN,
            },
            white_balance: WhiteBalanceAdjustment {
                enabled: true,
                temperature: f32::INFINITY,
                tint: f32::NEG_INFINITY,
            },
            crop: CropAdjustment {
                enabled: true,
                x: -1.0,
                y: 2.0,
                width: 5.0,
                height: 0.0,
                rotation: 100,
                straighten: 999.0,
                ..Default::default()
            },
            ..Default::default()
        };

        stack.validate_and_sanitize();

        assert_eq!(stack.exposure.ev, 0.0, "NaN EV must reset to 0.0");
        assert_eq!(stack.white_balance.temperature, 0.0, "Inf temp must reset to 0.0");
        assert_eq!(stack.white_balance.tint, 0.0, "-Inf tint must reset to 0.0");
        assert_eq!(stack.crop.x, 0.0);
        assert!((stack.crop.y + stack.crop.height - 1.0).abs() < 1e-4);
        assert!(stack.crop.width <= 1.0);
        assert!(stack.crop.height >= 0.001);
        assert_eq!(stack.crop.rotation, 90, "Rotation must normalize to multiples of 90");
        assert_eq!(stack.crop.straighten, 45.0, "Straighten must clamp to 45.0");
    }

    #[test]
    fn test_sidecar_oversized_file_rejection() {
        let oversized = " ".repeat((MAX_SIDECAR_FILE_SIZE + 100) as usize);
        let res = Sidecar::from_ron_str(&oversized);
        assert!(res.is_err(), "Must reject oversized sidecar content");
    }

    #[test]
    fn test_atomic_sidecar_save() {
        let temp_dir = std::env::temp_dir().join("beam_atomic_sidecar_test");
        let _ = fs::remove_dir_all(&temp_dir);
        let _ = fs::create_dir_all(&temp_dir);
        let sidecar_p = temp_dir.join("photo.beam");

        let sidecar = Sidecar::new("photo.jpg");
        sidecar.save_to_file(&sidecar_p).expect("Must save atomically");

        assert!(sidecar_p.is_file());
        let loaded = Sidecar::load_from_file(&sidecar_p).expect("Must load cleanly");
        assert_eq!(loaded.source_filename, "photo.jpg");

        // Verify no orphaned .tmp files in temp_dir
        for entry in fs::read_dir(&temp_dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            assert!(!name.ends_with(".tmp"), "No temporary files should linger: {name}");
        }

        let _ = fs::remove_dir_all(temp_dir);
    }

    proptest::proptest! {
        #[test]
        fn prop_crop_geometry_always_valid(
            x in proptest::num::f32::ANY,
            y in proptest::num::f32::ANY,
            w in proptest::num::f32::ANY,
            h in proptest::num::f32::ANY,
            rotation in proptest::num::u32::ANY,
            straighten in proptest::num::f32::ANY,
        ) {
            let mut stack = EditStack {
                crop: CropAdjustment {
                    enabled: true,
                    x,
                    y,
                    width: w,
                    height: h,
                    rotation,
                    straighten,
                    ..Default::default()
                },
                ..Default::default()
            };

            stack.validate_and_sanitize();

            proptest::prop_assert!(stack.crop.x.is_finite() && stack.crop.x >= 0.0 && stack.crop.x <= 1.0);
            proptest::prop_assert!(stack.crop.y.is_finite() && stack.crop.y >= 0.0 && stack.crop.y <= 1.0);
            proptest::prop_assert!(stack.crop.width.is_finite() && stack.crop.width >= 0.001 && stack.crop.width <= 1.0);
            proptest::prop_assert!(stack.crop.height.is_finite() && stack.crop.height >= 0.001 && stack.crop.height <= 1.0);
            proptest::prop_assert!(stack.crop.x + stack.crop.width <= 1.0001);
            proptest::prop_assert!(stack.crop.y + stack.crop.height <= 1.0001);
            proptest::prop_assert!(stack.crop.rotation % 90 == 0);
            proptest::prop_assert!(stack.crop.straighten.is_finite() && stack.crop.straighten >= -45.0 && stack.crop.straighten <= 45.0);
        }
    }
}
