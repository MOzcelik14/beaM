use super::*;

fn create_test_app() -> BeamApp {
    let (tx, rx) = std::sync::mpsc::channel();
    let library_db = LibraryDb::open_in_memory().expect("in-memory db");
    let ctx = egui::Context::default();
    let logo_texture = ctx.load_texture(
        "test_logo",
        egui::ColorImage::example(),
        egui::TextureOptions::default(),
    );
    let library_state = LibraryViewState {
        logo_texture: Some(logo_texture.clone()),
        ..Default::default()
    };
    let thumb_queue = Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));

    BeamApp {
        current_view: AppView::Library,
        library_db,
        library_state,
        thumbnail_cache_dir: std::env::temp_dir(),
        thumb_queue,
        current_image: None,
        current_path: None,
        current_metadata: None,
        edit_stack: EditStack::default(),
        sidecar_path: None,
        sidecar_status: "No image".to_string(),
        edit_pipeline: None,
        gpu_proxy_image: None,
        gpu_display_texture: None,
        egui_texture_id: None,
        gpu_before_texture: None,
        egui_before_texture_id: None,
        texture_aspect_ratio: 1.0,
        before_after_mode: BeforeAfterMode::Normal,
        split_view_divider: 0.5,
        crop_mode_active: false,
        wb_eyedropper_active: false,
        active_curve_channel: 0,
        selected_mask_id: None,
        show_mask_overlay: true,
        next_mask_id: 1,
        tx,
        rx,
        is_loading: false,
        loading_filename: None,
        histogram_data: None,
        histogram_mode: HistogramMode::Rgb,
        hsl_selected_band: 0,
        error_message: None,
        gpu_error: Arc::new(Mutex::new(None)),
        show_about: false,
        show_settings: false,
        show_develop_panel: true,
        show_history_panel: true,
        zoom_level: 1.0,
        is_one_to_one: false,
        pan_offset: Vec2::ZERO,
        gpu_full_image: None,
        gpu_full_texture: None,
        egui_full_texture_id: None,
        show_shadow_clipping: false,
        show_highlight_clipping: false,
        soft_proof_space: None,
        show_gamut_warning: false,
        show_focus_peaking: false,
        filmstrip_visible: true,
        filmstrip_height: 110.0,
        is_fullscreen: false,
        settings_tab: SettingsTab::General,
        plugins: Vec::new(),
        plugin_status: None,
        pending_plugin_approval: None,
        left_panel_tab: LeftPanelTab::Presets,
        user_presets: Vec::new(),
        preset_dir: std::env::temp_dir(),
        hovered_preset: None,
        stack_before_hover: None,
        show_new_preset_dialog: false,
        new_preset_name: String::new(),
        new_preset_mask: PresetGroupMask::default(),
        copied_settings: None,
        show_copy_dialog: false,
        copy_dialog_mask: PresetGroupMask::default(),
        show_sync_dialog: false,
        sync_dialog_mask: PresetGroupMask::default(),
        sync_dialog_source_photo: None,
        history: vec![HistoryStep::new("Initial", EditStack::default())],
        history_index: 0,
        slider_drag_start_stack: None,
        slider_drag_action_name: None,
        app_config: AppConfig::default(),
        show_export_dialog: false,
        show_export_progress: false,
        export_output_dir: std::env::temp_dir(),
        export_format_index: 0,
        export_jpeg_quality: 90,
        export_color_space: beam_core::ExportColorSpace::Srgb,
        export_resize_mode_index: 0,
        export_resize_long_edge: 2048,
        export_resize_percentage: 100.0,
        export_resize_width: 1920,
        export_resize_height: 1080,
        export_resize_lock_aspect: true,
        export_sharpening_index: 0,
        export_include_exif: true,
        export_template: "{name}".to_string(),
        export_suffix: "_beam".to_string(),
        export_watermark_enabled: false,
        export_watermark_text: String::new(),
        export_watermark_logo_path: None,
        export_watermark_anchor: beam_media::WatermarkAnchor::BottomRight,
        export_watermark_opacity: 0.65,
        export_watermark_scale: 0.12,
        export_targets: Vec::new(),
        export_cancel_flag: None,
        export_progress_data: None,
        export_is_running: false,
        export_status_message: None,
        logo_texture,
        pending_project_sidecar: None,
        last_saved_stack: None,
        pending_action: None,
        show_unsaved_changes_dialog: false,
        allow_close: false,
        decode_generation: 0,
        active_imports: 0,
        snapshots: Vec::new(),
        snapshot_name_input: String::new(),
    }
}

#[test]
fn test_unsaved_changes_detection() {
    let mut app = create_test_app();

    // 1. Initially no photo is open, so no unsaved changes even if default stack
    assert!(!app.has_unsaved_changes());

    // 2. Open a photo path with clean default stack
    app.current_path = Some(PathBuf::from("/tmp/test_photo.jpg"));
    assert!(!app.has_unsaved_changes());

    // 3. Mutate an edit parameter
    app.edit_stack.exposure.ev = 1.25;
    assert!(app.has_unsaved_changes());

    // 4. Mark as saved (last_saved_stack matches edit_stack)
    app.last_saved_stack = Some(app.edit_stack.clone());
    assert!(!app.has_unsaved_changes());

    // 5. Change another parameter
    app.edit_stack.contrast.contrast = -0.5;
    assert!(app.has_unsaved_changes());

    // 6. Reset to saved state
    app.edit_stack = app.last_saved_stack.clone().unwrap();
    assert!(!app.has_unsaved_changes());
}

#[test]
fn test_close_project_cleans_state() {
    let mut app = create_test_app();

    app.current_path = Some(PathBuf::from("/tmp/sample.raw"));
    app.sidecar_path = Some(PathBuf::from("/tmp/sample.raw.beam.ron"));
    app.edit_stack.exposure.ev = 2.0;
    app.last_saved_stack = Some(app.edit_stack.clone());
    app.crop_mode_active = true;
    app.current_view = AppView::Develop;

    app.close_project();

    assert!(app.current_path.is_none());
    assert!(app.current_image.is_none());
    assert!(app.sidecar_path.is_none());
    assert!(app.last_saved_stack.is_none());
    assert_eq!(app.edit_stack, EditStack::default());
    assert_eq!(app.current_view, AppView::Library);
    assert!(!app.crop_mode_active);
    assert!(!app.has_unsaved_changes());
}

#[test]
fn test_pending_actions_lifecycle() {
    let mut app = create_test_app();

    // With unsaved changes:
    app.current_path = Some(PathBuf::from("/tmp/photo.jpg"));
    app.edit_stack.exposure.ev = 0.75;
    assert!(app.has_unsaved_changes());

    // Request close project should trigger dialog
    app.request_close_project();
    assert!(app.show_unsaved_changes_dialog);
    assert_eq!(app.pending_action, Some(PendingAction::CloseProject));

    // User cancels
    app.show_unsaved_changes_dialog = false;
    app.pending_action = None;
    assert!(!app.show_unsaved_changes_dialog);
    assert_eq!(app.pending_action, None);

    // Request open new file with unsaved edits should trigger dialog
    let new_path = PathBuf::from("/tmp/another.jpg");
    app.request_load_file_path(new_path.clone());
    assert!(app.show_unsaved_changes_dialog);
    assert_eq!(app.pending_action, Some(PendingAction::OpenFile(new_path)));

    // Once changes are cleared or saved:
    app.show_unsaved_changes_dialog = false;
    app.pending_action = None;
    app.last_saved_stack = Some(app.edit_stack.clone());
    assert!(!app.has_unsaved_changes());

    // Now request_close_project should execute immediately without modal
    app.request_close_project();
    assert!(!app.show_unsaved_changes_dialog);
    assert_eq!(app.pending_action, None);
    assert!(app.current_path.is_none());
}

#[test]
fn test_localization_unsaved_strings() {
    // Turkish
    beam_core::set_language(beam_core::Language::Tr);
    assert_eq!(t!("unsaved_dialog_title"), "Kaydedilmemiş Değişiklikler");
    assert_eq!(t!("unsaved_save"), "Kaydet");
    assert_eq!(t!("unsaved_discard_and_exit"), "Kaydetmeden Çık");
    assert_eq!(t!("unsaved_cancel"), "İptal");
    assert!(t!("unsaved_dialog_message").contains("kaydedilmemiş"));

    // English
    beam_core::set_language(beam_core::Language::En);
    assert_eq!(t!("unsaved_dialog_title"), "Unsaved Changes");
    assert_eq!(t!("unsaved_save"), "Save");
    assert_eq!(t!("unsaved_discard_and_exit"), "Exit Without Saving");
    assert_eq!(t!("unsaved_cancel"), "Cancel");
    assert!(t!("unsaved_dialog_message").contains("unsaved changes"));
}
