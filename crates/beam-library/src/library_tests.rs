use super::*;
use image::{ImageBuffer, Rgba};

#[test]
fn test_library_db_in_memory_migration() {
    let db = LibraryDb::open_in_memory().expect("Failed to open in-memory db");
    let count: i64 = db
        .connection()
        .query_row("SELECT count(*) FROM photos", [], |row| row.get(0))
        .expect("Failed to query photos");
    assert_eq!(count, 0);
}

#[test]
fn test_library_import_and_filtering() {
    let db = LibraryDb::open_in_memory().expect("Failed to open db");
    let temp_dir = std::env::temp_dir().join("beam_library_test");
    let _ = fs::create_dir_all(&temp_dir);

    let img_path1 = temp_dir.join("photo1.png");
    let img_path2 = temp_dir.join("photo2.png");
    let thumb_dir = temp_dir.join("thumbs");

    let img1: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(100, 100, Rgba([200, 50, 50, 255]));
    img1.save(&img_path1).unwrap();

    let img2: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(100, 100, Rgba([50, 200, 50, 255]));
    img2.save(&img_path2).unwrap();

    let p1 = db.import_file(&img_path1, &thumb_dir).unwrap();
    let p2 = db.import_file(&img_path2, &thumb_dir).unwrap();

    // Update ratings and labels
    db.update_rating(p1.id, 4).unwrap();
    db.update_color_label(p1.id, ColorLabel::Red).unwrap();

    db.update_rating(p2.id, 2).unwrap();
    db.update_color_label(p2.id, ColorLabel::Green).unwrap();

    // Filter: min 3 stars
    let filter_stars = LibraryFilter {
        min_rating: 3,
        ..Default::default()
    };
    let res_stars = db.query_photos(&filter_stars).unwrap();
    assert_eq!(res_stars.len(), 1);
    assert_eq!(res_stars[0].id, p1.id);

    // Filter: color label Green
    let filter_green = LibraryFilter {
        color_label: Some(ColorLabel::Green),
        ..Default::default()
    };
    let res_green = db.query_photos(&filter_green).unwrap();
    assert_eq!(res_green.len(), 1);
    assert_eq!(res_green[0].id, p2.id);

    // Filter: search query "photo1"
    let filter_search = LibraryFilter {
        search_query: Some("photo1".to_string()),
        ..Default::default()
    };
    let res_search = db.query_photos(&filter_search).unwrap();
    assert_eq!(res_search.len(), 1);
    assert_eq!(res_search[0].id, p1.id);

    // Cleanup
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_library_thumbnail_updates() {
    let db = LibraryDb::open_in_memory().expect("Failed to open db");
    let temp_dir = std::env::temp_dir().join("beam_library_thumb_test");
    let _ = fs::create_dir_all(&temp_dir);

    let img_path = temp_dir.join("photo.png");
    let thumb_dir = temp_dir.join("thumbs");

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(50, 50, Rgba([100, 100, 100, 255]));
    img.save(&img_path).unwrap();

    let photo = db.import_file(&img_path, &thumb_dir).unwrap();
    assert!(photo.thumbnail_path.is_some());

    // Update thumbnail path
    db.update_thumbnail_path(photo.id, "/new/thumb/path.jpg")
        .unwrap();
    let updated = db.get_photo(photo.id).unwrap().unwrap();
    assert_eq!(
        updated.thumbnail_path.as_deref(),
        Some("/new/thumb/path.jpg")
    );

    // Clear all thumbnails
    db.clear_all_thumbnails().unwrap();
    let cleared = db.get_photo(photo.id).unwrap().unwrap();
    assert_eq!(cleared.thumbnail_path, None);

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_library_file_type_filtering() {
    let db = LibraryDb::open_in_memory().expect("Failed to open db");
    let temp_dir = std::env::temp_dir().join("beam_library_ft_test");
    let _ = fs::create_dir_all(&temp_dir);

    let p_png = temp_dir.join("photo.png");
    let p_jpg = temp_dir.join("photo.jpg");
    let p_webp = temp_dir.join("photo.webp");
    let thumb_dir = temp_dir.join("thumbs");

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(10, 10, Rgba([100, 100, 100, 255]));
    img.save(&p_png).unwrap();
    image::DynamicImage::ImageRgba8(img.clone())
        .to_rgb8()
        .save(&p_jpg)
        .unwrap();
    img.save(&p_webp).unwrap();

    db.import_file(&p_png, &thumb_dir).unwrap();
    db.import_file(&p_jpg, &thumb_dir).unwrap();
    db.import_file(&p_webp, &thumb_dir).unwrap();

    // 1. All
    let all = db.query_photos(&LibraryFilter::default()).unwrap();
    assert_eq!(all.len(), 3);

    // 2. PNG only
    let filter_png = LibraryFilter {
        file_type: FileTypeFilter::Png,
        ..Default::default()
    };
    let png_only = db.query_photos(&filter_png).unwrap();
    assert_eq!(png_only.len(), 1);
    assert!(png_only[0].file_name.ends_with(".png"));

    // 3. JPEG only
    let filter_jpg = LibraryFilter {
        file_type: FileTypeFilter::Jpeg,
        ..Default::default()
    };
    let jpg_only = db.query_photos(&filter_jpg).unwrap();
    assert_eq!(jpg_only.len(), 1);
    assert!(jpg_only[0].file_name.ends_with(".jpg"));

    // 4. WebP only
    let filter_webp = LibraryFilter {
        file_type: FileTypeFilter::WebP,
        ..Default::default()
    };
    let webp_only = db.query_photos(&filter_webp).unwrap();
    assert_eq!(webp_only.len(), 1);
    assert!(webp_only[0].file_name.ends_with(".webp"));

    // 5. RAW only
    let filter_raw = LibraryFilter {
        file_type: FileTypeFilter::Raw,
        ..Default::default()
    };
    let raw_only = db.query_photos(&filter_raw).unwrap();
    assert_eq!(raw_only.len(), 0);

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_schema_version_validation() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        r#"
            CREATE TABLE schema_version (version INTEGER PRIMARY KEY);
            INSERT INTO schema_version (version) VALUES (999);
            "#,
    )
    .unwrap();

    let db = LibraryDb { conn };
    let err = db.migrate();
    assert!(err.is_err(), "Must reject future schema versions");
    let msg = err.err().unwrap().to_string();
    assert!(msg.contains("Unsupported database schema version 999"));
}

#[test]
fn test_scan_folder_symlink_cycle_protection() {
    let temp_dir = std::env::temp_dir().join("beam_library_symlink_test");
    let _ = fs::remove_dir_all(&temp_dir);
    let sub_dir = temp_dir.join("sub");
    fs::create_dir_all(&sub_dir).unwrap();

    let img_path = temp_dir.join("test.png");
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(10, 10, Rgba([10, 20, 30, 255]));
    img.save(&img_path).unwrap();

    #[cfg(unix)]
    {
        let loop_link = sub_dir.join("cycle_link");
        let _ = std::os::unix::fs::symlink(&temp_dir, &loop_link);
    }

    let db = LibraryDb::open_in_memory().unwrap();
    let thumb_dir = temp_dir.join("thumbs");
    let count = db
        .scan_and_import_folder(&temp_dir, &thumb_dir, true)
        .unwrap();
    assert_eq!(
        count, 1,
        "Must import exactly 1 image despite symlink cycle"
    );

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_sql_injection_and_metacharacters() {
    let db = LibraryDb::open_in_memory().unwrap();
    let temp_dir = std::env::temp_dir().join("beam_library_sqli_test");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let img_path = temp_dir.join("target.png");
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_pixel(10, 10, Rgba([0, 0, 0, 255]));
    img.save(&img_path).unwrap();

    let thumb_dir = temp_dir.join("thumbs");
    db.import_file(&img_path, &thumb_dir).unwrap();

    // 1. Classic SQL injection attempt
    let sqli_filter = LibraryFilter {
        search_query: Some("'; DROP TABLE photos; --".into()),
        ..Default::default()
    };
    let res = db.query_photos(&sqli_filter).unwrap();
    assert_eq!(res.len(), 0, "Injection search must not match anything");

    // Verify photos table is still intact and readable
    let count_filter = LibraryFilter::default();
    assert_eq!(
        db.query_photos(&count_filter).unwrap().len(),
        1,
        "Table must not be dropped!"
    );

    // 2. OR injection attempt
    let or_filter = LibraryFilter {
        search_query: Some("' OR '1'='1".into()),
        ..Default::default()
    };
    let res_or = db.query_photos(&or_filter).unwrap();
    assert_eq!(
        res_or.len(),
        0,
        "Must treat search literally, not boolean OR"
    );

    // 3. Metacharacter % (must be escaped as literal, not wildcard)
    let wildcard_filter = LibraryFilter {
        search_query: Some("%".into()),
        ..Default::default()
    };
    let res_wc = db.query_photos(&wildcard_filter).unwrap();
    assert_eq!(res_wc.len(), 0, "Literal % must not act as SQL wildcard");

    // 4. Very long search query (10,000 characters)
    let long_query = "x".repeat(10_000);
    let long_filter = LibraryFilter {
        search_query: Some(long_query),
        ..Default::default()
    };
    let res_long = db.query_photos(&long_filter).unwrap();
    assert_eq!(res_long.len(), 0, "Oversized query handled safely");

    // 5. Unicode and special characters
    let unicode_filter = LibraryFilter {
        search_query: Some("öçşğü_日本語_Ω".into()),
        ..Default::default()
    };
    let res_uni = db.query_photos(&unicode_filter).unwrap();
    assert_eq!(res_uni.len(), 0);

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_corrupted_database_handling() {
    let temp_dir = std::env::temp_dir().join("beam_library_corrupt_test");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let corrupt_db_path = temp_dir.join("corrupted.db");
    // Write invalid garbage bytes that do not form a valid SQLite database header
    fs::write(
        &corrupt_db_path,
        b"NOT_A_SQLITE_DATABASE_INVALID_GARBAGE_BYTES_12345",
    )
    .unwrap();

    let res = LibraryDb::open(&corrupt_db_path);
    assert!(res.is_err(), "Must return error on corrupted database file");
    match res {
        Err(BeamError::Database(msg)) => {
            assert!(!msg.is_empty(), "Error message must be present");
        }
        Err(e) => panic!("Expected BeamError::Database, got {e:?}"),
        Ok(_) => panic!("Expected error, but database opened successfully"),
    }

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_locked_database_handling() {
    let temp_dir = std::env::temp_dir().join("beam_library_lock_test");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let db_path = temp_dir.join("library.db");
    let db1 = LibraryDb::open(&db_path).unwrap();

    // Lock database exclusively with db1
    db1.conn.execute("BEGIN EXCLUSIVE", []).unwrap();

    // Open second connection and try to write; must receive busy/locked error without panic
    let db2 = Connection::open(&db_path).unwrap();
    let res = db2.execute("INSERT INTO schema_version (version) VALUES (2)", []);
    assert!(
        res.is_err(),
        "Second connection must fail gracefully when locked"
    );

    // Rollback first connection to release lock cleanly
    db1.conn.execute("ROLLBACK", []).unwrap();

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_symlinks_outside_folder_skipped() {
    let temp_dir = std::env::temp_dir().join("beam_library_symlink_ext_test");
    let _ = fs::remove_dir_all(&temp_dir);
    let lib_dir = temp_dir.join("library_root");
    let ext_dir = temp_dir.join("external_dir");
    fs::create_dir_all(&lib_dir).unwrap();
    fs::create_dir_all(&ext_dir).unwrap();

    let outside_img = ext_dir.join("secret.png");
    let img1: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(10, 10, Rgba([1, 1, 1, 255]));
    img1.save(&outside_img).unwrap();

    let inside_img = lib_dir.join("normal.png");
    let img2: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(10, 10, Rgba([2, 2, 2, 255]));
    img2.save(&inside_img).unwrap();

    #[cfg(unix)]
    {
        let symlink_path = lib_dir.join("link_to_outside.png");
        let _ = std::os::unix::fs::symlink(&outside_img, &symlink_path);
    }

    let db = LibraryDb::open_in_memory().unwrap();
    let thumb_dir = temp_dir.join("thumbs");
    let count = db
        .scan_and_import_folder(&lib_dir, &thumb_dir, true)
        .unwrap();

    // Must import exactly 1 image (normal.png); the external symlink must be skipped
    assert_eq!(
        count, 1,
        "Must skip symlinks resolving outside the library root folder"
    );

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_deeply_nested_directory_tree() {
    let temp_dir = std::env::temp_dir().join("beam_library_depth_test");
    let _ = fs::remove_dir_all(&temp_dir);

    let mut current = temp_dir.clone();
    for i in 0..40 {
        current = current.join(format!("level_{i}"));
    }
    fs::create_dir_all(&current).unwrap();

    // Place an image at depth 10
    let mut depth_10 = temp_dir.clone();
    for i in 0..10 {
        depth_10 = depth_10.join(format!("level_{i}"));
    }
    let img_path = depth_10.join("photo.png");
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_pixel(10, 10, Rgba([5, 5, 5, 255]));
    img.save(&img_path).unwrap();

    let db = LibraryDb::open_in_memory().unwrap();
    let thumb_dir = temp_dir.join("thumbs");
    let count = db
        .scan_and_import_folder(&temp_dir, &thumb_dir, true)
        .unwrap();
    assert_eq!(count, 1, "Must scan deep tree without stack overflow");

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_photo_path_update_delete_and_relink() {
    let temp_dir = std::env::temp_dir().join("beam_library_relink_test");
    let _ = fs::remove_dir_all(&temp_dir);
    let old_folder = temp_dir.join("old_desktop_folder");
    let new_folder = temp_dir.join("new_desktop_folder");
    fs::create_dir_all(&old_folder).unwrap();
    fs::create_dir_all(&new_folder).unwrap();

    let img_path1 = old_folder.join("image_a.png");
    let img_path2 = old_folder.join("image_b.png");
    let thumb_dir = temp_dir.join("thumbs");

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(10, 10, Rgba([10, 20, 30, 255]));
    img.save(&img_path1).unwrap();
    img.save(&img_path2).unwrap();

    let db = LibraryDb::open_in_memory().unwrap();
    let p1 = db.import_file(&img_path1, &thumb_dir).unwrap();
    let p2 = db.import_file(&img_path2, &thumb_dir).unwrap();

    assert_eq!(db.query_photos(&LibraryFilter::default()).unwrap().len(), 2);

    // 1. Move image_a to new location and test update_photo_path
    let relocated_a = new_folder.join("image_a_renamed.png");
    fs::rename(&img_path1, &relocated_a).unwrap();
    db.update_photo_path(p1.id, &relocated_a).unwrap();

    let updated_p1 = db.get_photo(p1.id).unwrap().unwrap();
    assert_eq!(updated_p1.file_path, relocated_a.to_string_lossy());
    assert_eq!(updated_p1.file_name, "image_a_renamed.png");

    // 2. Move image_b to new_folder/image_b.png and test relink_folder
    let moved_b = new_folder.join("image_b.png");
    fs::rename(&img_path2, &moved_b).unwrap();
    let remapped_count = db.relink_folder(&old_folder, &new_folder).unwrap();
    assert_eq!(remapped_count, 1);

    let updated_p2 = db.get_photo(p2.id).unwrap().unwrap();
    assert_eq!(updated_p2.file_path, moved_b.to_string_lossy());
    assert!(Path::new(&updated_p2.file_path).exists());

    // 3. Test remove_missing_photos
    // Delete image_b from disk so it's missing
    fs::remove_file(&moved_b).unwrap();
    let missing_removed = db.remove_missing_photos().unwrap();
    assert_eq!(missing_removed, 1);
    assert_eq!(db.query_photos(&LibraryFilter::default()).unwrap().len(), 1);

    // 4. Test delete_photo
    db.delete_photo(p1.id).unwrap();
    assert_eq!(db.query_photos(&LibraryFilter::default()).unwrap().len(), 0);

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_list_subdirectories_and_folder_filter() {
    let temp_dir = std::env::temp_dir().join("beam_library_folder_browse_test");
    let _ = fs::remove_dir_all(&temp_dir);
    let root = temp_dir.join("photos_root");
    let sub_a = root.join("vacation");
    let sub_b = root.join("portraits");
    let hidden = root.join(".hidden_folder");
    fs::create_dir_all(&sub_a).unwrap();
    fs::create_dir_all(&sub_b).unwrap();
    fs::create_dir_all(&hidden).unwrap();

    let subdirs = list_subdirectories(&root);
    assert_eq!(subdirs.len(), 2);
    assert_eq!(subdirs[0].file_name().unwrap(), "portraits");
    assert_eq!(subdirs[1].file_name().unwrap(), "vacation");

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(10, 10, Rgba([10, 20, 30, 255]));
    let file_root = root.join("root_photo.png");
    let file_sub = sub_a.join("beach.png");
    img.save(&file_root).unwrap();
    img.save(&file_sub).unwrap();

    let thumb_dir = temp_dir.join("thumbs");
    let db = LibraryDb::open_in_memory().unwrap();
    db.import_file(&file_root, &thumb_dir).unwrap();
    db.import_file(&file_sub, &thumb_dir).unwrap();

    // 1. Filter non-recursive root
    let filter_non_rec = LibraryFilter {
        folder_path: Some(root.clone()),
        recursive: false,
        ..Default::default()
    };
    let res_non_rec = db.query_photos(&filter_non_rec).unwrap();
    assert_eq!(res_non_rec.len(), 1);
    assert_eq!(res_non_rec[0].file_name, "root_photo.png");

    // 2. Filter recursive root
    let filter_rec = LibraryFilter {
        folder_path: Some(root.clone()),
        recursive: true,
        ..Default::default()
    };
    let res_rec = db.query_photos(&filter_rec).unwrap();
    assert_eq!(res_rec.len(), 2);

    // 3. Filter subfolder
    let filter_sub = LibraryFilter {
        folder_path: Some(sub_a.clone()),
        recursive: false,
        ..Default::default()
    };
    let res_sub = db.query_photos(&filter_sub).unwrap();
    assert_eq!(res_sub.len(), 1);
    assert_eq!(res_sub[0].file_name, "beach.png");

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_photo_tags_and_filtering() {
    let temp_dir = std::env::temp_dir().join(format!("beam_tag_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(10, 10, Rgba([50, 100, 150, 255]));
    let f1 = temp_dir.join("sunset.png");
    let f2 = temp_dir.join("portrait.png");
    img.save(&f1).unwrap();
    img.save(&f2).unwrap();

    let thumb_dir = temp_dir.join("thumbs");
    let db = LibraryDb::open_in_memory().unwrap();
    let id1 = db.import_file(&f1, &thumb_dir).unwrap().id;
    let id2 = db.import_file(&f2, &thumb_dir).unwrap().id;

    db.add_tag(id1, "landscape").unwrap();
    db.add_tag(id1, "golden-hour").unwrap();
    db.add_tag(id1, "  landscape  ").unwrap(); // duplicate should be ignored
    db.add_tag(id2, "portrait").unwrap();

    let tags1 = db.get_photo_tags(id1).unwrap();
    assert_eq!(tags1, vec!["golden-hour", "landscape"]);

    let all_tags = db.list_all_tags().unwrap();
    assert_eq!(
        all_tags,
        vec![
            ("golden-hour".to_string(), 1),
            ("landscape".to_string(), 1),
            ("portrait".to_string(), 1),
        ]
    );

    let filter = LibraryFilter {
        tag: Some("landscape".to_string()),
        ..Default::default()
    };
    let matched = db.query_photos(&filter).unwrap();
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].id, id1);

    db.remove_tag(id1, "landscape").unwrap();
    assert_eq!(db.get_photo_tags(id1).unwrap(), vec!["golden-hour"]);
    let matched_after = db.query_photos(&filter).unwrap();
    assert!(matched_after.is_empty());

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_smart_collections_metadata_and_duplicates() {
    let temp_dir = std::env::temp_dir().join(format!("beam_phase3_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(12, 12, Rgba([80, 120, 160, 255]));
    let f1 = temp_dir.join("orig.png");
    let f2 = temp_dir.join("copy.png");
    img.save(&f1).unwrap();
    fs::copy(&f1, &f2).unwrap();

    let thumb_dir = temp_dir.join("thumbs");
    let db = LibraryDb::open_in_memory().unwrap();
    let id1 = db.import_file(&f1, &thumb_dir).unwrap().id;
    let _id2 = db.import_file(&f2, &thumb_dir).unwrap().id;

    // 1. Smart Collections
    let filter = LibraryFilter {
        min_rating: 4,
        file_type: FileTypeFilter::Png,
        tag: Some("best".to_string()),
        ..Default::default()
    };
    let sc_id = db.save_smart_collection("Top PNGs", &filter).unwrap();
    let cols = db.list_smart_collections().unwrap();
    assert_eq!(cols.len(), 1);
    assert_eq!(cols[0].name, "Top PNGs");
    assert_eq!(cols[0].min_rating, 4);
    assert_eq!(cols[0].file_type, FileTypeFilter::Png);
    assert_eq!(cols[0].tag.as_deref(), Some("best"));
    db.delete_smart_collection(sc_id).unwrap();
    assert!(db.list_smart_collections().unwrap().is_empty());

    // 2. IPTC/XMP Metadata + GPS
    let meta = PhotoMetadata {
        title: "Mountain Dawn".into(),
        caption: "Alpenglow over ridge".into(),
        copyright: "2026 beaM".into(),
        creator: "Photographer".into(),
        gps_lat: Some(46.5775),
        gps_lon: Some(7.9052),
    };
    db.set_photo_metadata(id1, &meta).unwrap();
    assert_eq!(db.get_photo_metadata(id1).unwrap(), meta);

    // 3. Duplicate Detection
    let dups = db.find_duplicates().unwrap();
    assert_eq!(dups.len(), 1);
    assert_eq!(dups[0].len(), 2);

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_xmp_sidecar_export_escapes_xml() {
    let temp_dir = std::env::temp_dir().join(format!("beam_xmp_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let photo = temp_dir.join("sample.dng");
    let meta = PhotoMetadata {
        title: "Dawn & <Ridge>".into(),
        caption: "Line \"1\" '2'".into(),
        copyright: "2026 beaM".into(),
        creator: "M. Author".into(),
        gps_lat: None,
        gps_lon: None,
    };
    let tags = vec!["landscape".to_string(), "raw<test>".to_string()];
    let xmp_path = write_xmp_sidecar(&photo, &meta, 5, ColorLabel::Green, &tags).unwrap();
    assert_eq!(xmp_path, temp_dir.join("sample.xmp"));

    let content = fs::read_to_string(&xmp_path).unwrap();
    assert!(content.contains("xmp:Rating=\"5\""));
    assert!(content.contains("xmp:Label=\"Green\""));
    assert!(content.contains("Dawn &amp; &lt;Ridge&gt;"));
    assert!(content.contains("Line &quot;1&quot; &apos;2&apos;"));
    assert!(content.contains("<rdf:li>raw&lt;test&gt;</rdf:li>"));

    let _ = fs::remove_dir_all(temp_dir);
}
