use std::fs;
use std::io::Cursor;
use std::path::Path;

use beam_core::i18n::AppConfig;
use beam_core::preset::built_in_presets;
use beam_core::sidecar::{EditStack, Sidecar};
use image::{ImageBuffer, ImageFormat, Rgba};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Generating fuzz seed corpora...");

    // 1. fuzz_image_decode
    let img_corpus = Path::new("corpus/fuzz_image_decode");
    fs::create_dir_all(img_corpus)?;

    // 8-bit RGBA PNG
    let img8: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(32, 32, Rgba([128, 64, 32, 255]));
    let mut png8_bytes = Vec::new();
    img8.write_to(&mut Cursor::new(&mut png8_bytes), ImageFormat::Png)?;
    fs::write(img_corpus.join("seed_8bit.png"), &png8_bytes)?;

    // 16-bit RGBA PNG
    let img16: ImageBuffer<Rgba<u16>, Vec<u16>> =
        ImageBuffer::from_pixel(16, 16, Rgba([32768, 16384, 8192, 65535]));
    let mut png16_bytes = Vec::new();
    img16.write_to(&mut Cursor::new(&mut png16_bytes), ImageFormat::Png)?;
    fs::write(img_corpus.join("seed_16bit.png"), &png16_bytes)?;

    // RGB JPEG
    let rgb8 = image::DynamicImage::ImageRgba8(img8).to_rgb8();
    let mut jpg_bytes = Vec::new();
    rgb8.write_to(&mut Cursor::new(&mut jpg_bytes), ImageFormat::Jpeg)?;
    fs::write(img_corpus.join("seed.jpg"), &jpg_bytes)?;

    // TIFF
    let mut tiff_bytes = Vec::new();
    rgb8.write_to(&mut Cursor::new(&mut tiff_bytes), ImageFormat::Tiff)?;
    fs::write(img_corpus.join("seed.tiff"), &tiff_bytes)?;

    // WebP
    let mut webp_bytes = Vec::new();
    let dyn_img = image::DynamicImage::ImageRgb8(rgb8);
    dyn_img.write_to(&mut Cursor::new(&mut webp_bytes), ImageFormat::WebP)?;
    fs::write(img_corpus.join("seed.webp"), &webp_bytes)?;

    println!("Generated image seeds in {}", img_corpus.display());

    // 2. fuzz_icc_parse
    let icc_corpus = Path::new("corpus/fuzz_icc_parse");
    fs::create_dir_all(icc_corpus)?;
    let p3 = moxcms::ColorProfile::new_display_p3();
    if let Ok(p3_bytes) = p3.encode() {
        fs::write(icc_corpus.join("display_p3.icc"), p3_bytes)?;
    }
    let srgb = moxcms::ColorProfile::new_srgb();
    if let Ok(srgb_bytes) = srgb.encode() {
        fs::write(icc_corpus.join("srgb.icc"), srgb_bytes)?;
    }
    println!("Generated ICC seeds in {}", icc_corpus.display());

    // 3. fuzz_exif_parse
    let exif_corpus = Path::new("corpus/fuzz_exif_parse");
    fs::create_dir_all(exif_corpus)?;
    // Minimal standard TIFF header + IFD with 1 entry (Orientation = 1)
    let mut exif_buf = Vec::new();
    // Little-endian header: II (0x49, 0x49), version 42 (0x2A, 0x00), offset to IFD 8 (0x08, 0x00, 0x00, 0x00)
    exif_buf.extend_from_slice(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]);
    // Number of directory entries: 1 (0x01, 0x00)
    exif_buf.extend_from_slice(&[0x01, 0x00]);
    // Entry 1: Tag 0x0112 (Orientation), Type 3 (SHORT), Count 1, Value 1
    exif_buf.extend_from_slice(&[
        0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    ]);
    // Next IFD offset: 0
    exif_buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    fs::write(exif_corpus.join("valid_exif.bin"), exif_buf)?;
    println!("Generated EXIF seed in {}", exif_corpus.display());

    // 4. fuzz_sidecar_loader
    let sidecar_corpus = Path::new("corpus/fuzz_sidecar_loader");
    fs::create_dir_all(sidecar_corpus)?;
    let mut sidecar = Sidecar::new("photo.arw");
    sidecar.rating = 4;
    sidecar.edit_stack.exposure.ev = 1.2;
    sidecar.edit_stack.contrast.contrast = 15.0;
    let sidecar_ron = sidecar.to_ron_string()?;
    fs::write(sidecar_corpus.join("sample.beam"), sidecar_ron)?;

    // Standalone beam project sidecar
    let mut stack2 = EditStack::default();
    stack2.white_balance.temperature = 500.0;
    let mut proj = Sidecar::new("RAW_2026.CR3");
    proj.edit_stack = stack2;
    fs::write(
        sidecar_corpus.join("standalone.beam"),
        proj.to_ron_string()?,
    )?;
    println!("Generated sidecar seeds in {}", sidecar_corpus.display());

    // 5. fuzz_preset_loader
    let preset_corpus = Path::new("corpus/fuzz_preset_loader");
    fs::create_dir_all(preset_corpus)?;
    for preset in built_in_presets() {
        let name = preset.name.to_lowercase().replace(' ', "_");
        fs::write(
            preset_corpus.join(format!("{name}.ron")),
            preset.to_ron_string()?,
        )?;
    }
    println!("Generated preset seeds in {}", preset_corpus.display());

    // 6. fuzz_config_loader
    let config_corpus = Path::new("corpus/fuzz_config_loader");
    fs::create_dir_all(config_corpus)?;
    let cfg = AppConfig::default();
    let cfg_ron = ron::ser::to_string_pretty(&cfg, ron::ser::PrettyConfig::new())?;
    fs::write(config_corpus.join("default_config.ron"), cfg_ron)?;
    println!("Generated config seeds in {}", config_corpus.display());

    // 7. fuzz_raw_decode
    let raw_corpus = Path::new("corpus/fuzz_raw_decode");
    fs::create_dir_all(raw_corpus)?;
    // Seed 1: Minimal TIFF structure with RAW tags (Little-endian)
    let mut raw_seed_le = Vec::new();
    raw_seed_le.extend_from_slice(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]);
    // Number of tags: 2
    raw_seed_le.extend_from_slice(&[0x02, 0x00]);
    // Tag 0x0100 (ImageWidth), Type 4 (LONG), Count 1, Value 64
    raw_seed_le.extend_from_slice(&[
        0x00, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
    ]);
    // Tag 0x0101 (ImageLength), Type 4 (LONG), Count 1, Value 64
    raw_seed_le.extend_from_slice(&[
        0x01, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
    ]);
    raw_seed_le.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // End of IFD
    fs::write(raw_corpus.join("seed_le.raw"), raw_seed_le)?;

    // Seed 2: Big-endian TIFF
    let mut raw_seed_be = Vec::new();
    raw_seed_be.extend_from_slice(&[0x4D, 0x4D, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08]);
    raw_seed_be.extend_from_slice(&[0x00, 0x01]);
    // Tag 0x0112 (Orientation), Type 3 (SHORT), Count 1, Value 1
    raw_seed_be.extend_from_slice(&[
        0x01, 0x12, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00,
    ]);
    raw_seed_be.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    fs::write(raw_corpus.join("seed_be.raw"), raw_seed_be)?;

    println!("Generated RAW seeds in {}", raw_corpus.display());
    println!("All seed corpora generated successfully.");
    Ok(())
}
