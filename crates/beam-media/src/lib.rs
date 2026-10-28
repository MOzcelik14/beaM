//! Media decoding and RAW processing for beaM.

pub mod decoder;

pub use decoder::{
    decode_image, generate_proxy, generate_thumbnail, save_thumbnail, thumbnail_cache_path,
    DecodedImage, ImageMetadata, PROXY_MAX_DIMENSION,
};
