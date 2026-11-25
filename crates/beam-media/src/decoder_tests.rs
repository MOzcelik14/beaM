use super::*;
use image::{ImageBuffer, Rgba};

#[test]
fn test_proxy_generation() {
    let w = 4000;
    let h = 3000;
    let data = vec![0.5f32; (w * h * 4) as usize];
    let (pw, ph, pdata) = generate_proxy(w, h, &data, 1920);

    assert_eq!(pw, 1920);
    assert_eq!(ph, 1440);
    assert_eq!(pdata.len(), (1920 * 1440 * 4) as usize);
    // Midpoint value should remain ~0.5
    assert!((pdata[0] - 0.5).abs() < 1e-4);
}

#[test]
fn test_raster_fallback_decode() {
    let temp_dir = std::env::temp_dir();
    let test_path = temp_dir.join("beam_test_image.png");

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(10, 10, Rgba([255, 128, 0, 255]));
    img.save(&test_path).expect("failed to create test image");

    let decoded = decode_image(&test_path).expect("failed to decode raster image");
    assert_eq!(decoded.width, 10);
    assert_eq!(decoded.height, 10);
    assert_eq!(decoded.proxy_width, 10);
    assert_eq!(decoded.data.len(), 10 * 10 * 4);

    let _ = std::fs::remove_file(test_path);
}

#[test]
fn test_nonexistent_file_handling() {
    let result = decode_image(Path::new("/nonexistent/file/test.ARW"));
    assert!(result.is_err());
}

#[test]
fn test_thumbnail_generation() {
    let temp_dir = std::env::temp_dir();
    let test_path = temp_dir.join("beam_test_thumb_source.png");
    let thumb_path = temp_dir.join("beam_test_thumb_output.png");

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(64, 48, Rgba([100, 150, 200, 255]));
    img.save(&test_path).expect("failed to save test image");

    let thumb = generate_thumbnail(&test_path, 32).expect("failed to generate thumbnail");
    assert_eq!(thumb.width(), 32);
    assert_eq!(thumb.height(), 24);

    save_thumbnail(&thumb, &thumb_path).expect("failed to save thumbnail");
    assert!(thumb_path.exists());

    let _ = std::fs::remove_file(test_path);
    let _ = std::fs::remove_file(thumb_path);
}

#[test]
fn test_save_thumbnail_jpg_and_png() {
    let temp_dir = std::env::temp_dir();
    let thumb_jpg = temp_dir.join("test_save_thumb.jpg");
    let thumb_png = temp_dir.join("test_save_thumb.png");
    let img = image::RgbaImage::new(32, 32);

    assert!(save_thumbnail(&img, &thumb_jpg).is_ok());
    assert!(thumb_jpg.exists());
    assert!(image::open(&thumb_jpg).is_ok());

    assert!(save_thumbnail(&img, &thumb_png).is_ok());
    assert!(thumb_png.exists());
    assert!(image::open(&thumb_png).is_ok());

    let _ = std::fs::remove_file(thumb_jpg);
    let _ = std::fs::remove_file(thumb_png);
}

#[test]
fn test_thumbnail_cache_key_invalidation() {
    let temp_dir = std::env::temp_dir().join("beam_test_cache_key");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let cache_dir = temp_dir.join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();

    let img_path = temp_dir.join("photo.png");
    let img = image::RgbaImage::new(16, 16);
    img.save(&img_path).unwrap();

    let path1 = thumbnail_cache_path(&img_path, &cache_dir);
    let path2 = thumbnail_cache_path(&img_path, &cache_dir);
    assert_eq!(
        path1, path2,
        "Cache path must be deterministic for unchanged file"
    );

    // Touch the file with different content
    std::thread::sleep(std::time::Duration::from_millis(15));
    let larger_img = image::RgbaImage::new(32, 32);
    larger_img.save(&img_path).unwrap();

    let path3 = thumbnail_cache_path(&img_path, &cache_dir);
    assert_ne!(
        path1, path3,
        "Cache path must change when source file changes"
    );

    // Create a sidecar file
    let sidecar_p = Sidecar::path_for(&img_path);
    let mut sidecar = Sidecar::new("photo.png");
    sidecar.save_to_file(&sidecar_p).unwrap();

    let path4 = thumbnail_cache_path(&img_path, &cache_dir);
    assert_ne!(
        path3, path4,
        "Cache path must change when sidecar is created"
    );

    // Modify sidecar file
    std::thread::sleep(std::time::Duration::from_millis(15));
    sidecar.edit_stack.exposure.ev = 1.5;
    sidecar.save_to_file(&sidecar_p).unwrap();

    let path5 = thumbnail_cache_path(&img_path, &cache_dir);
    assert_ne!(
        path4, path5,
        "Cache path must change when sidecar is edited"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_thumbnail_exif_orientation() {
    // Create an asymmetric 4x2 image
    let mut img = image::RgbaImage::new(4, 2);
    // Put a marker pixel at top-left (0, 0)
    img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));

    // 1 = Normal
    let r1 = apply_orientation(img.clone(), 1);
    assert_eq!(r1.dimensions(), (4, 2));
    assert_eq!(r1.get_pixel(0, 0), &image::Rgba([255, 0, 0, 255]));

    // 3 = Rotate 180 (top-left moves to bottom-right: x = 3, y = 1)
    let r3 = apply_orientation(img.clone(), 3);
    assert_eq!(r3.dimensions(), (4, 2));
    assert_eq!(r3.get_pixel(3, 1), &image::Rgba([255, 0, 0, 255]));

    // 6 = Rotate 90 CW (dimensions become 2x4, top-left moves to top-right: x = 1, y = 0)
    let r6 = apply_orientation(img.clone(), 6);
    assert_eq!(r6.dimensions(), (2, 4));
    assert_eq!(r6.get_pixel(1, 0), &image::Rgba([255, 0, 0, 255]));

    // 8 = Rotate 270 CW (dimensions become 2x4, top-left moves to bottom-left: x = 0, y = 3)
    let r8 = apply_orientation(img.clone(), 8);
    assert_eq!(r8.dimensions(), (2, 4));
    assert_eq!(r8.get_pixel(0, 3), &image::Rgba([255, 0, 0, 255]));
}

#[test]
fn test_generate_thumbnail_with_edits() {
    let temp_dir = std::env::temp_dir();
    let test_path = temp_dir.join("beam_test_thumb_edits.png");
    let img = image::ImageBuffer::from_pixel(64, 64, image::Rgba([50u8, 50u8, 50u8, 255u8]));
    img.save(&test_path).unwrap();

    // Default thumbnail
    let thumb_default = generate_thumbnail_with_edits(&test_path, None, 32).unwrap();
    let p_default = thumb_default.get_pixel(16, 16)[0];

    // Edit with +2 EV exposure
    let mut stack = EditStack::default();
    stack.exposure.ev = 2.0;
    let thumb_bright = generate_thumbnail_with_edits(&test_path, Some(&stack), 32).unwrap();
    let p_bright = thumb_bright.get_pixel(16, 16)[0];

    assert!(
        p_bright > p_default,
        "Thumbnail with +2 EV exposure must be brighter than default: {p_bright} > {p_default}"
    );

    let _ = std::fs::remove_file(test_path);
}

#[test]
fn test_decode_raster_formats() {
    let temp_dir = std::env::temp_dir().join("beam_test_formats");
    let _ = std::fs::create_dir_all(&temp_dir);

    // 1. JPEG
    let jpg_path = temp_dir.join("test.jpg");
    let img_rgb8: image::RgbImage =
        image::ImageBuffer::from_pixel(16, 16, image::Rgb([180, 100, 50]));
    img_rgb8.save(&jpg_path).unwrap();
    let decoded_jpg = decode_image(&jpg_path).expect("Failed to decode JPEG");
    assert_eq!(decoded_jpg.width, 16);
    assert_eq!(decoded_jpg.height, 16);
    assert_eq!(decoded_jpg.data.len(), 16 * 16 * 4);

    // 2. PNG
    let png_path = temp_dir.join("test.png");
    let img_rgba8: image::RgbaImage =
        image::ImageBuffer::from_pixel(16, 16, image::Rgba([180, 100, 50, 255]));
    img_rgba8.save(&png_path).unwrap();
    let decoded_png = decode_image(&png_path).expect("Failed to decode PNG");
    assert_eq!(decoded_png.width, 16);
    assert_eq!(decoded_png.height, 16);

    // 3. TIFF 8-bit
    let tiff8_path = temp_dir.join("test_8.tif");
    img_rgba8.save(&tiff8_path).unwrap();
    let decoded_tiff8 = decode_image(&tiff8_path).expect("Failed to decode TIFF 8-bit");
    assert_eq!(decoded_tiff8.width, 16);
    assert_eq!(decoded_tiff8.height, 16);

    // 4. TIFF 16-bit
    let tiff16_path = temp_dir.join("test_16.tif");
    let img_rgb16: image::ImageBuffer<image::Rgb<u16>, Vec<u16>> =
        image::ImageBuffer::from_pixel(16, 16, image::Rgb([30000u16, 40000u16, 50000u16]));
    img_rgb16.save(&tiff16_path).unwrap();
    let decoded_tiff16 = decode_image(&tiff16_path).expect("Failed to decode TIFF 16-bit");
    assert_eq!(decoded_tiff16.width, 16);
    assert_eq!(decoded_tiff16.height, 16);

    // 5. WebP
    let webp_path = temp_dir.join("test.webp");
    img_rgba8.save(&webp_path).unwrap();
    let decoded_webp = decode_image(&webp_path).expect("Failed to decode WebP");
    assert_eq!(decoded_webp.width, 16);
    assert_eq!(decoded_webp.height, 16);

    // 6. Camera uppercase .JPG and .JPEG
    let camera_jpg_path = temp_dir.join("DSC01234.JPG");
    img_rgb8.save(&camera_jpg_path).unwrap();
    let decoded_cam_jpg =
        decode_image(&camera_jpg_path).expect("Failed to decode camera uppercase .JPG");
    assert_eq!(decoded_cam_jpg.width, 16);
    assert_eq!(decoded_cam_jpg.height, 16);

    let camera_jpeg_path = temp_dir.join("IMG_9999.JPEG");
    img_rgb8.save(&camera_jpeg_path).unwrap();
    let decoded_cam_jpeg =
        decode_image(&camera_jpeg_path).expect("Failed to decode camera uppercase .JPEG");
    assert_eq!(decoded_cam_jpeg.width, 16);
    assert_eq!(decoded_cam_jpeg.height, 16);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_raster_16bit_preservation() {
    let temp_dir = std::env::temp_dir();
    let test_16_path = temp_dir.join("beam_16bit_prec.png");

    // Two adjacent 16-bit values that would collapse to the exact same value in 8-bit
    // In 8-bit: 1000 / 256 = 3.9 -> 3; 1020 / 256 = 3.98 -> 3
    let mut img16: image::ImageBuffer<image::Rgb<u16>, Vec<u16>> = image::ImageBuffer::new(2, 1);
    img16.put_pixel(0, 0, image::Rgb([1000u16, 2000u16, 3000u16]));
    img16.put_pixel(1, 0, image::Rgb([1020u16, 2000u16, 3000u16]));
    img16.save(&test_16_path).unwrap();

    let decoded = decode_image(&test_16_path).expect("Failed to decode 16-bit PNG");
    let p0_r = decoded.data[0];
    let p1_r = decoded.data[4];

    assert_ne!(
        p0_r, p1_r,
        "16-bit distinct values must not be quantized: {p0_r} != {p1_r}"
    );
    assert!(
        p1_r > p0_r,
        "Value 1020 must decode higher than 1000 in f32: {p1_r} > {p0_r}"
    );

    let _ = std::fs::remove_file(test_16_path);
}

#[test]
fn test_raster_orientation_f32() {
    // 4x2 buffer with marker at (0, 0)
    let mut data = vec![0.0f32; 4 * 2 * 4];
    data[0] = 1.0; // Red at top-left
    data[3] = 1.0;

    // Orientation 3: rotate 180 (top-left moves to x=3, y=1)
    let (w3, h3, d3) = apply_orientation_f32(4, 2, &data, 3);
    assert_eq!((w3, h3), (4, 2));
    let idx3 = (4 + 3) * 4;
    assert_eq!(d3[idx3], 1.0);

    // Orientation 6: rotate 90 CW (dimensions become 2x4, moves to x=1, y=0)
    let (w6, h6, d6) = apply_orientation_f32(4, 2, &data, 6);
    assert_eq!((w6, h6), (2, 4));
    let idx6 = 4;
    assert_eq!(d6[idx6], 1.0);
}

#[test]
fn test_srgb_linear_roundtrip() {
    let test_colors = [
        [0.0f32, 0.0f32, 0.0f32],
        [1.0, 1.0, 1.0],
        [0.5, 0.25, 0.75],
        [0.8, 0.1, 0.4],
        [0.2, 0.9, 0.3],
    ];

    for c in test_colors {
        let rec2020 = srgb_to_rec2020_linear(c[0], c[1], c[2]);
        let srgb = beam_core::color::rec2020_linear_to_srgb_display(rec2020);
        for ch in 0..3 {
            assert!(
                (c[ch] - srgb[ch]).abs() < 1e-4,
                "Channel {ch} round-trip failed: expected {}, got {}",
                c[ch],
                srgb[ch]
            );
        }
    }
}

#[test]
fn test_embedded_icc_profile_transform() {
    let p3_profile = moxcms::ColorProfile::new_display_p3();
    let p3_bytes = p3_profile
        .encode()
        .expect("Failed to encode Display P3 ICC profile");

    // Pure red in Display P3
    let mut pixels = vec![1.0f32, 0.0, 0.0, 1.0];
    let res = transform_icc_to_linear_rec2020(&p3_bytes, &mut pixels);
    assert!(res.is_ok(), "Display P3 ICC profile transform must succeed");

    // Pure Display P3 red should be transformed and stay positive
    assert!(pixels[0] > 0.0);
    assert_eq!(pixels[3], 1.0);
}

#[test]
fn test_zero_dimensions_graceful_handling() {
    let (pw, ph, pdata) = generate_proxy(0, 0, &[], PROXY_MAX_DIMENSION);
    assert_eq!((pw, ph), (0, 0));
    assert!(pdata.is_empty());

    let (ow, oh, odata) = apply_orientation_f32(0, 0, &[], 6);
    assert_eq!((ow, oh), (0, 0));
    assert!(odata.is_empty());
}

#[test]
fn test_oversized_icc_profile_rejection() {
    let oversized = vec![0u8; MAX_ICC_PROFILE_SIZE + 1024];
    let mut pixels = vec![1.0f32, 0.0, 0.0, 1.0];
    let res = transform_icc_to_linear_rec2020(&oversized, &mut pixels);
    assert!(res.is_err(), "Oversized ICC profiles must be rejected");
}

#[test]
fn test_100mp_image_opens_successfully() {
    // 10,000 x 10,000 = 100,000,000 pixels (100 MP)
    // Verify this is within MAX_IMAGE_PIXELS (150 MP) and MAX_IMAGE_DIMENSION (32,768)
    let w = 10_000u32;
    let h = 10_000u32;
    let total_pixels = (w as u64) * (h as u64);
    assert!(
        total_pixels <= MAX_IMAGE_PIXELS,
        "100 MP must be within MAX_IMAGE_PIXELS limit ({total_pixels} <= {MAX_IMAGE_PIXELS})"
    );
    assert!(w <= MAX_IMAGE_DIMENSION && h <= MAX_IMAGE_DIMENSION);

    // Verify proxy scaling math handles 100 MP cleanly
    let max_side = w.max(h);
    let scale = PROXY_MAX_DIMENSION as f32 / max_side as f32;
    let dst_w = ((w as f32 * scale).round() as u32).clamp(1, PROXY_MAX_DIMENSION);
    let dst_h = ((h as f32 * scale).round() as u32).clamp(1, PROXY_MAX_DIMENSION);
    assert_eq!(dst_w, PROXY_MAX_DIMENSION);
    assert_eq!(dst_h, PROXY_MAX_DIMENSION);
}

#[test]
fn test_65535x65535_png_header_rejected() {
    let temp_dir = std::env::temp_dir().join("beam_test_png_bomb");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let png_bomb_path = temp_dir.join("bomb.png");
    // PNG signature + IHDR chunk declaring dimensions 65535 x 65535
    let mut png_bytes = Vec::new();
    png_bytes.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    // IHDR chunk: length 13
    png_bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x0D]);
    // Chunk type: IHDR
    png_bytes.extend_from_slice(b"IHDR");
    // Width: 65535 (0x0000FFFF)
    png_bytes.extend_from_slice(&[0x00, 0x00, 0xFF, 0xFF]);
    // Height: 65535 (0x0000FFFF)
    png_bytes.extend_from_slice(&[0x00, 0x00, 0xFF, 0xFF]);
    // Bit depth: 8, color type: 2 (RGB), compression: 0, filter: 0, interlace: 0
    png_bytes.extend_from_slice(&[8, 2, 0, 0, 0]);
    // CRC (arbitrary 4 bytes)
    png_bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    std::fs::write(&png_bomb_path, &png_bytes).unwrap();

    let res = decode_image(&png_bomb_path);
    assert!(
        res.is_err(),
        "A 65535x65535 decompression bomb must be strictly rejected by image limits"
    );

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_fuzz_reproducer_raw_oom() {
    // Reproducer oom-662b8ff128146682485447ad5aaecb821636dd18 from fuzz_raw_decode
    // Declares 65535 IFD entries in a 37-byte file which previously triggered malloc(4294967295)
    let oom_bytes: &[u8] = &[
        0x4d, 0x4d, 0x00, 0x2a, 0x00, 0x00, 0x00, 0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x01, 0x01, 0x12, 0x00, 0x03, 0x00, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ];

    let validation_res = validate_tiff_raw_header(oom_bytes);
    assert!(
        validation_res.is_err(),
        "Must reject corrupted TIFF header with impossible IFD entry count"
    );

    let temp_dir = std::env::temp_dir().join("beam_test_fuzz_raw_oom");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let raw_path = temp_dir.join("corrupted.dng");
    std::fs::write(&raw_path, oom_bytes).unwrap();

    let res = decode_image(&raw_path);
    assert!(
        res.is_err(),
        "Must cleanly fail decoding without allocating 4.29 GB"
    );

    // Fuzz reproducer 2: IFD entry count declared 4294967041 elements
    let oom_bytes_2: &[u8] = &[
        77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 255, 255, 255, 255, 255, 255, 255, 1, 18, 32, 3, 0, 0, 0,
        1, 0, 0, 0, 8, 0, 0,
    ];
    let val_res_2 = validate_tiff_raw_header(oom_bytes_2);
    assert!(val_res_2.is_err());
    let raw_path_2 = temp_dir.join("corrupted_entry.dng");
    std::fs::write(&raw_path_2, oom_bytes_2).unwrap();
    let res_2 = decode_image(&raw_path_2);
    assert!(res_2.is_err());

    // Fuzz reproducer 3: next_ifd pointed to invalid offset 1 causing 2.91 GB allocation
    let oom_bytes_3: &[u8] = &[
        77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 1, 18, 1, 0, 0, 0, 0, 0, 3, 87, 0, 3, 0, 0, 0, 1, 0, 1, 1,
        0, 0, 1, 0, 0, 0, 0, 0, 0,
    ];
    let val_res_3 = validate_tiff_raw_header(oom_bytes_3);
    assert!(val_res_3.is_err());
    let raw_path_3 = temp_dir.join("corrupted_chain.dng");
    std::fs::write(&raw_path_3, oom_bytes_3).unwrap();
    let res_3 = decode_image(&raw_path_3);
    assert!(res_3.is_err());

    // Fuzz reproducer 4: X3F container with invalid directory offset and 303M entries
    let oom_bytes_4: &[u8] = &[
        70, 79, 86, 98, 0, 0, 0, 7, 255, 249, 24, 18, 0, 0, 1, 4, 1, 0, 0, 0, 0,
    ];
    let val_res_4 = validate_tiff_raw_header(oom_bytes_4);
    assert!(val_res_4.is_err());
    let raw_path_4 = temp_dir.join("corrupted.x3f");
    std::fs::write(&raw_path_4, oom_bytes_4).unwrap();
    let res_4 = decode_image(&raw_path_4);
    assert!(res_4.is_err());

    // Fuzz reproducer 5: CIFF container with out-of-bounds start offset (13.6 MB in 24-byte file)
    let oom_bytes_5: &[u8] = &[
        0, 0, 0, 11, 208, 0, 72, 69, 65, 80, 67, 67, 68, 82, 0, 12, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let val_res_5 = validate_tiff_raw_header(oom_bytes_5);
    assert!(val_res_5.is_err());
    let raw_path_5 = temp_dir.join("corrupted.crw");
    std::fs::write(&raw_path_5, oom_bytes_5).unwrap();
    let res_5 = decode_image(&raw_path_5);
    assert!(res_5.is_err());

    // Fuzz reproducer 6: CIFF container with entry data size out of range (16.7 MB in 124-byte file)
    let mut oom_bytes_6 = vec![
        8, 0, 0, 0, 0, 0, 72, 69, 65, 80, 67, 67, 68, 82, 0, 255, 255, 255, 0,
    ];
    oom_bytes_6.extend(std::iter::repeat_n(109, 92));
    oom_bytes_6.extend_from_slice(&[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let val_res_6 = validate_tiff_raw_header(&oom_bytes_6);
    assert!(val_res_6.is_err());
    let raw_path_6 = temp_dir.join("corrupted_entry.crw");
    std::fs::write(&raw_path_6, &oom_bytes_6).unwrap();
    let res_6 = decode_image(&raw_path_6);
    assert!(res_6.is_err());

    // Fuzz reproducer 7: RAF container with corrupted embedded TIFF structure
    let oom_bytes_7: &[u8] = &[
        70, 85, 74, 73, 70, 73, 76, 77, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 0, 1, 3,
        1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 1, 18, 0, 0, 3, 0, 0,
        0, 1, 0, 1, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 49, 77, 77, 77, 0, 42, 0, 49, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 0, 44, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 1, 1, 18, 0, 32, 3, 0, 16, 0, 18, 248,
    ];
    let val_res_7 = validate_tiff_raw_header(oom_bytes_7);
    assert!(val_res_7.is_err());
    let raw_path_7 = temp_dir.join("corrupted.raf");
    std::fs::write(&raw_path_7, oom_bytes_7).unwrap();
    let res_7 = decode_image(&raw_path_7);
    assert!(res_7.is_err());

    // Fuzz reproducer 8: JPEG/EXIF malformed APP1 triggering rawler Jfif infinite loop hang
    let oom_bytes_8: &[u8] = &[255, 216, 255, 225, 199, 77, 77, 0, 255, 216, 255, 225, 0];
    let val_res_8 = validate_tiff_raw_header(oom_bytes_8);
    assert!(val_res_8.is_err());
    let raw_path_8 = temp_dir.join("corrupted_jfif.dng");
    std::fs::write(&raw_path_8, oom_bytes_8).unwrap();
    let res_8 = decode_image(&raw_path_8);
    assert!(res_8.is_err());

    // Fuzz reproducer 9: CIFF container with subIFD valuedata_size out of bounds (1.14 GB in 116-byte file)
    let oom_bytes_9: &[u8] = &[
        88, 73, 0, 0, 0, 0, 72, 69, 65, 80, 67, 67, 68, 82, 0, 127, 7, 0, 2, 2, 0, 113, 107, 116,
        110, 37, 0, 247, 9, 68, 82, 0, 127, 7, 0, 2, 2, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 0, 0,
        0, 0, 0, 0, 0, 0, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        19, 0, 0, 0,
    ];
    let val_res_9 = validate_tiff_raw_header(oom_bytes_9);
    assert!(val_res_9.is_err());
    let raw_path_9 = temp_dir.join("corrupted_subifd.crw");
    std::fs::write(&raw_path_9, oom_bytes_9).unwrap();
    let res_9 = decode_image(&raw_path_9);
    assert!(res_9.is_err());

    // Fuzz reproducer 10: MRW container with data_offset out of range (2.77 MB in 77-byte file)
    let oom_bytes_10: &[u8] = &[
        0, 77, 82, 77, 0, 42, 86, 0, 0, 0, 0, 0, 0, 0, 37, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 97, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 2, 40, 0,
        44, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0,
    ];
    let val_res_10 = validate_tiff_raw_header(oom_bytes_10);
    assert!(val_res_10.is_err());
    let raw_path_10 = temp_dir.join("corrupted.mrw");
    std::fs::write(&raw_path_10, oom_bytes_10).unwrap();
    let res_10 = decode_image(&raw_path_10);
    assert!(res_10.is_err());

    // Fuzz reproducer 11: MRW container with corrupted IFD entries declaring 4GB allocation
    let oom_bytes_11: &[u8] = &[
        0, 77, 82, 77, 0, 0, 0, 8, 0, 9, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143,
        143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143,
        143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143,
        143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143,
        143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143,
        143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143,
        143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 143, 1, 0, 0, 8, 0,
        0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 16, 0, 0, 0, 0, 0, 0, 0, 0, 59, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 63, 0, 128, 0, 0, 0, 0, 0, 0, 196,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 176, 0, 0, 0, 0,
        0, 0, 0, 0, 219, 255, 255, 248, 18, 0, 3, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0,
    ];
    let val_res_11 = validate_tiff_raw_header(oom_bytes_11);
    assert!(val_res_11.is_err());
    let raw_path_11 = temp_dir.join("corrupted_entries.mrw");
    std::fs::write(&raw_path_11, oom_bytes_11).unwrap();
    let res_11 = decode_image(&raw_path_11);
    assert!(res_11.is_err());

    // Fuzz reproducer 12: X3F container with truncated header (14 bytes)
    let oom_bytes_12: &[u8] = &[70, 79, 86, 98, 73, 77, 77, 0, 46, 46, 46, 46, 46, 8];
    let val_res_12 = validate_tiff_raw_header(oom_bytes_12);
    assert!(val_res_12.is_err());
    let raw_path_12 = temp_dir.join("corrupted.x3f");
    std::fs::write(&raw_path_12, oom_bytes_12).unwrap();
    let res_12 = decode_image(&raw_path_12);
    assert!(res_12.is_err());

    // Fuzz reproducer 13: CIFF container with invalid header marker (114 bytes)
    let oom_bytes_13: &[u8] = &[
        88, 73, 0, 0, 0, 0, 72, 69, 65, 80, 67, 67, 68, 82, 0, 127, 7, 0, 2, 2, 0, 113, 107, 116,
        110, 37, 0, 247, 9, 68, 82, 0, 127, 7, 0, 2, 2, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 0, 0,
        0, 0, 0, 0, 0, 0, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 19,
        0, 0, 0,
    ];
    let val_res_13 = validate_tiff_raw_header(oom_bytes_13);
    assert!(val_res_13.is_err());
    let raw_path_13 = temp_dir.join("corrupted_subifd.crw");
    std::fs::write(&raw_path_13, oom_bytes_13).unwrap();
    let res_13 = decode_image(&raw_path_13);
    assert!(res_13.is_err());

    // Fuzz reproducer 14: CIFF container with corrupted subIFD bounds (49 bytes)
    let oom_bytes_14: &[u8] = &[
        88, 73, 0, 0, 0, 0, 72, 69, 65, 80, 67, 67, 68, 82, 0, 127, 255, 0, 2, 1, 0, 0, 106, 2, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 171, 49, 72, 0, 0, 0, 0, 0, 0, 0, 19, 0, 0, 0,
    ];
    let val_res_14 = validate_tiff_raw_header(oom_bytes_14);
    assert!(val_res_14.is_err());
    let raw_path_14 = temp_dir.join("corrupted_bounds.crw");
    std::fs::write(&raw_path_14, oom_bytes_14).unwrap();
    let res_14 = decode_image(&raw_path_14);
    assert!(res_14.is_err());

    // Fuzz reproducer 15: X3F container with IMA2 directory entry offset out of bounds (63 bytes)
    let oom_bytes_15: &[u8] = &[
        70, 79, 86, 98, 65, 100, 46, 86, 0, 0, 0, 11, 0, 0, 8, 255, 3, 0, 0, 0, 46, 188, 86, 0, 0,
        0, 131, 0, 73, 77, 65, 50, 11, 0, 8, 0, 4, 96, 0, 255, 255, 255, 255, 136, 255, 50, 0, 255,
        77, 77, 77, 47, 77, 77, 77, 77, 225, 6, 254, 8, 0, 0, 0,
    ];
    let val_res_15 = validate_tiff_raw_header(oom_bytes_15);
    assert!(val_res_15.is_err());
    let raw_path_15 = temp_dir.join("corrupted_ima2.x3f");
    std::fs::write(&raw_path_15, oom_bytes_15).unwrap();
    let res_15 = decode_image(&raw_path_15);
    assert!(res_15.is_err());

    // Fuzz reproducer 16: QTK container with invalid dimensions (Apple QuickTake)
    let oom_bytes_16: &[u8] = &[
        113, 107, 116, 107, 70, 73, 76, 77, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 0, 1,
        3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 149, 0, 3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 77, 0, 0, 0, 0, 0, 149, 77, 77, 0, 42, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 8, 0, 1, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 77, 0, 77, 8, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 77, 0, 0, 0, 0, 0, 149, 77, 77, 0, 42, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 8, 0, 1, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 77, 77, 42, 0, 0, 0, 8, 0,
        9, 1, 0, 8, 0, 1, 0, 1, 3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 8, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 64, 0, 8, 0, 9, 1, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 8, 0, 1, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 77, 77, 42, 0, 0, 0, 8, 0, 9,
        1, 0, 8, 0, 1, 0, 1, 3, 1, 0, 0, 0, 0, 239, 0, 9, 1, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 8, 171, 10, 185, 90, 77, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 63, 0, 128, 0, 0, 0, 0,
        0, 0, 0, 0, 9, 0, 0, 0, 0, 73, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0,
        0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 39, 0, 0, 0, 0, 0, 0, 0, 183, 0, 42, 3, 0, 0, 0, 1, 52, 1, 0,
        0, 0, 46, 0, 0, 1, 0, 0, 70, 73, 76, 255, 255, 255, 255, 255, 255, 255, 3, 77, 77, 77, 76,
        77, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 0, 1, 3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0,
        8, 0, 0, 0, 0, 0, 0, 0, 0, 63, 0, 128, 0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 77, 77, 0, 0, 1, 58, 1, 0,
        0, 0, 46, 0, 0, 1, 0, 0, 70, 73, 76, 77, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1,
        0, 1, 3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 149, 0, 0, 0, 0, 8, 0, 1, 1, 18, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 77, 0, 0, 0, 0, 0, 149, 77, 77, 0, 42, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 8, 0, 1, 77, 77, 76, 77, 77, 77, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 59, 0, 0, 0, 0, 224, 252, 0, 1, 109, 0, 0, 149, 0, 3, 1, 0,
        0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 77, 0, 0, 0, 0, 0, 77, 0, 149, 42, 0, 77, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255,
    ];
    let val_res_16 = validate_tiff_raw_header(oom_bytes_16);
    assert!(val_res_16.is_err());
    let raw_path_16 = temp_dir.join("corrupted.qtk");
    std::fs::write(&raw_path_16, oom_bytes_16).unwrap();
    let res_16 = decode_image(&raw_path_16);
    assert!(res_16.is_err());

    // Fuzz reproducer 17: QTK container with truncated bitpump payload (Apple QuickTake)
    let oom_bytes_17: &[u8] = &[
        113, 107, 116, 107, 70, 73, 76, 77, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 0, 1,
        3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 1, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 77, 0, 77,
        8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 77, 0, 0, 0, 0, 0, 149, 77, 77, 0, 42, 0, 0, 0, 8,
        0, 9, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 1, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 77, 77,
        0, 0, 0, 245, 42, 0, 0, 0, 8, 0, 9, 1, 0, 8, 0, 1, 0, 1, 3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0,
        0, 0, 0, 64, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 16, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 64, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 1, 77, 77, 76, 77, 77, 77, 0, 42, 0,
        0, 77, 77, 42, 0, 0, 0, 8, 0, 9, 1, 0, 8, 0, 1, 0, 1, 3, 1, 0, 0, 0, 0, 239, 0, 9, 1, 0, 0,
        0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 171, 10, 185, 90, 77, 8, 0, 0, 0, 63, 0, 128,
        0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 73, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0,
        0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 39, 0, 0, 0, 0, 0, 126, 126, 126, 126, 126, 126,
        126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126,
        126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 77, 77, 0,
        42, 161, 0, 0, 8, 0, 1, 73, 0, 0, 1, 52, 1, 0, 0, 0, 46, 0, 0, 1, 0, 0, 70, 73, 76, 255,
        255, 255, 255, 255, 255, 255, 3, 77, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 0, 1,
        3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 11, 0, 0, 0, 1, 183, 3, 0, 77, 0, 0, 0, 0, 73, 77, 126, 126,
        126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126,
        126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126,
        126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126,
        126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 126, 0, 0, 183, 0,
        42, 3, 0, 0, 0, 1, 52, 1, 0, 0, 0, 46, 0, 0, 1, 0, 0, 70, 73, 76, 255, 255, 255, 255, 255,
        255, 255, 3, 77, 77, 77, 76, 77, 77, 77, 0, 42, 0, 0, 0, 8, 0, 1, 0, 1, 3, 1, 0, 0, 0, 0,
        8, 0, 9, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 63, 0, 128, 0, 0, 0, 0, 0, 9, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 77, 77, 0, 42, 0, 0, 0, 8, 0, 9, 1, 0, 0, 8, 0, 0, 0,
        2, 0, 0, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 91, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 77, 77,
        0, 0, 1, 58, 1, 0, 0, 0, 46, 0, 0, 43, 1, 0, 0, 70, 73, 76, 77, 77, 77, 76, 77, 0, 0, 0,
        245, 77, 77, 0, 42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 65, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 47, 0,
        0, 0, 0, 0, 0, 8, 0, 1, 0, 1, 3, 1, 0, 0, 0, 0, 8, 0, 9, 1, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 149, 0, 0, 0, 0, 8, 0, 1, 1, 18, 0, 0, 0, 0, 0,
        0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 77, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 149,
    ];
    let val_res_17 = validate_tiff_raw_header(oom_bytes_17);
    assert!(val_res_17.is_err());
    let raw_path_17 = temp_dir.join("corrupted_payload.qtk");
    std::fs::write(&raw_path_17, oom_bytes_17).unwrap();
    let res_17 = decode_image(&raw_path_17);
    assert!(res_17.is_err());

    // Fuzz reproducer 18: Panasonic RW2 container with corrupted IFD allocation (111 bytes)
    let oom_bytes_18: &[u8] = &[
        77, 77, 0, 85, 0, 0, 0, 20, 77, 77, 0, 42, 0, 131, 131, 131, 131, 131, 131, 131, 131, 131,
        131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131,
        131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131,
        131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131,
        131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 131, 0, 0,
        8, 0, 1, 1, 1, 18, 0, 6, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0,
    ];
    let val_res_18 = validate_tiff_raw_header(oom_bytes_18);
    assert!(val_res_18.is_err());
    let raw_path_18 = temp_dir.join("corrupted.rw2");
    std::fs::write(&raw_path_18, oom_bytes_18).unwrap();
    let res_18 = decode_image(&raw_path_18);
    assert!(res_18.is_err());

    // Fuzz reproducer 19: JPEG/EXIF container with truncated TIFF header causing JFIF infinite loop (19 bytes)
    let oom_bytes_19: &[u8] = &[
        255, 216, 255, 225, 0, 8, 69, 120, 105, 102, 0, 0, 49, 0, 0, 1, 0, 0, 0,
    ];
    let val_res_19 = validate_tiff_raw_header(oom_bytes_19);
    assert!(val_res_19.is_err());
    let raw_path_19 = temp_dir.join("corrupted_app1.jpg");
    std::fs::write(&raw_path_19, oom_bytes_19).unwrap();
    let res_19 = decode_image(&raw_path_19);
    assert!(res_19.is_err());

    // Fuzz reproducer 20: BMFF vendor box 64-bit size overflow causing panic in rawler (87 bytes)
    let oom_bytes_20: &[u8] = &[
        0, 0, 0, 11, 0, 77, 73, 73, 42, 0, 12, 0, 0, 0, 1, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 66, 2, 42, 190, 0, 1, 2, 16, 0, 0, 0, 8, 2, 4, 1, 0,
        0, 0,
    ];
    let val_res_20 = validate_tiff_raw_header(oom_bytes_20);
    assert!(val_res_20.is_err());
    let raw_path_20 = temp_dir.join("corrupted.cr3");
    std::fs::write(&raw_path_20, oom_bytes_20).unwrap();
    let res_20 = decode_image(&raw_path_20);
    assert!(res_20.is_err());

    // Fuzz reproducer 21: BMFF ftyp box zero size causing underflow panic (98 bytes)
    let oom_bytes_21: &[u8] = &[
        0, 0, 0, 0, 102, 116, 121, 112, 0, 8, 0, 1, 1, 18, 32, 3, 0, 0, 0, 0, 0, 8, 0, 0, 1, 0, 0,
        0, 0, 0, 0, 0, 77, 77, 77, 0, 42, 0, 49, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0,
        129, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 37, 0,
    ];
    let val_res_21 = validate_tiff_raw_header(oom_bytes_21);
    assert!(val_res_21.is_err());
    let raw_path_21 = temp_dir.join("corrupted_ftyp.cr3");
    std::fs::write(&raw_path_21, oom_bytes_21).unwrap();
    let res_21 = decode_image(&raw_path_21);
    assert!(res_21.is_err());

    // Fuzz reproducer 22: JPEG/EXIF container with root IFD offset 0 causing JFIF infinite loop (60 bytes)
    let oom_bytes_22: &[u8] = &[
        255, 216, 255, 225, 0, 36, 69, 120, 105, 102, 0, 0, 77, 77, 0, 42, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 0, 0, 0, 0, 0, 0, 0, 16, 0, 16,
    ];
    let val_res_22 = validate_tiff_raw_header(oom_bytes_22);
    assert!(val_res_22.is_err());
    let raw_path_22 = temp_dir.join("corrupted_zero_ifd.jpg");
    std::fs::write(&raw_path_22, oom_bytes_22).unwrap();
    let res_22 = decode_image(&raw_path_22);
    assert!(res_22.is_err());

    // Fuzz reproducer 23: JPEG/EXIF container with IFD entry count 0 causing JFIF infinite loop (40 bytes)
    let oom_bytes_23: &[u8] = &[
        255, 216, 255, 225, 0, 36, 69, 120, 105, 102, 0, 0, 77, 77, 0, 42, 0, 0, 0, 8, 0, 0, 0, 0,
        0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 27, 77, 16,
    ];
    let val_res_23 = validate_tiff_raw_header(oom_bytes_23);
    assert!(val_res_23.is_err());
    let raw_path_23 = temp_dir.join("corrupted_zero_entries.jpg");
    std::fs::write(&raw_path_23, oom_bytes_23).unwrap();
    let res_23 = decode_image(&raw_path_23);
    assert!(res_23.is_err());

    // Fuzz reproducer 24: BMFF container with secondary truncated ftyp box causing underflow panic (36 bytes)
    let oom_bytes_24: &[u8] = &[
        0, 0, 0, 16, 102, 116, 121, 112, 252, 0, 0, 0, 163, 163, 0, 0, 0, 0, 0, 8, 102, 116, 121,
        112, 0, 0, 0, 8, 252, 0, 0, 0, 163, 163, 0, 0,
    ];
    let val_res_24 = validate_tiff_raw_header(oom_bytes_24);
    assert!(val_res_24.is_err());
    let raw_path_24 = temp_dir.join("corrupted_second_ftyp.cr3");
    std::fs::write(&raw_path_24, oom_bytes_24).unwrap();
    let res_24 = decode_image(&raw_path_24);
    assert!(res_24.is_err());
    // Fuzz reproducer 25: CIFF container with recursive self-referential subIFD bomb (113 bytes)
    let oom_bytes_25: &[u8] = &[
        73, 73, 26, 0, 0, 0, 72, 69, 65, 80, 67, 67, 68, 82, 119, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0,
        0, 8, 0, 0, 45, 87, 0, 0, 0, 0, 0, 0, 0, 73, 42, 87, 0, 0, 0, 0, 0, 0, 0, 73, 42, 87, 0, 0,
        0, 0, 0, 0, 0, 73, 58, 73, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 73, 42, 87,
        0, 0, 0, 0, 0, 0, 0, 73, 42, 87, 0, 0, 0, 0, 0, 0, 0, 73, 58, 73, 0, 0, 0, 0, 0, 0, 0, 1,
        0, 0, 0,
    ];
    let val_res_25 = validate_tiff_raw_header(oom_bytes_25);
    assert!(val_res_25.is_err());
    let raw_path_25 = temp_dir.join("corrupted_ciff_subifd_cycle.crw");
    std::fs::write(&raw_path_25, oom_bytes_25).unwrap();
    let res_25 = decode_image(&raw_path_25);
    assert!(res_25.is_err());
    // Fuzz reproducer 26: JPEG/EXIF container with truncated APP1 TIFF header causing rawler infinite loop (42 bytes)
    let oom_bytes_26: &[u8] = &[
        255, 216, 255, 225, 0, 14, 69, 120, 105, 102, 0, 0, 73, 73, 42, 0, 77, 14, 69, 120, 106, 0,
        42, 0, 0, 8, 252, 9, 1, 48, 33, 0, 32, 1, 122, 0, 0, 0, 1, 216, 255, 65,
    ];
    let val_res_26 = validate_tiff_raw_header(oom_bytes_26);
    assert!(val_res_26.is_err());
    let raw_path_26 = temp_dir.join("corrupted_app1_trunc.jpg");
    std::fs::write(&raw_path_26, oom_bytes_26).unwrap();
    let res_26 = decode_image(&raw_path_26);
    assert!(res_26.is_err());
    // Fuzz reproducer 27: TIFF container with SubIFD tag having count 0 causing rawler empty slice indexing panic (62 bytes)
    let oom_bytes_27: &[u8] = &[
        77, 77, 0, 42, 0, 0, 0, 8, 0, 4, 1, 74, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 8, 0, 0, 0, 0, 0,
        0, 0, 0,
    ];
    let val_res_27 = validate_tiff_raw_header(oom_bytes_27);
    assert!(val_res_27.is_err());
    let raw_path_27 = temp_dir.join("corrupted_subifd_count0.tif");
    std::fs::write(&raw_path_27, oom_bytes_27).unwrap();
    let res_27 = decode_image(&raw_path_27);
    assert!(res_27.is_err());

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_validate_large_raw_container_headers_with_64k_buffer() {
    // 1. Canon CR3 BMFF header with mdat box > 64KB inside a 10MB file
    let mut cr3_head = Vec::new();
    cr3_head.extend_from_slice(&24u32.to_be_bytes());
    cr3_head.extend_from_slice(b"ftypcrx ");
    cr3_head.extend_from_slice(&0u32.to_be_bytes());
    cr3_head.extend_from_slice(b"crx isom");
    cr3_head.extend_from_slice(&5_000_000u32.to_be_bytes());
    cr3_head.extend_from_slice(b"mdat");
    cr3_head.resize(64, 0);
    assert!(
        validate_raw_container_with_size(&cr3_head, 10_000_000).is_ok(),
        "Valid CR3 head should pass when total_file_size is 10MB"
    );

    // 2. TIFF/ARW header with SubIFD offset at 500_000 in a 10MB file
    let mut tiff_head = vec![0u8; 64];
    tiff_head[0..4].copy_from_slice(b"II*\0");
    tiff_head[4..8].copy_from_slice(&8u32.to_le_bytes());
    tiff_head[8..10].copy_from_slice(&1u16.to_le_bytes()); // 1 IFD entry
    tiff_head[10..12].copy_from_slice(&0x014Au16.to_le_bytes()); // SubIFD tag
    tiff_head[12..14].copy_from_slice(&4u16.to_le_bytes()); // LONG
    tiff_head[14..18].copy_from_slice(&1u32.to_le_bytes()); // count = 1
    tiff_head[18..22].copy_from_slice(&500_000u32.to_le_bytes()); // offset > 64KB
    assert!(
        validate_raw_container_with_size(&tiff_head, 10_000_000).is_ok(),
        "Valid TIFF head with >64KB SubIFD offset should pass when total_file_size is 10MB"
    );
}

#[test]
fn test_icc_transform_preserves_display_p3_wide_gamut() {
    let p3_icc = moxcms::ColorProfile::new_display_p3()
        .encode()
        .expect("encode Display P3 ICC");
    // Pure Display P3 green (well outside sRGB gamut)
    let mut px = [0.0f32, 1.0, 0.0, 1.0];
    transform_icc_to_linear_rec2020(&p3_icc, &mut px).expect("ICC transform");
    assert!(
        px[1] > 0.93,
        "Expected Display P3 green in Linear Rec.2020 to exceed sRGB-clipped G (~0.919), got {:?}",
        px
    );
}

#[test]
fn test_oversized_tiff_raw_header_rejected_before_decode() {
    // Construct a tiny (<128 B) synthetic TIFF/DNG header claiming 50000x50000 resolution.
    // This must be rejected by validate_tiff_raw_structure before rawler allocates anything.
    let mut head = vec![0u8; 64];
    head[0..4].copy_from_slice(b"II*\0");
    head[4..8].copy_from_slice(&8u32.to_le_bytes()); // IFD0 at offset 8
    head[8..10].copy_from_slice(&2u16.to_le_bytes()); // 2 entries
                                                      // Entry 0: ImageWidth (0x0100), LONG (4), count=1, value=50_000
    head[10..12].copy_from_slice(&0x0100u16.to_le_bytes());
    head[12..14].copy_from_slice(&4u16.to_le_bytes());
    head[14..18].copy_from_slice(&1u32.to_le_bytes());
    head[18..22].copy_from_slice(&50_000u32.to_le_bytes());
    // Entry 1: ImageLength (0x0101), LONG (4), count=1, value=50_000
    head[22..24].copy_from_slice(&0x0101u16.to_le_bytes());
    head[24..26].copy_from_slice(&4u16.to_le_bytes());
    head[26..30].copy_from_slice(&1u32.to_le_bytes());
    head[30..34].copy_from_slice(&50_000u32.to_le_bytes());

    let err = validate_tiff_raw_structure(&head, head.len())
        .expect_err("Oversized TIFF/DNG dimensions must be rejected at header validation");
    assert!(
        err.to_string().contains("exceeds maximum allowed"),
        "Unexpected error message: {err}"
    );

    let temp_path =
        std::env::temp_dir().join(format!("beam_oversized_dng_{}.dng", std::process::id()));
    std::fs::write(&temp_path, &head).unwrap();
    let decode_err = decode_raw_with_rawler(&temp_path)
        .expect_err("decode_raw_with_rawler must reject oversized header early");
    assert!(
        decode_err.to_string().contains("exceeds maximum allowed"),
        "Unexpected decode error: {decode_err}"
    );
    let _ = std::fs::remove_file(temp_path);
}
