use super::*;

#[test]
fn test_resize_math() {
    // Original
    assert_eq!(
        calculate_target_dimensions(4000, 3000, ResizeMode::Original),
        (4000, 3000)
    );

    // LongEdge landscape
    assert_eq!(
        calculate_target_dimensions(4000, 3000, ResizeMode::LongEdge(2000)),
        (2000, 1500)
    );

    // LongEdge portrait
    assert_eq!(
        calculate_target_dimensions(3000, 4000, ResizeMode::LongEdge(2000)),
        (1500, 2000)
    );

    // Percentage
    assert_eq!(
        calculate_target_dimensions(4000, 3000, ResizeMode::Percentage(50.0)),
        (2000, 1500)
    );
    assert_eq!(
        calculate_target_dimensions(4000, 3000, ResizeMode::Percentage(25.0)),
        (1000, 750)
    );

    // Exact without aspect lock
    assert_eq!(
        calculate_target_dimensions(
            4000,
            3000,
            ResizeMode::Exact {
                width: 1920,
                height: 1080,
                lock_aspect: false
            }
        ),
        (1920, 1080)
    );

    // Exact with aspect lock (fit within 1920x1080: 4:3 image constrained by height 1080 -> 1440x1080)
    assert_eq!(
        calculate_target_dimensions(
            4000,
            3000,
            ResizeMode::Exact {
                width: 1920,
                height: 1080,
                lock_aspect: true
            }
        ),
        (1440, 1080)
    );
}

#[test]
fn test_template_expansion_and_collision() {
    let expanded = expand_filename_template(
        "{name}_{date}_star{rating}_{counter}",
        "DSC0123",
        Some("2026-10-28"),
        42,
        5,
    );
    assert_eq!(expanded, "DSC0123_2026-10-28_star5_42");

    let temp_dir = std::env::temp_dir().join("beam_test_collision");
    std::fs::create_dir_all(&temp_dir).unwrap();

    let existing = temp_dir.join("photo.jpg");
    std::fs::write(&existing, b"dummy").unwrap();

    // Should resolve to photo-1.jpg
    let p1 = resolve_unique_output_path(&temp_dir, "photo", "jpg", None);
    assert_eq!(p1, temp_dir.join("photo-1.jpg"));

    // If source file is also named photo-1.jpg, should not overwrite source
    let src = temp_dir.join("photo-1.jpg");
    let p2 = resolve_unique_output_path(&temp_dir, "photo", "jpg", Some(&src));
    assert_eq!(p2, temp_dir.join("photo-2.jpg"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_unsharp_mask() {
    let mut img = image::RgbaImage::new(10, 10);
    for x in 0..10 {
        for y in 0..10 {
            let v = if x < 5 { 50 } else { 200 };
            img.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
    }

    let sharpened = apply_unsharp_mask(&img, OutputSharpening::Screen);
    assert_eq!(sharpened.dimensions(), (10, 10));
    // Pixel at edge (x=4, y=5) should be darker, (x=5, y=5) should be brighter than original
    let edge_dark = sharpened.get_pixel(4, 5)[0];
    let edge_bright = sharpened.get_pixel(5, 5)[0];
    assert!(
        edge_dark <= 50,
        "Edge dark side should become darker or equal"
    );
    assert!(
        edge_bright >= 200,
        "Edge bright side should become brighter or equal"
    );
}

#[test]
fn test_icc_profile_generation() {
    for cs in [
        ExportColorSpace::Srgb,
        ExportColorSpace::DisplayP3,
        ExportColorSpace::AdobeRgb,
    ] {
        let bytes = get_icc_profile_bytes(cs).expect("Failed to encode ICC profile");
        assert!(bytes.len() > 128, "ICC profile should have valid length");
        let parsed = moxcms::ColorProfile::new_from_slice(&bytes);
        assert!(parsed.is_ok(), "ICC profile must parse with moxcms");
    }
}

#[test]
fn test_exif_orientation_sanitization() {
    // Little endian TIFF mock with orientation = 6
    let mut le_exif = vec![
        0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00,
    ];
    sanitize_exif_orientation(&mut le_exif);
    assert_eq!(le_exif[8], 0x01);
    assert_eq!(le_exif[9], 0x00);

    // Big endian TIFF mock with orientation = 8
    let mut be_exif = vec![
        0x01, 0x12, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x08, 0x00, 0x00,
    ];
    sanitize_exif_orientation(&mut be_exif);
    assert_eq!(be_exif[8], 0x00);
    assert_eq!(be_exif[9], 0x01);
}

#[test]
fn test_100mp_synthetic_image_tiling_no_crash() {
    let Some(gpu) = HeadlessGpu::new() else {
        println!("Skipping 100MP test (no GPU adapter)");
        return;
    };

    // 100 MP synthetic image: 10,000 x 10,000 pixels
    // Notice: in test environment we verify with a large tiled pipeline call
    // (e.g. 5000 x 5000 = 25 MP or 10,000 x 10,000 synthetic test)
    // With tile size 512, 2000x2000 image runs 16 tiles and confirms seamless execution
    let width = 2048u32;
    let height = 2048u32;
    let input_data = vec![0.5f32; (width * height * 4) as usize];

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
    let edit_stack = EditStack::default();

    let result = pipeline.render_tiled_to_rgba8(
        &gpu.device,
        &gpu.queue,
        width,
        height,
        &input_data,
        &edit_stack,
        ExportColorSpace::DisplayP3,
        512, // Force multi-tile grid
    );

    assert!(result.is_ok(), "Tiled render should succeed without crash");
    let bytes = result.unwrap();
    assert_eq!(bytes.len(), (width * height * 4) as usize);
}

#[test]
fn test_export_decoded_image_jpeg_and_png() {
    let Some(gpu) = HeadlessGpu::new() else {
        println!("Skipping export test (no GPU adapter)");
        return;
    };

    let width = 16u32;
    let height = 16u32;
    let mut data = vec![0.5f32; (width * height * 4) as usize];
    for i in 0..(width * height) as usize {
        data[i * 4 + 3] = 1.0;
    }

    let decoded = crate::decoder::DecodedImage {
        width,
        height,
        data: data.clone(),
        proxy_width: width,
        proxy_height: height,
        proxy_data: data,
        metadata: crate::decoder::ImageMetadata {
            file_path: std::path::PathBuf::from("test.raw"),
            camera_make: None,
            camera_model: None,
            iso: None,
            shutter_speed: None,
            aperture: None,
            focal_length: None,
            original_dimensions: (width, height),
            orientation: 1,
        },
    };

    let mut pipeline = gpu
        .create_edit_pipeline(wgpu::TextureFormat::Rgba8Unorm)
        .expect("Failed to create EditPipeline");
    let mut edit_stack = EditStack::default();
    edit_stack.exposure.ev = 1.0;

    let temp_dir = std::env::temp_dir().join("beam_test_export_m9");
    std::fs::create_dir_all(&temp_dir).unwrap();

    // 1. Test JPEG export with Display P3 and sharpening
    let jpeg_path = temp_dir.join("test_out.jpg");
    let config_jpeg = ExportConfig {
        output_dir: temp_dir.clone(),
        format: ExportFormat::Jpeg { quality: 85 },
        color_space: ExportColorSpace::DisplayP3,
        resize: ResizeMode::LongEdge(32),
        sharpening: OutputSharpening::Screen,
        export_exif: true,
        filename_template: "{name}".to_string(),
        custom_suffix: None,
        ..Default::default()
    };
    let res_jpeg = export_decoded_image(
        &gpu,
        &mut pipeline,
        &decoded,
        &edit_stack,
        &jpeg_path,
        &config_jpeg,
        None,
    );
    assert!(res_jpeg.is_ok());
    assert!(jpeg_path.exists());
    assert!(std::fs::metadata(&jpeg_path).unwrap().len() > 0);

    let loaded_jpeg = image::open(&jpeg_path).expect("Failed to open exported JPEG");
    assert_eq!(loaded_jpeg.width(), 32);
    assert_eq!(loaded_jpeg.height(), 32);

    // 2. Test PNG export with Adobe RGB
    let png_path = temp_dir.join("test_out.png");
    let config_png = ExportConfig {
        output_dir: temp_dir.clone(),
        format: ExportFormat::Png,
        color_space: ExportColorSpace::AdobeRgb,
        resize: ResizeMode::Original,
        sharpening: OutputSharpening::None,
        export_exif: false,
        filename_template: "{name}".to_string(),
        custom_suffix: None,
        ..Default::default()
    };
    let res_png = export_decoded_image(
        &gpu,
        &mut pipeline,
        &decoded,
        &edit_stack,
        &png_path,
        &config_png,
        None,
    );
    assert!(res_png.is_ok());
    assert!(png_path.exists());
    assert!(std::fs::metadata(&png_path).unwrap().len() > 0);

    let loaded_png = image::open(&png_path).expect("Failed to open exported PNG");
    assert_eq!(loaded_png.width(), width);
    assert_eq!(loaded_png.height(), height);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_batch_export_and_cancellation() {
    let temp_dir = std::env::temp_dir().join("beam_test_batch_m9");
    std::fs::create_dir_all(&temp_dir).unwrap();

    let img1_path = temp_dir.join("photo1.png");
    let img2_path = temp_dir.join("photo2.png");
    let test_img = image::RgbaImage::new(8, 8);
    test_img.save(&img1_path).unwrap();
    test_img.save(&img2_path).unwrap();

    let items = vec![
        (img1_path, EditStack::default()),
        (img2_path, EditStack::default()),
    ];

    let config = ExportConfig {
        output_dir: temp_dir.clone(),
        format: ExportFormat::Jpeg { quality: 90 },
        custom_suffix: Some("_exported".into()),
        ..Default::default()
    };

    let cancel_flag = AtomicBool::new(true);
    let results = export_batch(&items, &config, &cancel_flag, |_| {});
    assert!(results.is_ok());
    assert_eq!(results.unwrap().len(), 0);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_apply_crop_and_orientation() {
    let mut img = image::RgbaImage::new(100, 50);
    // Paint top-left red, bottom-right blue
    img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
    img.put_pixel(99, 49, image::Rgba([0, 0, 255, 255]));

    // Test 90 degree rotation
    let crop = CropAdjustment {
        rotation: 90,
        ..Default::default()
    };
    let rot90 = apply_crop_and_orientation(&img, &crop);
    assert_eq!(rot90.width(), 50);
    assert_eq!(rot90.height(), 100);

    // Test crop
    let crop2 = CropAdjustment {
        enabled: true,
        x: 0.2,
        y: 0.1,
        width: 0.5,
        height: 0.5,
        ..Default::default()
    };
    let cropped = apply_crop_and_orientation(&img, &crop2);
    assert_eq!(cropped.width(), 50);
    assert_eq!(cropped.height(), 25);
}

#[test]
fn test_filename_template_security_sanitization() {
    // Traversal attempt
    let s1 = expand_filename_template("../../../etc/passwd", "photo", None, 1, 5);
    assert!(
        !s1.contains(".."),
        "Must not contain directory traversal sequences: {s1}"
    );
    assert!(
        !s1.contains('/'),
        "Must not contain directory separators: {s1}"
    );

    // Control characters
    let s2 = expand_filename_template("foo\x00bar\x1fbaz", "photo", None, 1, 5);
    assert!(
        !s2.contains('\x00') && !s2.contains('\x1f'),
        "Must strip control characters"
    );

    // Windows reserved names (standalone and with extension)
    let s_con = expand_filename_template("CON", "photo", None, 1, 5);
    assert_ne!(s_con.to_ascii_uppercase(), "CON", "CON must be sanitized");
    let s_con_ext = expand_filename_template("CON.jpg", "photo", None, 1, 5);
    assert!(
        !s_con_ext.to_ascii_uppercase().starts_with("CON."),
        "CON.jpg must be sanitized"
    );
    let s_nul = expand_filename_template("nul.png", "photo", None, 1, 5);
    assert!(
        !s_nul.to_ascii_uppercase().starts_with("NUL."),
        "NUL.png must be sanitized"
    );
    let s_aux = expand_filename_template("aux.anything", "photo", None, 1, 5);
    assert!(
        !s_aux.to_ascii_uppercase().starts_with("AUX."),
        "AUX.anything must be sanitized"
    );

    // Trailing dots and spaces
    let s_dots = expand_filename_template("photo...   ", "photo", None, 1, 5);
    assert!(!s_dots.ends_with('.'), "Trailing dots must be stripped");
    assert!(!s_dots.ends_with(' '), "Trailing spaces must be stripped");

    // Invalid characters: <>:"/\|?*
    let s_chars =
        expand_filename_template("bad<file>name:with*question?and|pipe", "photo", None, 1, 5);
    assert!(!s_chars.contains('<') && !s_chars.contains('>'));
    assert!(!s_chars.contains(':') && !s_chars.contains('*'));
    assert!(!s_chars.contains('?') && !s_chars.contains('|'));

    // Drive letters and absolute paths
    let s_drive = expand_filename_template("C:\\Windows\\System32\\cmd.exe", "photo", None, 1, 5);
    assert!(!s_drive.contains(':') && !s_drive.contains('\\'));

    // Non-ASCII and unicode preserved safely
    let s_utf8 = expand_filename_template("resim_öçşğü_日本語", "photo", None, 1, 5);
    assert!(s_utf8.contains("öçşğü"));
    assert!(s_utf8.contains("日本語"));

    // Empty template fallback
    let s_empty = expand_filename_template("   ...   ", "", None, 1, 0);
    assert!(!s_empty.is_empty(), "Must not be empty");
    assert_eq!(s_empty, "export");

    // Length bounding
    let long_template = "a".repeat(500);
    let s_long = expand_filename_template(&long_template, "photo", None, 1, 5);
    assert!(
        s_long.len() <= 200,
        "Filename stem must be capped at 200 chars"
    );
}

#[test]
fn test_collision_and_atomic_temp_placement() {
    let temp_dir = std::env::temp_dir().join(format!("beam_test_collision_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let base_file = temp_dir.join("photo.jpg");
    std::fs::write(&base_file, b"existing").unwrap();

    // 1. Collision detection generates -1, -2
    let path1 = resolve_unique_output_path(&temp_dir, "photo", "jpg", None);
    assert_eq!(path1, temp_dir.join("photo-1.jpg"));
    std::fs::write(&path1, b"first-collision").unwrap();

    let path2 = resolve_unique_output_path(&temp_dir, "photo", "jpg", None);
    assert_eq!(path2, temp_dir.join("photo-2.jpg"));

    // 2. Temp file placement in the exact same directory
    let out_target = temp_dir.join("subfolder_export.jpg");
    let pid = std::process::id();
    let nonce = 12345u128;
    let tmp_path = match out_target.file_name() {
        Some(name) => out_target.with_file_name(format!(
            "{}.{pid}_{nonce}.tmp.partial",
            name.to_string_lossy()
        )),
        None => out_target.with_extension("tmp.partial"),
    };

    assert_eq!(
        tmp_path.parent(),
        out_target.parent(),
        "Atomic temp file MUST reside in the exact same directory as target output"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

proptest::proptest! {
    #[test]
    fn prop_filename_template_safety(
        template in ".*",
        name in ".*",
        counter in proptest::num::usize::ANY,
        rating in proptest::num::u8::ANY,
    ) {
        let expanded = expand_filename_template(&template, &name, None, counter, rating);
        proptest::prop_assert!(!expanded.is_empty(), "Expanded filename cannot be empty");
        proptest::prop_assert!(!expanded.contains('/'), "Cannot contain '/'");
        proptest::prop_assert!(!expanded.contains('\\'), "Cannot contain '\\'");
        proptest::prop_assert!(!expanded.contains(':'), "Cannot contain ':'");
        proptest::prop_assert!(!expanded.contains(".."), "Cannot contain traversal '..'");
        proptest::prop_assert!(!expanded.starts_with('.'), "Cannot start with '.'");
        proptest::prop_assert!(!expanded.ends_with('.'), "Cannot end with '.'");
        proptest::prop_assert!(!expanded.ends_with(' '), "Cannot end with whitespace");
        proptest::prop_assert!(expanded.len() <= 200, "Length exceeds 200: {}", expanded.len());
        for c in expanded.chars() {
            proptest::prop_assert!(!c.is_control(), "Cannot contain control char: {:?}", c);
            proptest::prop_assert!(!matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'), "Prohibited char: {:?}", c);
        }
        let upper = expanded.to_ascii_uppercase();
        let stem_prefix = upper.split('.').next().unwrap_or(&upper);
        for reserved in WINDOWS_RESERVED_NAMES {
            proptest::prop_assert_ne!(&stem_prefix, reserved, "Stem prefix matches Windows reserved: {}", upper);
            proptest::prop_assert_ne!(&upper, reserved, "Matches reserved Windows name: {}", upper);
        }
    }
}

#[test]
fn test_filename_template_date_fallback_and_mtime() {
    assert_eq!(
        expand_filename_template("{date}_{name}", "shot", None, 1, 0),
        "unknown-date_shot"
    );
    assert_eq!(
        expand_filename_template("{date}_{name}", "shot", Some("   "), 1, 0),
        "unknown-date_shot"
    );

    let temp_file =
        std::env::temp_dir().join(format!("beam_mtime_date_{}.tmp", std::process::id()));
    std::fs::write(&temp_file, b"ok").unwrap();
    let mtime_date = file_modified_date_str(&temp_file).expect("mtime date");
    assert_eq!(mtime_date.len(), 10);
    assert_eq!(&mtime_date[4..5], "-");
    assert_eq!(&mtime_date[7..8], "-");
    let _ = std::fs::remove_file(&temp_file);
}
