//! Preset management and selective adjustment grouping for beaM.
//!
//! Stores develop presets as human-readable RON files with selective group inclusion
//! (white balance, exposure, tone curve, HSL, crop).

use crate::sidecar::{
    ContrastAdjustment, CropAdjustment, EditStack, ExposureAdjustment, HighlightsShadowsAdjustment,
    HslAdjustment, ToneCurveAdjustment, ToneCurvePoint, WhiteBalanceAdjustment,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const PRESET_SCHEMA_VERSION: u32 = 1;

/// Selective adjustment group mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetGroupMask {
    pub white_balance: bool,
    pub exposure: bool,
    pub highlights_shadows: bool,
    pub contrast: bool,
    pub tone_curve: bool,
    pub hsl: bool,
    pub crop: bool,
}

impl Default for PresetGroupMask {
    fn default() -> Self {
        Self {
            white_balance: true,
            exposure: true,
            highlights_shadows: true,
            contrast: true,
            tone_curve: true,
            hsl: true,
            crop: false, // Crop excluded by default when applying presets
        }
    }
}

impl PresetGroupMask {
    pub fn all() -> Self {
        Self {
            white_balance: true,
            exposure: true,
            highlights_shadows: true,
            contrast: true,
            tone_curve: true,
            hsl: true,
            crop: true,
        }
    }

    pub fn none() -> Self {
        Self {
            white_balance: false,
            exposure: false,
            highlights_shadows: false,
            contrast: false,
            tone_curve: false,
            hsl: false,
            crop: false,
        }
    }

    pub fn has_any(&self) -> bool {
        self.white_balance
            || self.exposure
            || self.highlights_shadows
            || self.contrast
            || self.tone_curve
            || self.hsl
            || self.crop
    }
}

/// A develop preset with selective adjustment groups.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub built_in: bool,
    #[serde(default)]
    pub white_balance: Option<WhiteBalanceAdjustment>,
    #[serde(default)]
    pub exposure: Option<ExposureAdjustment>,
    #[serde(default)]
    pub highlights_shadows: Option<HighlightsShadowsAdjustment>,
    #[serde(default)]
    pub contrast: Option<ContrastAdjustment>,
    #[serde(default)]
    pub tone_curve: Option<ToneCurveAdjustment>,
    #[serde(default)]
    pub hsl: Option<HslAdjustment>,
    #[serde(default)]
    pub crop: Option<CropAdjustment>,
}

fn default_schema_version() -> u32 {
    PRESET_SCHEMA_VERSION
}

impl Preset {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            schema_version: PRESET_SCHEMA_VERSION,
            id: id.into(),
            name: name.into(),
            description: None,
            built_in: false,
            white_balance: None,
            exposure: None,
            highlights_shadows: None,
            contrast: None,
            tone_curve: None,
            hsl: None,
            crop: None,
        }
    }

    /// Create a preset from an existing edit stack with selective groups included.
    pub fn from_stack(
        id: impl Into<String>,
        name: impl Into<String>,
        stack: &EditStack,
        mask: PresetGroupMask,
    ) -> Self {
        Self {
            schema_version: PRESET_SCHEMA_VERSION,
            id: id.into(),
            name: name.into(),
            description: None,
            built_in: false,
            white_balance: if mask.white_balance {
                Some(stack.white_balance.clone())
            } else {
                None
            },
            exposure: if mask.exposure {
                Some(stack.exposure.clone())
            } else {
                None
            },
            highlights_shadows: if mask.highlights_shadows {
                Some(stack.highlights_shadows.clone())
            } else {
                None
            },
            contrast: if mask.contrast {
                Some(stack.contrast.clone())
            } else {
                None
            },
            tone_curve: if mask.tone_curve {
                Some(stack.tone_curve.clone())
            } else {
                None
            },
            hsl: if mask.hsl {
                Some(stack.hsl.clone())
            } else {
                None
            },
            crop: if mask.crop {
                Some(stack.crop.clone())
            } else {
                None
            },
        }
    }

    /// Selectively apply the preset adjustments into an existing edit stack.
    pub fn apply_to(&self, stack: &mut EditStack) {
        if let Some(wb) = &self.white_balance {
            stack.white_balance = wb.clone();
        }
        if let Some(exp) = &self.exposure {
            stack.exposure = exp.clone();
        }
        if let Some(hs) = &self.highlights_shadows {
            stack.highlights_shadows = hs.clone();
        }
        if let Some(c) = &self.contrast {
            stack.contrast = c.clone();
        }
        if let Some(tc) = &self.tone_curve {
            stack.tone_curve = tc.clone();
        }
        if let Some(hsl) = &self.hsl {
            stack.hsl = hsl.clone();
        }
        if let Some(crop) = &self.crop {
            stack.crop = crop.clone();
        }
    }

    pub fn to_ron_string(&self) -> Result<String, ron::Error> {
        let pretty = ron::ser::PrettyConfig::new()
            .depth_limit(6)
            .separate_tuple_members(true)
            .enumerate_arrays(true);
        ron::ser::to_string_pretty(self, pretty)
    }

    pub fn from_ron_str(s: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str::<Self>(s)
    }
}

/// Returns the 8 built-in professional photography presets.
pub fn built_in_presets() -> Vec<Preset> {
    vec![
        // 1. Neutral (Reset to standard baseline)
        Preset {
            schema_version: PRESET_SCHEMA_VERSION,
            id: "builtin_neutral".to_string(),
            name: "preset_name_neutral".to_string(),
            description: Some("preset_desc_neutral".to_string()),
            built_in: true,
            white_balance: Some(WhiteBalanceAdjustment::default()),
            exposure: Some(ExposureAdjustment::default()),
            highlights_shadows: Some(HighlightsShadowsAdjustment::default()),
            contrast: Some(ContrastAdjustment::default()),
            tone_curve: Some(ToneCurveAdjustment::default()),
            hsl: Some(HslAdjustment::default()),
            crop: None,
        },
        // 2. Vivid (Punchy colors, elevated contrast)
        Preset {
            schema_version: PRESET_SCHEMA_VERSION,
            id: "builtin_vivid".to_string(),
            name: "preset_name_vivid".to_string(),
            description: Some("preset_desc_vivid".to_string()),
            built_in: true,
            white_balance: None,
            exposure: Some(ExposureAdjustment {
                ev: 0.2,
                ..Default::default()
            }),
            highlights_shadows: Some(HighlightsShadowsAdjustment {
                highlights: -15.0,
                shadows: 15.0,
                ..Default::default()
            }),
            contrast: Some(ContrastAdjustment {
                contrast: 25.0,
                ..Default::default()
            }),
            tone_curve: None,
            hsl: {
                let mut hsl = HslAdjustment::default();
                for band in &mut hsl.bands {
                    band.saturation = 25.0;
                }
                Some(hsl)
            },
            crop: None,
        },
        // 3. Matte (Lifted blacks, soft highlight rolloff)
        Preset {
            schema_version: PRESET_SCHEMA_VERSION,
            id: "builtin_matte".to_string(),
            name: "preset_name_matte".to_string(),
            description: Some("preset_desc_matte".to_string()),
            built_in: true,
            white_balance: None,
            exposure: None,
            highlights_shadows: Some(HighlightsShadowsAdjustment {
                highlights: -10.0,
                shadows: 25.0,
                ..Default::default()
            }),
            contrast: Some(ContrastAdjustment {
                contrast: -10.0,
                ..Default::default()
            }),
            tone_curve: Some(ToneCurveAdjustment {
                enabled: true,
                points: vec![
                    ToneCurvePoint { x: 0.0, y: 0.12 },
                    ToneCurvePoint { x: 0.25, y: 0.26 },
                    ToneCurvePoint { x: 0.5, y: 0.50 },
                    ToneCurvePoint { x: 0.75, y: 0.74 },
                    ToneCurvePoint { x: 1.0, y: 0.90 },
                ],
            }),
            hsl: None,
            crop: None,
        },
        // 4. Black & White (Desaturated with punchy tonal contrast)
        Preset {
            schema_version: PRESET_SCHEMA_VERSION,
            id: "builtin_bw".to_string(),
            name: "preset_name_bw".to_string(),
            description: Some("preset_desc_bw".to_string()),
            built_in: true,
            white_balance: None,
            exposure: Some(ExposureAdjustment {
                ev: 0.1,
                ..Default::default()
            }),
            highlights_shadows: Some(HighlightsShadowsAdjustment {
                highlights: -25.0,
                shadows: 20.0,
                ..Default::default()
            }),
            contrast: Some(ContrastAdjustment {
                contrast: 35.0,
                ..Default::default()
            }),
            tone_curve: Some(ToneCurveAdjustment {
                enabled: true,
                points: vec![
                    ToneCurvePoint { x: 0.0, y: 0.0 },
                    ToneCurvePoint { x: 0.25, y: 0.20 },
                    ToneCurvePoint { x: 0.5, y: 0.50 },
                    ToneCurvePoint { x: 0.75, y: 0.80 },
                    ToneCurvePoint { x: 1.0, y: 1.0 },
                ],
            }),
            hsl: {
                let mut hsl = HslAdjustment::default();
                for band in &mut hsl.bands {
                    band.saturation = -100.0;
                }
                Some(hsl)
            },
            crop: None,
        },
        // 5. Warm Film (Golden warmth, lifted shadows)
        Preset {
            schema_version: PRESET_SCHEMA_VERSION,
            id: "builtin_warm_film".to_string(),
            name: "preset_name_warm_film".to_string(),
            description: Some("preset_desc_warm_film".to_string()),
            built_in: true,
            white_balance: Some(WhiteBalanceAdjustment {
                enabled: true,
                temperature: 18.0,
                tint: 6.0,
            }),
            exposure: None,
            highlights_shadows: Some(HighlightsShadowsAdjustment {
                highlights: -20.0,
                shadows: 20.0,
                ..Default::default()
            }),
            contrast: Some(ContrastAdjustment {
                contrast: 15.0,
                ..Default::default()
            }),
            tone_curve: Some(ToneCurveAdjustment {
                enabled: true,
                points: vec![
                    ToneCurvePoint { x: 0.0, y: 0.08 },
                    ToneCurvePoint { x: 0.25, y: 0.24 },
                    ToneCurvePoint { x: 0.5, y: 0.52 },
                    ToneCurvePoint { x: 0.75, y: 0.76 },
                    ToneCurvePoint { x: 1.0, y: 0.95 },
                ],
            }),
            hsl: {
                let mut hsl = HslAdjustment::default();
                hsl.bands[1].saturation = 15.0; // Orange
                hsl.bands[2].saturation = 15.0; // Yellow
                hsl.bands[5].luminance = -10.0; // Blue
                Some(hsl)
            },
            crop: None,
        },
        // 6. Cool Cinematic (Teal shadows, cool mood)
        Preset {
            schema_version: PRESET_SCHEMA_VERSION,
            id: "builtin_cool_cinematic".to_string(),
            name: "preset_name_cool_cinematic".to_string(),
            description: Some("preset_desc_cool_cinematic".to_string()),
            built_in: true,
            white_balance: Some(WhiteBalanceAdjustment {
                enabled: true,
                temperature: -15.0,
                tint: -5.0,
            }),
            exposure: None,
            highlights_shadows: Some(HighlightsShadowsAdjustment {
                highlights: -15.0,
                shadows: 15.0,
                ..Default::default()
            }),
            contrast: Some(ContrastAdjustment {
                contrast: 20.0,
                ..Default::default()
            }),
            tone_curve: Some(ToneCurveAdjustment {
                enabled: true,
                points: vec![
                    ToneCurvePoint { x: 0.0, y: 0.04 },
                    ToneCurvePoint { x: 0.25, y: 0.22 },
                    ToneCurvePoint { x: 0.5, y: 0.48 },
                    ToneCurvePoint { x: 0.75, y: 0.78 },
                    ToneCurvePoint { x: 1.0, y: 1.0 },
                ],
            }),
            hsl: {
                let mut hsl = HslAdjustment::default();
                hsl.bands[4].saturation = 20.0; // Cyan
                hsl.bands[5].saturation = 15.0; // Blue
                hsl.bands[1].luminance = 10.0; // Orange skin tones
                Some(hsl)
            },
            crop: None,
        },
        // 7. Punchy Contrast (High clarity and dynamic range)
        Preset {
            schema_version: PRESET_SCHEMA_VERSION,
            id: "builtin_punchy_contrast".to_string(),
            name: "preset_name_punchy_contrast".to_string(),
            description: Some("preset_desc_punchy_contrast".to_string()),
            built_in: true,
            white_balance: None,
            exposure: None,
            highlights_shadows: Some(HighlightsShadowsAdjustment {
                highlights: -30.0,
                shadows: 25.0,
                ..Default::default()
            }),
            contrast: Some(ContrastAdjustment {
                contrast: 50.0,
                ..Default::default()
            }),
            tone_curve: Some(ToneCurveAdjustment {
                enabled: true,
                points: vec![
                    ToneCurvePoint { x: 0.0, y: 0.0 },
                    ToneCurvePoint { x: 0.25, y: 0.18 },
                    ToneCurvePoint { x: 0.5, y: 0.50 },
                    ToneCurvePoint { x: 0.75, y: 0.82 },
                    ToneCurvePoint { x: 1.0, y: 1.0 },
                ],
            }),
            hsl: None,
            crop: None,
        },
        // 8. Moody Landscape (Deep foliage and sky)
        Preset {
            schema_version: PRESET_SCHEMA_VERSION,
            id: "builtin_moody_landscape".to_string(),
            name: "preset_name_moody_landscape".to_string(),
            description: Some("preset_desc_moody_landscape".to_string()),
            built_in: true,
            white_balance: None,
            exposure: Some(ExposureAdjustment {
                ev: -0.15,
                ..Default::default()
            }),
            highlights_shadows: Some(HighlightsShadowsAdjustment {
                highlights: -40.0,
                shadows: 30.0,
                ..Default::default()
            }),
            contrast: Some(ContrastAdjustment {
                contrast: 20.0,
                ..Default::default()
            }),
            tone_curve: None,
            hsl: {
                let mut hsl = HslAdjustment::default();
                hsl.bands[3].saturation = -20.0; // Green
                hsl.bands[3].luminance = -15.0;
                hsl.bands[5].saturation = 15.0; // Blue
                hsl.bands[5].luminance = -25.0;
                hsl.bands[1].saturation = 20.0; // Orange
                Some(hsl)
            },
            crop: None,
        },
    ]
}

/// Default user presets directory (`~/.config/beam/presets` or `%APPDATA%\beam\presets`).
pub fn default_presets_dir() -> PathBuf {
    if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(config_home).join("beam").join("presets")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config/beam/presets")
    } else if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata).join("beam/presets")
    } else {
        PathBuf::from("beam_presets")
    }
}

/// Load all user presets from the specified directory.
pub fn load_user_presets(dir: &Path) -> Vec<Preset> {
    let mut presets = Vec::new();
    if !dir.exists() {
        return presets;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("ron") {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(preset) = Preset::from_ron_str(&content) {
                        presets.push(preset);
                    }
                }
            }
        }
    }
    presets.sort_by_key(|a| a.name.to_lowercase());
    presets
}

/// Save a user preset to disk.
pub fn save_user_preset(preset: &Preset, dir: &Path) -> Result<PathBuf, std::io::Error> {
    fs::create_dir_all(dir)?;
    let clean_id: String = preset
        .id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let filename = format!("{clean_id}.beam.preset.ron");
    let path = dir.join(filename);
    let s = preset
        .to_ron_string()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    fs::write(&path, s)?;
    Ok(path)
}

/// Delete a user preset file from disk.
pub fn delete_user_preset(id: &str, dir: &Path) -> Result<bool, std::io::Error> {
    if !dir.exists() {
        return Ok(false);
    }
    let clean_id: String = id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let filename = format!("{clean_id}.beam.preset.ron");
    let path = dir.join(filename);
    if path.exists() {
        fs::remove_file(path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_built_in_presets_validity() {
        let presets = built_in_presets();
        assert_eq!(presets.len(), 8);
        for p in presets {
            assert!(p.built_in);
            assert!(!p.id.is_empty());
            assert!(!p.name.is_empty());
            assert!(p.description.is_some());
            let ron = p.to_ron_string().expect("Serialization failed");
            let roundtrip = Preset::from_ron_str(&ron).expect("Deserialization failed");
            assert_eq!(p, roundtrip);
        }
    }

    #[test]
    fn test_selective_preset_apply() {
        let mut stack = EditStack::default();
        stack.white_balance.temperature = 25.0;
        stack.contrast.contrast = 10.0;

        // Create a preset that only has contrast and exposure
        let preset = Preset {
            schema_version: PRESET_SCHEMA_VERSION,
            id: "test_contrast".to_string(),
            name: "Contrast Boost".to_string(),
            description: None,
            built_in: false,
            white_balance: None,
            exposure: Some(ExposureAdjustment {
                ev: 1.0,
                ..Default::default()
            }),
            highlights_shadows: None,
            contrast: Some(ContrastAdjustment {
                contrast: 50.0,
                ..Default::default()
            }),
            tone_curve: None,
            hsl: None,
            crop: None,
        };

        preset.apply_to(&mut stack);

        // White balance should remain untouched
        assert_eq!(stack.white_balance.temperature, 25.0);
        // Contrast should be updated
        assert_eq!(stack.contrast.contrast, 50.0);
        // Exposure should be updated
        assert_eq!(stack.exposure.ev, 1.0);
    }

    #[test]
    fn test_preset_from_stack_mask() {
        let mut stack = EditStack::default();
        stack.white_balance.temperature = 42.0;
        stack.contrast.contrast = 20.0;
        stack.exposure.ev = -0.5;

        let mask = PresetGroupMask {
            white_balance: true,
            exposure: false,
            highlights_shadows: false,
            contrast: true,
            tone_curve: false,
            hsl: false,
            crop: false,
        };

        let preset = Preset::from_stack("custom_1", "My Preset", &stack, mask);
        assert!(preset.white_balance.is_some());
        assert_eq!(preset.white_balance.unwrap().temperature, 42.0);
        assert!(preset.contrast.is_some());
        assert_eq!(preset.contrast.unwrap().contrast, 20.0);
        assert!(preset.exposure.is_none());
    }

    #[test]
    fn test_save_and_load_user_preset() {
        let temp_dir = std::env::temp_dir().join("beam_preset_test");
        let _ = fs::remove_dir_all(&temp_dir);

        let preset = Preset {
            schema_version: PRESET_SCHEMA_VERSION,
            id: "user_vivid_landscape".to_string(),
            name: "Vivid Landscape".to_string(),
            description: Some("Custom landscape preset".to_string()),
            built_in: false,
            white_balance: None,
            exposure: Some(ExposureAdjustment {
                ev: 0.5,
                ..Default::default()
            }),
            highlights_shadows: None,
            contrast: None,
            tone_curve: None,
            hsl: None,
            crop: None,
        };

        let saved_path = save_user_preset(&preset, &temp_dir).expect("Save failed");
        assert!(saved_path.exists());

        let loaded = load_user_presets(&temp_dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "user_vivid_landscape");
        assert_eq!(loaded[0].name, "Vivid Landscape");

        let deleted = delete_user_preset("user_vivid_landscape", &temp_dir).expect("Delete failed");
        assert!(deleted);
        let loaded_after = load_user_presets(&temp_dir);
        assert_eq!(loaded_after.len(), 0);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
