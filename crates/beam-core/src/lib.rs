//! beaM. core crate: edit model, sidecars, color science, and errors.

/// The official user-facing display name of the application.
/// Note the required trailing dot: "beaM."
pub const APP_NAME: &str = "beaM.";

pub mod color;
pub mod error;
pub mod geometry;
pub mod i18n;
pub mod preset;
pub mod sidecar;

pub use color::ExportColorSpace;
pub use error::{BeamError, Result};
pub use geometry::{
    calculate_cropped_dimensions, calculate_effective_aspect_ratio, create_centered_crop_rect,
    map_output_uv_to_source,
};
pub use i18n::{current_language, set_language, t, t_with_args, AppConfig, I18n, Language};
pub use preset::{
    built_in_presets, default_presets_dir, delete_user_preset, load_user_presets, save_user_preset,
    Preset, PresetGroupMask, PRESET_SCHEMA_VERSION,
};
pub use sidecar::{
    CropAdjustment, CropAspectPreset, EditStack, HistoryStep, Sidecar, CURRENT_FORMAT_VERSION,
    MAX_HISTORY_STEPS,
};
