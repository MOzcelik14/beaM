//! Media decoding and RAW processing for beaM.

pub mod decoder;
pub mod export;

pub use decoder::{
    apply_orientation, apply_orientation_f32, decode_image, decode_raster_image, generate_proxy,
    generate_thumbnail, generate_thumbnail_with_edits, is_raster_file, save_thumbnail,
    srgb_to_rec2020_linear, thumbnail_cache_path, DecodedImage, ImageMetadata, PROXY_MAX_DIMENSION,
};
pub use export::{
    apply_unsharp_mask, calculate_target_dimensions, expand_filename_template, export_batch,
    export_decoded_image, export_image, extract_raw_exif, generate_output_path,
    get_icc_profile_bytes, resolve_unique_output_path, sanitize_exif_orientation, BatchProgress,
    ExportConfig, ExportFormat, OutputSharpening, ResizeMode,
};
