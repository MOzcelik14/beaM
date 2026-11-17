//! beaM. core crate: edit model, sidecars, color science, and errors.

/// The official user-facing display name of the application.
/// Note the required trailing dot: "beaM."
pub const APP_NAME: &str = "beaM.";

pub mod color;
pub mod error;
pub mod fs_util;
pub mod geometry;
pub mod i18n;
pub mod lut;
pub mod plugin;
pub mod preset;
pub mod segmentation;
pub mod sidecar;

pub use color::ExportColorSpace;
pub use error::{BeamError, Result};
pub use fs_util::atomic_write;
pub use geometry::{
    calculate_cropped_dimensions, calculate_effective_aspect_ratio, create_centered_crop_rect,
    map_output_uv_to_source, map_output_uv_to_source_with_aspect,
};
pub use i18n::{
    current_language, set_language, t, t_with_args, AppConfig, I18n, Language, ShortcutConfig,
    ShortcutProfile,
};
pub use plugin::{
    default_plugins_dir, discover_plugins, install_sample_plugin, run_plugin, PluginCategory,
    PluginManifest, PluginOutput,
};
pub use preset::{
    built_in_presets, default_presets_dir, delete_user_preset, load_user_presets, save_user_preset,
    Preset, PresetGroupMask, PRESET_SCHEMA_VERSION,
};
pub use segmentation::{detect_sky_mask, detect_subject_mask};
pub use sidecar::{
    ColorLabel, CropAdjustment, CropAspectPreset, EditStack, HistoryStep, LensAdjustment, Sidecar,
    CURRENT_FORMAT_VERSION, MAX_HISTORY_STEPS,
};
