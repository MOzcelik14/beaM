//! Media decoding and RAW processing for beaM.

pub mod decoder;

pub use decoder::{decode_image, DecodedImage, ImageMetadata};
