#[path = "dialogs.rs"]
mod dialogs;

use crate::views::library::{
    show_library_view, LibraryAction, LibraryViewState, ThumbnailRequest, ThumbnailStatus,
};
use crate::widgets::color_wheel::show_color_wheel;
use crate::widgets::crop::show_crop_overlay;
use crate::widgets::curve::show_tone_curve_colored;
use crate::widgets::histogram::{show_histogram_with_clipping, HistogramData, HistogramMode};
use crate::widgets::mask_overlay::show_mask_overlay;
use beam_core::geometry::{calculate_effective_aspect_ratio, create_centered_crop_rect};
use beam_core::sidecar::{
    ColorLabel, CropAspectPreset, EditStack, HistoryStep, Sidecar, MAX_HISTORY_STEPS,
};
use beam_core::{
    built_in_presets, default_presets_dir, delete_user_preset, load_user_presets, save_user_preset,
    set_language, t, AppConfig, Language, Preset, PresetGroupMask, ShortcutProfile, APP_NAME,
};
use beam_gpu::{EditPipeline, GpuImage};
use beam_library::{default_catalog_path, default_thumbnail_cache_dir, LibraryDb};
use beam_media::{decode_image, DecodedImage, ImageMetadata};
use eframe::egui::{
    self, Align2, Color32, FontId, MenuBar, Panel, Pos2, Rect, RichText, Slider, Stroke, Vec2,
};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppView {
    Library,
    Develop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LeftPanelTab {
    #[default]
    Presets,
    History,
    Snapshots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BeforeAfterMode {
    #[default]
    Normal,
    Toggle,
    SplitView,
    SideBySide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    Shortcuts,
    Plugins,
}

enum BackgroundMsg {
    Started(u64, PathBuf),
    Success(u64, Box<DecodedImage>),
    DecodeError {
        gen_id: u64,
        msg: String,
    },
    Error(String),
    ImportComplete {
        count: usize,
        folder: Option<PathBuf>,
    },
    ExportProgress(beam_media::BatchProgress),
    ExportFinished {
        success_count: usize,
        total: usize,
        cancelled: bool,
    },
    ThumbnailReady {
        photo_id: i64,
        thumb_path: String,
        image: egui::ColorImage,
    },
    ThumbnailFailed {
        photo_id: i64,
        error: String,
    },
    PluginFinished(String),
}

pub struct BeamApp {
    // Current application view mode
    pub current_view: AppView,

    // Library catalog database and view state
    pub library_db: LibraryDb,
    pub library_state: LibraryViewState,
    pub thumbnail_cache_dir: PathBuf,
    thumb_queue: Arc<(Mutex<VecDeque<ThumbnailRequest>>, Condvar)>,

    // Current decoded image and metadata
    current_image: Option<Box<DecodedImage>>,
    current_path: Option<PathBuf>,
    current_metadata: Option<ImageMetadata>,

    // Non-destructive edit stack & sidecar
    edit_stack: EditStack,
    sidecar_path: Option<PathBuf>,
    sidecar_status: String,

    // GPU rendering state
    edit_pipeline: Option<EditPipeline>,
    gpu_proxy_image: Option<GpuImage>,
    gpu_display_texture: Option<wgpu::Texture>,
    egui_texture_id: Option<egui::TextureId>,
    gpu_before_texture: Option<wgpu::Texture>,
    egui_before_texture_id: Option<egui::TextureId>,
    texture_aspect_ratio: f32,
    before_after_mode: BeforeAfterMode,
    split_view_divider: f32,
    pub crop_mode_active: bool,
    pub wb_eyedropper_active: bool,
    pub active_curve_channel: usize,
    pub selected_mask_id: Option<u64>,
    pub show_mask_overlay: bool,
    pub next_mask_id: u64,

    // Background decoding
    tx: Sender<BackgroundMsg>,
    rx: Receiver<BackgroundMsg>,
    is_loading: bool,
    loading_filename: Option<String>,

    // UI & Widget state
    histogram_data: Option<HistogramData>,
    histogram_mode: HistogramMode,
    hsl_selected_band: usize,
    error_message: Option<String>,
    show_about: bool,
    show_settings: bool,
    show_develop_panel: bool,
    show_history_panel: bool,
    history: Vec<HistoryStep>,
    history_index: usize,
    slider_drag_start_stack: Option<EditStack>,
    slider_drag_action_name: Option<String>,
    app_config: AppConfig,

    // Zoom, Pan & Full-Resolution Inspection
    zoom_level: f32,
    is_one_to_one: bool,
    pan_offset: Vec2,
    gpu_full_image: Option<GpuImage>,
    gpu_full_texture: Option<wgpu::Texture>,
    egui_full_texture_id: Option<egui::TextureId>,

    // Clipping & Soft Proofing overlays
    show_shadow_clipping: bool,
    show_highlight_clipping: bool,
    soft_proof_space: Option<beam_core::ExportColorSpace>,
    show_gamut_warning: bool,
    show_focus_peaking: bool,

    // Filmstrip & Fullscreen
    filmstrip_visible: bool,
    filmstrip_height: f32,
    is_fullscreen: bool,

    // Settings modal tab & plugins
    settings_tab: SettingsTab,
    plugins: Vec<beam_core::PluginManifest>,
    plugin_status: Option<String>,

    // Presets & Settings Sync
    left_panel_tab: LeftPanelTab,
    user_presets: Vec<Preset>,
    preset_dir: PathBuf,
    hovered_preset: Option<Preset>,
    stack_before_hover: Option<EditStack>,
    show_new_preset_dialog: bool,
    new_preset_name: String,
    new_preset_mask: PresetGroupMask,
    copied_settings: Option<(EditStack, PresetGroupMask)>,
    show_copy_dialog: bool,
    copy_dialog_mask: PresetGroupMask,
    show_sync_dialog: bool,
    sync_dialog_mask: PresetGroupMask,
    sync_dialog_source_photo: Option<(i64, String, EditStack)>,

    // Export dialog & process state
    show_export_dialog: bool,
    show_export_progress: bool,
    export_output_dir: PathBuf,
    export_format_index: usize,
    export_jpeg_quality: u8,
    export_color_space: beam_core::ExportColorSpace,
    export_resize_mode_index: usize,
    export_resize_long_edge: u32,
    export_resize_percentage: f32,
    export_resize_width: u32,
    export_resize_height: u32,
    export_resize_lock_aspect: bool,
    export_sharpening_index: usize,
    export_include_exif: bool,
    export_template: String,
    export_suffix: String,
    export_watermark_enabled: bool,
    export_watermark_text: String,
    export_watermark_logo_path: Option<PathBuf>,
    export_watermark_anchor: beam_media::WatermarkAnchor,
    export_watermark_opacity: f32,
    export_watermark_scale: f32,
    export_targets: Vec<(PathBuf, EditStack)>,
    export_cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    export_progress_data: Option<beam_media::BatchProgress>,
    export_is_running: bool,
    export_status_message: Option<String>,
    logo_texture: egui::TextureHandle,
    pending_project_sidecar: Option<(PathBuf, Sidecar)>,
    last_saved_stack: Option<EditStack>,
    pending_action: Option<PendingAction>,
    show_unsaved_changes_dialog: bool,
    allow_close: bool,
    decode_generation: u64,
    active_imports: usize,
    snapshots: Vec<HistoryStep>,
    snapshot_name_input: String,
}

/// Action to be executed after unsaved changes confirmation
#[derive(Clone, Debug, PartialEq)]
pub enum PendingAction {
    ExitApp,
    CloseProject,
    OpenFile(PathBuf),
    OpenProject(PathBuf),
    OpenFileDialog,
    OpenProjectDialog,
    ImportFolderDialog,
}

fn default_export_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        let pic = PathBuf::from(home).join("Pictures");
        if pic.exists() {
            return pic;
        }
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        let pic = PathBuf::from(userprofile).join("Pictures");
        if pic.exists() {
            return pic;
        }
    }
    std::env::temp_dir()
}

fn generate_and_save_thumb(
    req: &ThumbnailRequest,
    thumb_path: &Path,
) -> std::result::Result<(PathBuf, egui::ColorImage), String> {
    let edit_stack = req
        .sidecar_path
        .as_ref()
        .and_then(|sp| Sidecar::load_from_file(sp).map(|sc| sc.edit_stack).ok());

    let rgba_img =
        beam_media::generate_thumbnail_with_edits(&req.source_path, edit_stack.as_ref(), 256)
            .map_err(|e| e.to_string())?;

    let size = [rgba_img.width() as usize, rgba_img.height() as usize];
    let pixels = rgba_img.as_raw().clone();

    if let Err(e) = beam_media::save_thumbnail(&rgba_img, thumb_path) {
        log::warn!("Failed to save thumbnail to {}: {e}", thumb_path.display());
    }

    Ok((
        thumb_path.to_path_buf(),
        egui::ColorImage::from_rgba_unmultiplied(size, &pixels),
    ))
}

impl BeamApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel();

        let edit_pipeline = cc
            .wgpu_render_state
            .as_ref()
            .map(|rs| EditPipeline::new(&rs.device, wgpu::TextureFormat::Rgba8Unorm));

        let catalog_path = default_catalog_path();
        let library_db = LibraryDb::open(&catalog_path)
            .or_else(|e| {
                log::warn!(
                    "Could not open persistent catalog at {}: {e}. Using in-memory database.",
                    catalog_path.display()
                );
                LibraryDb::open_in_memory()
            })
            .expect("Failed to initialize catalog database");

        let thumbnail_cache_dir = default_thumbnail_cache_dir();
        let _ = std::fs::create_dir_all(&thumbnail_cache_dir);

        let app_config = AppConfig::load();
        set_language(app_config.language);

        let export_output_dir = default_export_dir();

        let thumb_queue: Arc<(Mutex<VecDeque<ThumbnailRequest>>, Condvar)> =
            Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
        let worker_queue = thumb_queue.clone();
        let worker_tx = tx.clone();
        let worker_thumb_dir = thumbnail_cache_dir.clone();

        thread::spawn(move || {
            let (lock, cvar) = &*worker_queue;
            loop {
                let req = {
                    let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
                    while q.is_empty() {
                        q = cvar.wait(q).unwrap_or_else(|e| e.into_inner());
                    }
                    q.pop_front()
                };

                let Some(req) = req else { continue };
                let req_id = req.photo_id;

                let thumb_path =
                    beam_media::thumbnail_cache_path(&req.source_path, &worker_thumb_dir);

                let unwind_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if thumb_path.exists() {
                        match image::open(&thumb_path) {
                            Ok(img) => {
                                let rgba = img.to_rgba8();
                                let size = [rgba.width() as usize, rgba.height() as usize];
                                Ok((
                                    thumb_path.clone(),
                                    egui::ColorImage::from_rgba_unmultiplied(
                                        size,
                                        &rgba.into_raw(),
                                    ),
                                ))
                            }
                            Err(_) => generate_and_save_thumb(&req, &thumb_path),
                        }
                    } else {
                        generate_and_save_thumb(&req, &thumb_path)
                    }
                }));

                match unwind_res {
                    Ok(Ok((path, color_image))) => {
                        let _ = worker_tx.send(BackgroundMsg::ThumbnailReady {
                            photo_id: req_id,
                            thumb_path: path.to_string_lossy().to_string(),
                            image: color_image,
                        });
                    }
                    Ok(Err(e)) => {
                        let _ = worker_tx.send(BackgroundMsg::ThumbnailFailed {
                            photo_id: req_id,
                            error: e,
                        });
                    }
                    Err(panic_err) => {
                        log::error!(
                            "Thumbnail worker caught panic for photo {req_id}: {panic_err:?}"
                        );
                        let _ = worker_tx.send(BackgroundMsg::ThumbnailFailed {
                            photo_id: req_id,
                            error: "Thumbnail generation panicked".to_string(),
                        });
                    }
                }
            }
        });

        let preset_dir = default_presets_dir();
        let user_presets = load_user_presets(&preset_dir);

        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            cc.egui_ctx.style_mut_of(theme, |s| {
                s.spacing.item_spacing = egui::vec2(8.0, 8.0);
                s.spacing.button_padding = egui::vec2(10.0, 6.0);
                s.spacing.indent = 16.0;
                s.spacing.slider_width = 140.0;

                let v = &mut s.visuals;
                v.panel_fill = Color32::from_rgb(22, 23, 28);
                v.window_fill = Color32::from_rgb(26, 28, 34);
                v.faint_bg_color = Color32::from_rgb(18, 19, 23);
                v.extreme_bg_color = Color32::from_rgb(14, 15, 18);
                v.code_bg_color = Color32::from_rgb(18, 19, 23);

                v.window_corner_radius = 8.0.into();
                v.menu_corner_radius = 7.0.into();

                v.widgets.noninteractive.corner_radius = 7.0.into();
                v.widgets.noninteractive.bg_fill = Color32::from_rgb(26, 28, 34);
                v.widgets.noninteractive.bg_stroke =
                    Stroke::new(1.0, Color32::from_rgb(38, 41, 50));
                v.widgets.noninteractive.fg_stroke =
                    Stroke::new(1.0, Color32::from_rgb(185, 190, 202));

                v.widgets.inactive.corner_radius = 7.0.into();
                v.widgets.inactive.bg_fill = Color32::from_rgb(32, 35, 43);
                v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(46, 50, 62));
                v.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(215, 220, 230));

                v.widgets.hovered.corner_radius = 7.0.into();
                v.widgets.hovered.bg_fill = Color32::from_rgb(44, 49, 60);
                v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(70, 78, 96));
                v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::from_rgb(250, 252, 255));

                v.widgets.active.corner_radius = 7.0.into();
                v.widgets.active.bg_fill = Color32::from_rgb(52, 60, 78);
                v.widgets.active.bg_stroke = Stroke::new(1.0, Color32::from_rgb(90, 110, 150));
                v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::from_rgb(255, 255, 255));

                v.widgets.open.corner_radius = 7.0.into();
                v.widgets.open.bg_fill = Color32::from_rgb(36, 40, 50);
                v.widgets.open.bg_stroke = Stroke::new(1.0, Color32::from_rgb(60, 68, 84));

                v.selection.bg_fill = Color32::from_rgba_unmultiplied(59, 130, 246, 180);
                v.selection.stroke = Stroke::new(1.5, Color32::from_rgb(96, 165, 250));
                v.slider_trailing_fill = true;
            });
        }

        let logo_texture = {
            let img = image::load_from_memory(include_bytes!("../../../assets/logo.png"))
                .expect("Valid embedded logo asset")
                .to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            let color_image = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            cc.egui_ctx
                .load_texture("beam_app_logo", color_image, egui::TextureOptions::LINEAR)
        };

        let active_folder = app_config.last_opened_folder.clone().or_else(|| {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .ok()
                .map(PathBuf::from)?;
            let pictures = home.join("Pictures");
            if pictures.exists() {
                Some(pictures)
            } else {
                Some(home)
            }
        });
        let subdirectories = active_folder
            .as_ref()
            .map(|f| beam_library::list_subdirectories(f))
            .unwrap_or_default();

        let mut library_state = LibraryViewState {
            logo_texture: Some(logo_texture.clone()),
            active_folder: active_folder.clone(),
            bookmarked_folders: app_config.bookmarked_folders.clone(),
            subdirectories,
            ..Default::default()
        };
        if let Some(ref folder) = active_folder {
            library_state.filter.folder_path = Some(folder.clone());
            library_state.filter.recursive = app_config.recursive_browse;
        }

        let mut app = Self {
            current_view: AppView::Library,
            library_db,
            library_state,
            thumbnail_cache_dir,
            thumb_queue,
            current_image: None,
            current_path: None,
            current_metadata: None,
            edit_stack: EditStack::default(),
            sidecar_path: None,
            sidecar_status: "No image".to_string(),
            edit_pipeline,
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
            show_about: false,
            show_settings: false,
            show_develop_panel: app_config.show_develop_panel,
            show_history_panel: app_config.show_history_panel,
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
            filmstrip_visible: app_config.filmstrip_visible,
            filmstrip_height: app_config.filmstrip_height.unwrap_or(110.0),
            is_fullscreen: false,
            settings_tab: SettingsTab::General,
            plugins: beam_core::discover_plugins(&beam_core::default_plugins_dir()),
            plugin_status: None,
            left_panel_tab: LeftPanelTab::Presets,
            user_presets,
            preset_dir,
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
            app_config,
            show_export_dialog: false,
            show_export_progress: false,
            export_output_dir,
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
        };

        if let Some(ref folder) = app.library_state.active_folder.clone() {
            if folder.exists() {
                app.import_folder_path(folder.clone());
            }
        }

        app
    }

    /// Navigate to a folder in the file browser and scan its photos
    pub fn browse_folder(&mut self, folder: PathBuf) {
        self.library_state.active_folder = Some(folder.clone());
        self.library_state.filter.folder_path = Some(folder.clone());
        self.library_state.subdirectories = beam_library::list_subdirectories(&folder);
        self.app_config.last_opened_folder = Some(folder.clone());
        let _ = self.app_config.save();
        self.import_folder_path(folder);
        self.library_state.needs_refresh = true;
    }

    /// Open file dialog and spawn background thread to decode the image
    pub fn open_file_dialog(&mut self) {
        self.decode_generation = self.decode_generation.wrapping_add(1);
        let gen_id = self.decode_generation;
        let tx = self.tx.clone();
        let dialog_title = t!("menu_file_open");
        thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .set_title(dialog_title)
                .add_filter(
                    "All Supported Photos",
                    &[
                        // RAW formats (lowercase & camera uppercase)
                        "arw", "cr2", "cr3", "nef", "dng", "raf", "rw2", "orf", "pef", "ARW", "CR2",
                        "CR3", "NEF", "DNG", "RAF", "RW2", "ORF", "PEF",
                        // JPEG formats
                        "jpg", "jpeg", "JPG", "JPEG", // Raster formats
                        "png", "tif", "tiff", "webp", "PNG", "TIF", "TIFF", "WEBP",
                    ],
                )
                .add_filter(
                    "JPEG Photos (*.jpg, *.jpeg, *.JPG, *.JPEG)",
                    &["jpg", "jpeg", "JPG", "JPEG"],
                )
                .add_filter(
                    "RAW Photos (*.arw, *.cr2, *.nef, *.dng...)",
                    &[
                        "arw", "cr2", "cr3", "nef", "dng", "raf", "rw2", "orf", "pef", "ARW",
                        "CR2", "CR3", "NEF", "DNG", "RAF", "RW2", "ORF", "PEF",
                    ],
                )
                .add_filter("All Files", &["*"])
                .pick_file();

            if let Some(path) = file {
                let _ = tx.send(BackgroundMsg::Started(gen_id, path.clone()));
                let res =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode_image(&path)));
                match res {
                    Ok(Ok(decoded)) => {
                        let _ = tx.send(BackgroundMsg::Success(gen_id, Box::new(decoded)));
                    }
                    Ok(Err(err)) => {
                        let _ = tx.send(BackgroundMsg::DecodeError {
                            gen_id,
                            msg: err.to_string(),
                        });
                    }
                    Err(panic_err) => {
                        log::error!("Image decoding worker panicked: {panic_err:?}");
                        let _ = tx.send(BackgroundMsg::DecodeError {
                            gen_id,
                            msg: "Image decoding panicked".to_string(),
                        });
                    }
                }
            }
        });
    }

    /// Import a folder path asynchronously into the library catalog
    pub fn import_folder_path(&mut self, dir: PathBuf) {
        let tx = self.tx.clone();
        let thumb_dir = self.thumbnail_cache_dir.clone();
        let catalog_path = default_catalog_path();
        let recursive = self.app_config.recursive_browse;
        self.active_imports += 1;
        self.library_state.is_importing = true;
        self.library_state.import_status = Some(t!("library_importing"));

        let scan_dir = dir.clone();
        thread::spawn(move || {
            let db = match LibraryDb::open(&catalog_path) {
                Ok(d) => d,
                Err(e) => {
                    let _ = tx.send(BackgroundMsg::Error(format!(
                        "Failed to open database for import: {e}"
                    )));
                    return;
                }
            };

            let scan_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                db.scan_and_import_folder(&scan_dir, &thumb_dir, recursive)
            }));

            match scan_res {
                Ok(Ok(count)) => {
                    let _ = tx.send(BackgroundMsg::ImportComplete {
                        count,
                        folder: Some(scan_dir),
                    });
                }
                Ok(Err(e)) => {
                    let _ = tx.send(BackgroundMsg::Error(format!("Folder import failed: {e}")));
                }
                Err(panic_err) => {
                    log::error!("Folder import panicked: {panic_err:?}");
                    let _ = tx.send(BackgroundMsg::Error("Folder import panicked".to_string()));
                }
            }
        });
    }

    /// Open folder dialog and recursively import all photos into the library catalog
    pub fn import_folder_dialog(&mut self) {
        let tx = self.tx.clone();
        let thumb_dir = self.thumbnail_cache_dir.clone();
        let catalog_path = default_catalog_path();
        let dialog_title = t!("menu_file_import");
        let recursive = self.app_config.recursive_browse;

        thread::spawn(move || {
            let folder = rfd::FileDialog::new().set_title(dialog_title).pick_folder();

            if let Some(dir) = folder {
                let db = match LibraryDb::open(&catalog_path) {
                    Ok(d) => d,
                    Err(e) => {
                        let _ = tx.send(BackgroundMsg::Error(format!(
                            "Failed to open database for import: {e}"
                        )));
                        return;
                    }
                };

                let scan_dir = dir.clone();
                let scan_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    db.scan_and_import_folder(&scan_dir, &thumb_dir, recursive)
                }));

                match scan_res {
                    Ok(Ok(count)) => {
                        let _ = tx.send(BackgroundMsg::ImportComplete {
                            count,
                            folder: Some(dir),
                        });
                    }
                    Ok(Err(e)) => {
                        let _ = tx.send(BackgroundMsg::Error(format!("Folder import failed: {e}")));
                    }
                    Err(panic_err) => {
                        log::error!("Folder import dialog panicked: {panic_err:?}");
                        let _ = tx.send(BackgroundMsg::Error("Folder import panicked".to_string()));
                    }
                }
            }
        });
    }

    /// Request closing the project. If there are unsaved changes, prompt the user.
    pub fn request_close_project(&mut self) {
        if self.has_unsaved_changes() {
            self.pending_action = Some(PendingAction::CloseProject);
            self.show_unsaved_changes_dialog = true;
        } else {
            self.close_project();
        }
    }

    /// Close current project / photo and clear the workspace
    pub fn close_project(&mut self) {
        self.current_image = None;
        self.current_path = None;
        self.sidecar_path = None;
        self.last_saved_stack = None;
        self.edit_stack = EditStack::default();
        self.history = vec![HistoryStep::new(
            t!("history_initial"),
            EditStack::default(),
        )];
        self.history_index = 0;
        self.current_metadata = None;
        self.sidecar_status = String::new();
        self.gpu_proxy_image = None;
        self.gpu_display_texture = None;
        self.egui_texture_id = None;
        self.gpu_before_texture = None;
        self.egui_before_texture_id = None;
        self.gpu_full_image = None;
        self.gpu_full_texture = None;
        self.egui_full_texture_id = None;
        self.histogram_data = None;
        self.snapshots.clear();
        self.selected_mask_id = None;
        self.wb_eyedropper_active = false;
        self.crop_mode_active = false;
        self.slider_drag_start_stack = None;
        self.slider_drag_action_name = None;
        self.current_view = AppView::Library;
    }

    /// Request opening a file dialog. If there are unsaved changes, prompt first.
    pub fn request_open_file_dialog(&mut self) {
        if self.has_unsaved_changes() {
            self.pending_action = Some(PendingAction::OpenFileDialog);
            self.show_unsaved_changes_dialog = true;
        } else {
            self.open_file_dialog();
        }
    }

    /// Request opening a project dialog. If there are unsaved changes, prompt first.
    pub fn request_open_project_dialog(&mut self) {
        if self.has_unsaved_changes() {
            self.pending_action = Some(PendingAction::OpenProjectDialog);
            self.show_unsaved_changes_dialog = true;
        } else {
            self.open_project_dialog();
        }
    }

    /// Request importing a folder dialog. If there are unsaved changes, prompt first.
    pub fn request_import_folder_dialog(&mut self) {
        if self.has_unsaved_changes() {
            self.pending_action = Some(PendingAction::ImportFolderDialog);
            self.show_unsaved_changes_dialog = true;
        } else {
            self.import_folder_dialog();
        }
    }

    /// Request loading a file path. If there are unsaved changes, prompt first.
    pub fn request_load_file_path(&mut self, path: PathBuf) {
        if self.has_unsaved_changes() {
            self.pending_action = Some(PendingAction::OpenFile(path));
            self.show_unsaved_changes_dialog = true;
        } else {
            self.load_file_path(path);
        }
    }

    /// Request loading a project file. If there are unsaved changes, prompt first.
    pub fn request_load_project_file(&mut self, path: PathBuf) {
        if self.has_unsaved_changes() {
            self.pending_action = Some(PendingAction::OpenProject(path));
            self.show_unsaved_changes_dialog = true;
        } else {
            self.load_project_file(path);
        }
    }

    /// Request application exit. If there are unsaved changes, prompt first.
    pub fn request_exit(&mut self, ctx: &egui::Context) {
        if self.has_unsaved_changes() {
            self.pending_action = Some(PendingAction::ExitApp);
            self.show_unsaved_changes_dialog = true;
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Execute a deferred pending action after unsaved changes confirmation
    pub fn execute_pending_action(&mut self, action: PendingAction, ctx: &egui::Context) {
        match action {
            PendingAction::ExitApp => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            PendingAction::CloseProject => {
                self.close_project();
                ctx.request_repaint();
            }
            PendingAction::OpenFile(path) => {
                self.load_file_path(path);
            }
            PendingAction::OpenProject(path) => {
                self.load_project_file(path);
            }
            PendingAction::OpenFileDialog => {
                self.open_file_dialog();
            }
            PendingAction::OpenProjectDialog => {
                self.open_project_dialog();
            }
            PendingAction::ImportFolderDialog => {
                self.import_folder_dialog();
            }
        }
    }

    /// Load a specific file path asynchronously
    pub fn load_file_path(&mut self, path: PathBuf) {
        self.load_file_path_with_sidecar(path, None);
    }

    /// Load a specific file path asynchronously with optional preloaded project/sidecar
    pub fn load_file_path_with_sidecar(
        &mut self,
        path: PathBuf,
        project_sidecar: Option<(PathBuf, Sidecar)>,
    ) {
        self.decode_generation = self.decode_generation.wrapping_add(1);
        let gen_id = self.decode_generation;
        self.pending_project_sidecar = project_sidecar;
        self.current_view = AppView::Develop;
        let tx = self.tx.clone();
        let _ = tx.send(BackgroundMsg::Started(gen_id, path.clone()));
        thread::spawn(move || {
            let res =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode_image(&path)));
            match res {
                Ok(Ok(decoded)) => {
                    let _ = tx.send(BackgroundMsg::Success(gen_id, Box::new(decoded)));
                }
                Ok(Err(err)) => {
                    let _ = tx.send(BackgroundMsg::DecodeError {
                        gen_id,
                        msg: err.to_string(),
                    });
                }
                Err(panic_err) => {
                    log::error!("Image decoding worker panicked: {panic_err:?}");
                    let _ = tx.send(BackgroundMsg::DecodeError {
                        gen_id,
                        msg: "Image decoding panicked".to_string(),
                    });
                }
            }
        });
    }

    /// Load a .beam project file
    pub fn load_project_file(&mut self, project_path: PathBuf) {
        let sidecar = match Sidecar::load_from_file(&project_path) {
            Ok(sc) => sc,
            Err(e) => {
                self.error_message = Some(format!("{}: {e}", t!("project_load_error")));
                return;
            }
        };

        // Resolve original photo path
        let mut resolved_image_path = None;
        let direct = PathBuf::from(&sidecar.source_filename);
        if direct.is_file() {
            resolved_image_path = Some(direct);
        } else if let Some(parent) = project_path.parent() {
            let candidate = parent.join(&sidecar.source_filename);
            if candidate.is_file() {
                resolved_image_path = Some(candidate);
            } else if let Ok(entries) = std::fs::read_dir(parent) {
                let target_lower = sidecar.source_filename.to_ascii_lowercase();
                let target_stem = Path::new(&sidecar.source_filename)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase());

                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() {
                        let fname = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                        if fname.to_ascii_lowercase() == target_lower {
                            resolved_image_path = Some(p);
                            break;
                        }
                        if let Some(ref stem) = target_stem {
                            if let Some(p_stem) = p.file_stem().and_then(|s| s.to_str()) {
                                if p_stem.to_ascii_lowercase() == *stem
                                    && beam_library::is_supported_image(&p)
                                {
                                    resolved_image_path = Some(p);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Some(img_path) = resolved_image_path {
            self.load_file_path_with_sidecar(img_path, Some((project_path, sidecar)));
        } else {
            self.error_message = Some(t!(
                "project_image_not_found",
                "path" => sidecar.source_filename
            ));
        }
    }

    /// Save current project / sidecar
    pub fn save_project(&mut self) {
        if self.current_image.is_some() {
            self.save_sidecar();
        }
    }

    /// Save current project as a standalone .beam project file
    pub fn save_project_as_dialog(&mut self) {
        if let Some(path) = &self.current_path {
            let suggested_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("project");
            let default_name = format!("{suggested_name}.beam");
            let dialog = rfd::FileDialog::new()
                .set_title(t!("menu_save_project_as"))
                .set_file_name(&default_name)
                .add_filter("beaM Project (*.beam)", &["beam", "BEAM"]);

            let target = if let Some(parent) = path.parent() {
                dialog.set_directory(parent).save_file()
            } else {
                dialog.save_file()
            };

            if let Some(save_path) = target {
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
                let mut sidecar = Sidecar::new(filename);
                sidecar.edit_stack = self.edit_stack.clone();
                sidecar.history = self.history.clone();
                sidecar.history_index = self.history_index;
                if let Some(photo_id) = self.library_state.selected_photo_id {
                    if let Ok(Some(photo)) = self.library_db.get_photo(photo_id) {
                        sidecar.set_rating(photo.rating);
                        sidecar.set_color_label(photo.color_label);
                    }
                }
                if let Err(e) = sidecar.save_to_file(&save_path) {
                    log::error!("Failed to save project to {}: {e}", save_path.display());
                    self.error_message = Some(format!("Failed to save project: {e}"));
                } else {
                    self.last_saved_stack = Some(self.edit_stack.clone());
                    self.sidecar_path = Some(save_path.clone());
                    self.sidecar_status = t!("project_saved");
                    log::info!("Project saved to {}", save_path.display());
                }
            }
        }
    }

    /// Open project dialog to select and open a .beam project
    pub fn open_project_dialog(&mut self) {
        let dialog_title = t!("menu_open_project");
        let file = rfd::FileDialog::new()
            .set_title(dialog_title)
            .add_filter("beaM Project (*.beam)", &["beam", "BEAM", "beam.ron"])
            .add_filter("All Files", &["*"])
            .pick_file();

        if let Some(path) = file {
            self.load_project_file(path);
        }
    }

    /// Process messages from the background decoding and import threads
    fn check_background_messages(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                BackgroundMsg::Started(gen_id, path) => {
                    if gen_id != self.decode_generation {
                        continue;
                    }
                    self.is_loading = true;
                    self.loading_filename = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string());
                    self.current_path = Some(path.clone());
                    self.error_message = None;

                    // Compute sidecar path and load existing edits if present
                    let (sidecar_p, loaded_sidecar) =
                        if let Some((proj_p, sc)) = self.pending_project_sidecar.take() {
                            (proj_p, Some(sc))
                        } else if let Some(existing_p) = Sidecar::find_sidecar_for(&path) {
                            let sc = Sidecar::load_from_file(&existing_p).ok();
                            (Sidecar::path_for(&path), sc)
                        } else {
                            (Sidecar::path_for(&path), None)
                        };

                    if let Some(sidecar) = loaded_sidecar {
                        self.edit_stack = sidecar.edit_stack;
                        self.history = sidecar.history;
                        self.history_index = sidecar.history_index;
                        self.snapshots = sidecar.snapshots;
                        self.sidecar_status = t!("status_ready");
                    } else {
                        self.edit_stack = EditStack::default();
                        self.history = vec![HistoryStep::new(
                            t!("history_initial"),
                            EditStack::default(),
                        )];
                        self.history_index = 0;
                        self.snapshots.clear();
                        self.sidecar_status = t!("status_ready");
                    }
                    self.slider_drag_start_stack = None;
                    self.slider_drag_action_name = None;
                    self.sidecar_path = Some(sidecar_p);
                    self.last_saved_stack = Some(self.edit_stack.clone());
                    ctx.request_repaint();
                }
                BackgroundMsg::Success(gen_id, decoded) => {
                    if gen_id != self.decode_generation {
                        continue;
                    }
                    self.is_loading = false;
                    self.loading_filename = None;
                    self.current_metadata = Some(decoded.metadata.clone());
                    self.upload_proxy_and_render(frame, *decoded);
                    ctx.request_repaint();
                }
                BackgroundMsg::DecodeError { gen_id, msg } => {
                    if gen_id != self.decode_generation {
                        continue;
                    }
                    self.is_loading = false;
                    self.loading_filename = None;
                    self.error_message = Some(msg);
                    ctx.request_repaint();
                }
                BackgroundMsg::Error(err_msg) => {
                    self.active_imports = self.active_imports.saturating_sub(1);
                    if self.active_imports == 0 {
                        self.library_state.is_importing = false;
                        self.library_state.import_status = None;
                    }
                    self.error_message = Some(err_msg);
                    ctx.request_repaint();
                }
                BackgroundMsg::ImportComplete { count, folder } => {
                    self.active_imports = self.active_imports.saturating_sub(1);
                    if self.active_imports == 0 {
                        self.library_state.is_importing = false;
                        self.library_state.import_status = None;
                    }
                    if let Some(folder) = folder {
                        if self.library_state.active_folder.as_ref() == Some(&folder) {
                            self.library_state.subdirectories =
                                beam_library::list_subdirectories(&folder);
                        }
                    }
                    self.library_state.needs_refresh = true;
                    log::info!("Imported {count} photos into catalog");
                    ctx.request_repaint();
                }
                BackgroundMsg::ExportProgress(progress) => {
                    self.export_progress_data = Some(progress);
                    ctx.request_repaint();
                }
                BackgroundMsg::ExportFinished {
                    success_count,
                    total,
                    cancelled,
                } => {
                    self.export_is_running = false;
                    self.export_cancel_flag = None;
                    self.export_status_message = if cancelled {
                        Some(t!(
                            "export_cancelled",
                            "count" => success_count,
                            "total" => total
                        ))
                    } else {
                        Some(t!(
                            "export_complete",
                            "count" => success_count,
                            "total" => total
                        ))
                    };
                    ctx.request_repaint();
                }
                BackgroundMsg::ThumbnailReady {
                    photo_id,
                    thumb_path,
                    image,
                } => {
                    let tex = ctx.load_texture(
                        format!("thumb_{photo_id}"),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.library_state
                        .insert_thumbnail(photo_id, ThumbnailStatus::Ready(tex));
                    let _ = self.library_db.update_thumbnail_path(photo_id, &thumb_path);
                    ctx.request_repaint();
                }
                BackgroundMsg::ThumbnailFailed { photo_id, error } => {
                    log::warn!("Thumbnail generation failed for photo {photo_id}: {error}");
                    self.library_state
                        .insert_thumbnail(photo_id, ThumbnailStatus::Failed(error));
                    ctx.request_repaint();
                }
                BackgroundMsg::PluginFinished(msg) => {
                    self.plugin_status = Some(msg);
                    ctx.request_repaint();
                }
            }
        }
    }

    /// Upload proxy buffer to GPU and render edit pipeline
    fn upload_proxy_and_render(&mut self, frame: &mut eframe::Frame, decoded: DecodedImage) {
        let Some(render_state) = frame.wgpu_render_state() else {
            self.error_message = Some("GPU RenderState not available".to_string());
            return;
        };

        let device = &render_state.device;
        let queue = &render_state.queue;

        if self.edit_pipeline.is_none() {
            self.edit_pipeline = Some(EditPipeline::new(device, wgpu::TextureFormat::Rgba8Unorm));
        }
        let Some(pipeline) = self.edit_pipeline.as_mut() else {
            return;
        };

        // Upload proxy image in linear Rec.2020
        let gpu_proxy = match GpuImage::from_rgba32f(
            device,
            queue,
            decoded.proxy_width,
            decoded.proxy_height,
            &decoded.proxy_data,
        ) {
            Ok(img) => img,
            Err(e) => {
                self.error_message = Some(format!("Failed to upload proxy to GPU: {e}"));
                return;
            }
        };

        // Create the display target texture (Rgba8Unorm)
        let display_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("beam_display_target_texture"),
            size: wgpu::Extent3d {
                width: decoded.proxy_width,
                height: decoded.proxy_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let display_view = display_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Run edit pipeline with active proofing/clipping/peaking options
        pipeline.render_with_proof_options(
            device,
            queue,
            &gpu_proxy,
            &display_view,
            &self.edit_stack,
            self.crop_mode_active,
            self.show_shadow_clipping,
            self.show_highlight_clipping,
            self.soft_proof_space,
            self.show_gamut_warning,
            self.show_focus_peaking,
        );

        // Create and render Before (unedited baseline) texture
        let before_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("beam_before_target_texture"),
            size: wgpu::Extent3d {
                width: decoded.proxy_width,
                height: decoded.proxy_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let before_view = before_texture.create_view(&wgpu::TextureViewDescriptor::default());
        pipeline.render(
            device,
            queue,
            &gpu_proxy,
            &before_view,
            &EditStack::default(),
        );

        // Register or update egui native textures
        let mut renderer = render_state.renderer.write();
        if let Some(old_full_id) = self.egui_full_texture_id.take() {
            renderer.free_texture(&old_full_id);
        }
        if let Some(existing_id) = self.egui_texture_id {
            renderer.update_egui_texture_from_wgpu_texture(
                device,
                &display_view,
                wgpu::FilterMode::Linear,
                existing_id,
            );
        } else {
            let id =
                renderer.register_native_texture(device, &display_view, wgpu::FilterMode::Linear);
            self.egui_texture_id = Some(id);
        }

        if let Some(existing_before_id) = self.egui_before_texture_id {
            renderer.update_egui_texture_from_wgpu_texture(
                device,
                &before_view,
                wgpu::FilterMode::Linear,
                existing_before_id,
            );
        } else {
            let id =
                renderer.register_native_texture(device, &before_view, wgpu::FilterMode::Linear);
            self.egui_before_texture_id = Some(id);
        }
        drop(renderer);

        let pw = decoded.proxy_width as usize;
        let ph = decoded.proxy_height as usize;
        self.histogram_data = if pw > 0 && ph > 0 && decoded.proxy_data.len() >= pw * ph * 4 {
            let step = (pw.max(ph) / 160).max(1);
            let sw = pw.div_ceil(step) as u32;
            let sh = ph.div_ceil(step) as u32;
            let mut sample = Vec::with_capacity((sw * sh * 4) as usize);
            for y in (0..ph).step_by(step) {
                for x in (0..pw).step_by(step) {
                    let idx = (y * pw + x) * 4;
                    sample.extend_from_slice(&decoded.proxy_data[idx..idx + 4]);
                }
            }
            let edited = beam_media::decoder::apply_cpu_edits(sw, sh, &sample, &self.edit_stack);
            Some(HistogramData::compute_from_rgba32f_dims(
                &edited,
                sw as usize,
                sh as usize,
            ))
        } else {
            None
        };
        self.texture_aspect_ratio =
            calculate_effective_aspect_ratio(decoded.width, decoded.height, &self.edit_stack.crop);
        self.gpu_display_texture = Some(display_texture);
        self.gpu_before_texture = Some(before_texture);
        self.gpu_proxy_image = Some(gpu_proxy);
        self.current_image = Some(Box::new(decoded));
        self.gpu_full_image = None;
        self.gpu_full_texture = None;
        self.zoom_level = 1.0;
        self.pan_offset = Vec2::ZERO;
        self.is_one_to_one = false;

        // Automatically register photo in library database
        if let Some(path) = &self.current_path {
            if let Ok(record) = self.library_db.import_file(path, &self.thumbnail_cache_dir) {
                self.library_state.selected_photo_id = Some(record.id);
            }
            self.library_state.needs_refresh = true;
        }
    }

    /// Ensure full resolution texture is uploaded and rendered with current edit stack
    fn ensure_full_res_rendered(&mut self, frame: &mut eframe::Frame) {
        let (Some(render_state), Some(pipeline), Some(decoded)) = (
            frame.wgpu_render_state(),
            &mut self.edit_pipeline,
            &self.current_image,
        ) else {
            return;
        };

        let device = &render_state.device;
        let queue = &render_state.queue;

        if self.gpu_full_image.is_none() {
            let gpu_full = match GpuImage::from_rgba32f(
                device,
                queue,
                decoded.width,
                decoded.height,
                &decoded.data,
            ) {
                Ok(img) => img,
                Err(e) => {
                    log::warn!("Failed to upload full resolution image to GPU: {e}");
                    return;
                }
            };
            self.gpu_full_image = Some(gpu_full);
        }

        if self.gpu_full_texture.is_none() {
            let full_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("beam_full_display_target_texture"),
                size: wgpu::Extent3d {
                    width: decoded.width,
                    height: decoded.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.gpu_full_texture = Some(full_texture);
        }

        let Some(gpu_full) = &self.gpu_full_image else {
            return;
        };
        let Some(full_texture) = &self.gpu_full_texture else {
            return;
        };
        let full_view = full_texture.create_view(&wgpu::TextureViewDescriptor::default());

        pipeline.render_with_proof_options(
            device,
            queue,
            gpu_full,
            &full_view,
            &self.edit_stack,
            self.crop_mode_active,
            self.show_shadow_clipping,
            self.show_highlight_clipping,
            self.soft_proof_space,
            self.show_gamut_warning,
            self.show_focus_peaking,
        );

        let mut renderer = render_state.renderer.write();
        if let Some(existing_id) = self.egui_full_texture_id {
            renderer.update_egui_texture_from_wgpu_texture(
                device,
                &full_view,
                wgpu::FilterMode::Linear,
                existing_id,
            );
        } else {
            let id = renderer.register_native_texture(device, &full_view, wgpu::FilterMode::Linear);
            self.egui_full_texture_id = Some(id);
        }
    }

    /// Re-render the existing proxy with updated slider values (takes <1ms on GPU)
    fn re_render_edits(&mut self, frame: &mut eframe::Frame) {
        let (Some(render_state), Some(gpu_proxy), Some(display_tex), Some(pipeline)) = (
            frame.wgpu_render_state(),
            &self.gpu_proxy_image,
            &self.gpu_display_texture,
            &mut self.edit_pipeline,
        ) else {
            return;
        };

        if let Some(img) = &self.current_image {
            if self.crop_mode_active {
                let (w, h) = if self.edit_stack.crop.rotation == 90
                    || self.edit_stack.crop.rotation == 270
                {
                    (img.height as f32, img.width as f32)
                } else {
                    (img.width as f32, img.height as f32)
                };
                self.texture_aspect_ratio = w / h;
            } else {
                self.texture_aspect_ratio =
                    calculate_effective_aspect_ratio(img.width, img.height, &self.edit_stack.crop);
            }
        }

        let device = &render_state.device;
        let queue = &render_state.queue;
        let display_view = display_tex.create_view(&wgpu::TextureViewDescriptor::default());

        pipeline.render_with_proof_options(
            device,
            queue,
            gpu_proxy,
            &display_view,
            &self.edit_stack,
            self.crop_mode_active,
            self.show_shadow_clipping,
            self.show_highlight_clipping,
            self.soft_proof_space,
            self.show_gamut_warning,
            self.show_focus_peaking,
        );

        if self.is_one_to_one {
            self.ensure_full_res_rendered(frame);
        }

        if let Some(img) = &self.current_image {
            let pw = img.proxy_width as usize;
            let ph = img.proxy_height as usize;
            if pw > 0 && ph > 0 && img.proxy_data.len() >= pw * ph * 4 {
                let step = (pw.max(ph) / 160).max(1);
                let sw = pw.div_ceil(step) as u32;
                let sh = ph.div_ceil(step) as u32;
                let mut sample = Vec::with_capacity((sw * sh * 4) as usize);
                for y in (0..ph).step_by(step) {
                    for x in (0..pw).step_by(step) {
                        let idx = (y * pw + x) * 4;
                        sample.extend_from_slice(&img.proxy_data[idx..idx + 4]);
                    }
                }
                let edited =
                    beam_media::decoder::apply_cpu_edits(sw, sh, &sample, &self.edit_stack);
                self.histogram_data = Some(HistogramData::compute_from_rgba32f_dims(
                    &edited,
                    sw as usize,
                    sh as usize,
                ));
            }
        }
    }

    /// Push a new history step onto the history stack.
    pub fn push_history_step(&mut self, name: impl Into<String>) {
        let name_str = name.into();
        if self.history.is_empty() {
            self.history.push(HistoryStep::new(
                t!("history_initial"),
                EditStack::default(),
            ));
        }

        if self.history_index + 1 < self.history.len() {
            self.history.truncate(self.history_index + 1);
        }

        self.history
            .push(HistoryStep::new(name_str, self.edit_stack.clone()));
        self.history_index = self.history.len() - 1;

        if self.history.len() > MAX_HISTORY_STEPS {
            self.history.remove(1);
            self.history_index = self.history.len() - 1;
        }
        self.save_sidecar();
    }

    /// Undo to previous history step
    pub fn undo(&mut self, frame: &mut eframe::Frame) {
        if self.history_index > 0 && !self.history.is_empty() {
            self.history_index -= 1;
            self.edit_stack = self.history[self.history_index].stack.clone();
            self.re_render_edits(frame);
            self.save_sidecar();
        }
    }

    /// Redo to next history step
    pub fn redo(&mut self, frame: &mut eframe::Frame) {
        if self.history_index + 1 < self.history.len() {
            self.history_index += 1;
            self.edit_stack = self.history[self.history_index].stack.clone();
            self.re_render_edits(frame);
            self.save_sidecar();
        }
    }

    /// Jump directly to a step by index
    pub fn jump_to_history_step(&mut self, frame: &mut eframe::Frame, index: usize) {
        if index < self.history.len() {
            self.history_index = index;
            self.edit_stack = self.history[index].stack.clone();
            self.re_render_edits(frame);
            self.save_sidecar();
        }
    }

    /// Paste settings from clipboard to current image
    pub fn paste_settings(&mut self, frame: &mut eframe::Frame) {
        if let Some((src_stack, mask)) = &self.copied_settings {
            let preset = Preset::from_stack("clipboard", "Clipboard", src_stack, *mask);
            preset.apply_to(&mut self.edit_stack);
            self.push_history_step(t!("history_paste_settings"));
            self.re_render_edits(frame);
        }
    }

    /// Import preset file via file picker
    pub fn import_preset_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("beaM Preset", &["ron"])
            .pick_file()
        {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(preset) = Preset::from_ron_str(&content) {
                    let _ = save_user_preset(&preset, &self.preset_dir);
                    self.user_presets = load_user_presets(&self.preset_dir);
                }
            }
        }
    }

    /// Toggle Before/After view
    pub fn toggle_before_after(&mut self) {
        self.before_after_mode = match self.before_after_mode {
            BeforeAfterMode::Normal => BeforeAfterMode::Toggle,
            BeforeAfterMode::Toggle => BeforeAfterMode::Normal,
            BeforeAfterMode::SplitView => BeforeAfterMode::Normal,
            BeforeAfterMode::SideBySide => BeforeAfterMode::Normal,
        };
    }

    /// Persist current edit stack and history to the sidecar file on disk
    fn save_sidecar(&mut self) {
        if let Some(path) = &self.current_path {
            let sidecar_p = self
                .sidecar_path
                .clone()
                .unwrap_or_else(|| Sidecar::path_for(path));
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
            let mut sidecar = Sidecar::new(filename);
            sidecar.edit_stack = self.edit_stack.clone();
            sidecar.history = self.history.clone();
            sidecar.history_index = self.history_index;
            sidecar.snapshots = self.snapshots.clone();

            if let Some(photo_id) = self.library_state.selected_photo_id {
                if let Ok(Some(photo)) = self.library_db.get_photo(photo_id) {
                    sidecar.set_rating(photo.rating);
                    sidecar.set_color_label(photo.color_label);
                }
            }

            if let Err(e) = sidecar.save_to_file(&sidecar_p) {
                log::error!("Failed to save sidecar to {}: {e}", sidecar_p.display());
                self.sidecar_status = t!("status_save_error", "error" => e.to_string());
            } else {
                self.sidecar_path = Some(sidecar_p);
                self.last_saved_stack = Some(self.edit_stack.clone());
                self.sidecar_status = t!("status_saved");
                if let Some(photo_id) = self.library_state.selected_photo_id {
                    self.library_state.thumbnail_status.remove(&photo_id);
                    let new_thumb_path =
                        beam_media::thumbnail_cache_path(path, &self.thumbnail_cache_dir);
                    let _ = self
                        .library_db
                        .update_thumbnail_path(photo_id, &new_thumb_path.to_string_lossy());
                }
            }
        }
    }

    /// Queue a batch of thumbnail generation requests, prioritizing visible items
    pub fn queue_thumbnail_requests(&self, requests: Vec<ThumbnailRequest>) {
        if requests.is_empty() {
            return;
        }
        let (lock, cvar) = &*self.thumb_queue;
        let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
        for req in requests {
            if !q.iter().any(|r| r.photo_id == req.photo_id) {
                if q.len() >= 500 {
                    q.pop_back();
                }
                q.push_front(req);
            }
        }
        cvar.notify_all();
    }

    /// Invalidate disk cache and rebuild all thumbnails from scratch
    pub fn rebuild_thumbnails(&mut self) {
        // 1. Clear thumbnail worker queue
        {
            let (lock, _) = &*self.thumb_queue;
            let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
            q.clear();
        }

        // 2. Clear disk cache files on a background thread so UI stays responsive
        let cache_dir = self.thumbnail_cache_dir.clone();
        std::thread::spawn(move || {
            if let Ok(entries) = std::fs::read_dir(&cache_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() {
                        let _ = std::fs::remove_file(p);
                    }
                }
            }
        });

        // 3. Clear SQLite thumbnail paths
        let _ = self.library_db.clear_all_thumbnails();

        // 4. Reset in-memory textures and status
        self.library_state.thumbnail_status.clear();
        self.library_state.thumbnail_order.clear();
        self.library_state.needs_refresh = true;
    }
}

impl eframe::App for BeamApp {
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.save_session_state();
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Intercept window close request if there are unsaved changes
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self.has_unsaved_changes()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending_action = Some(PendingAction::ExitApp);
            self.show_unsaved_changes_dialog = true;
        }

        self.check_background_messages(&ctx, frame);

        // Handle drag-and-drop of photos or folders onto the window
        let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped_files.is_empty() {
            for dropped in dropped_files {
                let path = dropped.path();
                if !path.as_os_str().is_empty() {
                    let path_buf = path.to_path_buf();
                    if path_buf.is_dir() {
                        self.browse_folder(path_buf);
                    } else if path_buf.is_file() {
                        let ext = path_buf
                            .extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_ascii_lowercase();
                        if ext == "beam" || path_buf.to_string_lossy().ends_with(".beam.ron") {
                            self.request_load_project_file(path_buf);
                        } else {
                            self.request_load_file_path(path_buf);
                        }
                        break;
                    }
                }
            }
        }

        // Top menu bar
        Panel::top("menu_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.image(egui::load::SizedTexture::new(
                    self.logo_texture.id(),
                    Vec2::new(18.0, 18.0),
                ));
                ui.add_space(4.0);
                ui.label(RichText::new(APP_NAME).strong().size(14.5));
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(4.0);

                MenuBar::new().ui(ui, |ui| {
                    ui.menu_button(t!("menu_file"), |ui| {
                        if ui.button(t!("menu_import_folder_shortcut")).clicked() {
                            self.request_import_folder_dialog();
                        }
                        if ui.button(t!("menu_open_photo_shortcut")).clicked() {
                            self.request_open_file_dialog();
                        }
                        if ui.button(t!("menu_open_project_shortcut")).clicked() {
                            self.request_open_project_dialog();
                        }
                        ui.separator();
                        let can_save = self.current_image.is_some() || self.current_path.is_some();
                        if ui
                            .add_enabled(
                                can_save,
                                egui::Button::new(t!("menu_save_project_shortcut")),
                            )
                            .clicked()
                        {
                            self.save_project();
                        }
                        if ui
                            .add_enabled(
                                can_save,
                                egui::Button::new(t!("menu_save_project_as_shortcut")),
                            )
                            .clicked()
                        {
                            self.save_project_as_dialog();
                        }
                        let can_close = self.current_image.is_some() || self.current_path.is_some();
                        if ui
                            .add_enabled(
                                can_close,
                                egui::Button::new(t!("menu_close_project_shortcut")),
                            )
                            .clicked()
                        {
                            self.request_close_project();
                        }
                        ui.separator();
                        let can_export = (self.current_view == AppView::Develop
                            && self.current_image.is_some())
                            || (self.current_view == AppView::Library
                                && !self.library_state.photos.is_empty());
                        if ui
                            .add_enabled(can_export, egui::Button::new(t!("menu_export_shortcut")))
                            .clicked()
                        {
                            self.open_export_dialog();
                        }
                        ui.separator();
                        if ui.button(t!("menu_quit")).clicked() {
                            self.request_exit(ui.ctx());
                        }
                    });

                    ui.menu_button(t!("menu_edit"), |ui| {
                        let can_undo = self.history_index > 0 && !self.history.is_empty();
                        if ui
                            .add_enabled(can_undo, egui::Button::new(t!("menu_undo")))
                            .clicked()
                        {
                            self.undo(frame);
                            ui.close();
                        }
                        let can_redo = self.history_index + 1 < self.history.len();
                        if ui
                            .add_enabled(can_redo, egui::Button::new(t!("menu_redo")))
                            .clicked()
                        {
                            self.redo(frame);
                            ui.close();
                        }
                        ui.separator();
                        let can_copy = self.current_image.is_some();
                        if ui
                            .add_enabled(
                                can_copy,
                                egui::Button::new(format!("{} (Ctrl+C)", t!("menu_copy_settings"))),
                            )
                            .clicked()
                        {
                            self.show_copy_dialog = true;
                            self.copy_dialog_mask = PresetGroupMask::default();
                            ui.close();
                        }
                        let can_paste =
                            self.copied_settings.is_some() && self.current_image.is_some();
                        if ui
                            .add_enabled(
                                can_paste,
                                egui::Button::new(format!(
                                    "{} (Ctrl+V)",
                                    t!("menu_paste_settings")
                                )),
                            )
                            .clicked()
                        {
                            self.paste_settings(frame);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(t!("menu_settings")).clicked() {
                            self.show_settings = true;
                            ui.close();
                        }
                    });

                    ui.menu_button(t!("menu_library"), |ui| {
                        if ui.button(t!("menu_import_folder_shortcut")).clicked() {
                            self.import_folder_dialog();
                        }
                        ui.separator();
                        if ui
                            .button(t!("menu_rebuild_thumbnails"))
                            .on_hover_text(t!("menu_rebuild_thumbnails_tooltip"))
                            .clicked()
                        {
                            self.rebuild_thumbnails();
                        }
                    });

                    ui.menu_button(t!("menu_view"), |ui| {
                        ui.selectable_value(
                            &mut self.current_view,
                            AppView::Library,
                            t!("menu_view_library"),
                        );
                        ui.selectable_value(
                            &mut self.current_view,
                            AppView::Develop,
                            t!("menu_view_develop"),
                        );
                        ui.separator();
                        ui.checkbox(&mut self.show_develop_panel, t!("menu_develop_panel"));
                        ui.checkbox(&mut self.show_history_panel, t!("menu_history_panel"));
                    });

                    ui.menu_button(t!("menu_help"), |ui| {
                        if ui.button(t!("menu_about")).clicked() {
                            self.show_about = true;
                        }
                    });
                });

                // Right: Mode Switcher (Library / Develop) Segmented Pill Control
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(4.0);
                    let is_lib = self.current_view == AppView::Library;
                    let is_dev = self.current_view == AppView::Develop;

                    egui::Frame::new()
                        .fill(Color32::from_rgb(17, 18, 23))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(38, 41, 52)))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::symmetric(2, 2))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;

                                let (dev_bg, dev_stroke, dev_fg) = if is_dev {
                                    (
                                        Color32::from_rgb(46, 56, 78),
                                        Stroke::new(1.0, Color32::from_rgb(70, 92, 138)),
                                        Color32::from_rgb(250, 252, 255),
                                    )
                                } else {
                                    (
                                        Color32::TRANSPARENT,
                                        Stroke::NONE,
                                        Color32::from_rgb(160, 166, 178),
                                    )
                                };
                                let dev_btn = egui::Button::new(
                                    RichText::new(format!("{} (D)", t!("view_develop")))
                                        .size(11.5)
                                        .color(dev_fg)
                                        .strong(),
                                )
                                .fill(dev_bg)
                                .stroke(dev_stroke)
                                .corner_radius(4.0);
                                if ui.add(dev_btn).clicked() {
                                    self.current_view = AppView::Develop;
                                }

                                let (lib_bg, lib_stroke, lib_fg) = if is_lib {
                                    (
                                        Color32::from_rgb(46, 56, 78),
                                        Stroke::new(1.0, Color32::from_rgb(70, 92, 138)),
                                        Color32::from_rgb(250, 252, 255),
                                    )
                                } else {
                                    (
                                        Color32::TRANSPARENT,
                                        Stroke::NONE,
                                        Color32::from_rgb(160, 166, 178),
                                    )
                                };
                                let lib_btn = egui::Button::new(
                                    RichText::new(format!("{} (G)", t!("view_library")))
                                        .size(11.5)
                                        .color(lib_fg)
                                        .strong(),
                                )
                                .fill(lib_bg)
                                .stroke(lib_stroke)
                                .corner_radius(4.0);
                                if ui.add(lib_btn).clicked() {
                                    self.current_view = AppView::Library;
                                }
                            });
                        });
                });
            });
        });

        // Global Keyboard shortcuts
        let typing = ctx.egui_wants_keyboard_input();
        if ctx.input(|i| {
            (i.modifiers.command || i.modifiers.ctrl)
                && i.modifiers.shift
                && i.key_pressed(egui::Key::Z)
        }) || ctx
            .input(|i| (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::Y))
        {
            self.redo(frame);
            ctx.request_repaint();
        } else if ctx
            .input(|i| (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::Z))
        {
            self.undo(frame);
            ctx.request_repaint();
        }

        if !typing
            && ctx.input(|i| {
                i.key_pressed(egui::Key::Backslash) && !i.modifiers.command && !i.modifiers.ctrl
            })
        {
            if self.crop_mode_active {
                self.crop_mode_active = false;
                self.re_render_edits(frame);
            }
            self.toggle_before_after();
            ctx.request_repaint();
        }

        if self.current_view == AppView::Develop {
            if ctx.input(|i| {
                (i.modifiers.command || i.modifiers.ctrl)
                    && !i.modifiers.shift
                    && i.key_pressed(egui::Key::C)
            }) {
                if self.current_image.is_some() {
                    self.show_copy_dialog = true;
                    self.copy_dialog_mask = PresetGroupMask::default();
                }
            } else if ctx.input(|i| {
                (i.modifiers.command || i.modifiers.ctrl)
                    && !i.modifiers.shift
                    && i.key_pressed(egui::Key::V)
            }) {
                if self.current_image.is_some() {
                    self.paste_settings(frame);
                    ctx.request_repaint();
                }
            } else if !typing
                && ctx.input(|i| {
                    i.key_pressed(egui::Key::C) && !i.modifiers.command && !i.modifiers.ctrl
                })
            {
                self.crop_mode_active = !self.crop_mode_active;
                if self.crop_mode_active {
                    self.before_after_mode = BeforeAfterMode::Normal;
                }
                self.re_render_edits(frame);
                ctx.request_repaint();
            } else if !typing
                && self.crop_mode_active
                && ctx.input(|i| {
                    (i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape))
                        && !i.modifiers.command
                        && !i.modifiers.ctrl
                })
            {
                self.crop_mode_active = false;
                self.re_render_edits(frame);
                ctx.request_repaint();
            }
        }

        if !typing
            && ctx
                .input(|i| i.key_pressed(egui::Key::G) && !i.modifiers.command && !i.modifiers.ctrl)
        {
            if self.crop_mode_active {
                self.crop_mode_active = false;
                self.re_render_edits(frame);
            }
            self.current_view = AppView::Library;
        }
        if !typing
            && ctx
                .input(|i| i.key_pressed(egui::Key::D) && !i.modifiers.command && !i.modifiers.ctrl)
        {
            self.current_view = AppView::Develop;
        }
        if ctx.input(|i| {
            (i.modifiers.command || i.modifiers.ctrl)
                && !i.modifiers.shift
                && i.key_pressed(egui::Key::W)
        }) {
            self.request_close_project();
        }
        if ctx.input(|i| {
            (i.modifiers.command || i.modifiers.ctrl)
                && !i.modifiers.shift
                && i.key_pressed(egui::Key::O)
        }) {
            self.request_open_file_dialog();
        }
        if ctx.input(|i| {
            (i.modifiers.command || i.modifiers.ctrl)
                && i.modifiers.shift
                && i.key_pressed(egui::Key::O)
        }) {
            self.request_import_folder_dialog();
        }
        if ctx.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::S)) {
            self.save_project_as_dialog();
        } else if ctx
            .input(|i| i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::S))
        {
            self.save_project();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::E)) {
            self.open_export_dialog();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Comma)) {
            self.show_settings = true;
        }

        // Milestone 13: Ratings (0-5)
        if !typing
            && self.app_config.shortcuts.number_keys_rate
            && !ctx.input(|i| i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt)
        {
            if ctx.input(|i| i.key_pressed(egui::Key::Num0)) {
                self.set_active_photo_rating(0);
                ctx.request_repaint();
            } else if ctx.input(|i| i.key_pressed(egui::Key::Num1)) {
                self.set_active_photo_rating(1);
                ctx.request_repaint();
            } else if ctx.input(|i| i.key_pressed(egui::Key::Num2)) {
                self.set_active_photo_rating(2);
                ctx.request_repaint();
            } else if ctx.input(|i| i.key_pressed(egui::Key::Num3)) {
                self.set_active_photo_rating(3);
                ctx.request_repaint();
            } else if ctx.input(|i| i.key_pressed(egui::Key::Num4)) {
                self.set_active_photo_rating(4);
                ctx.request_repaint();
            } else if ctx.input(|i| i.key_pressed(egui::Key::Num5)) {
                self.set_active_photo_rating(5);
                ctx.request_repaint();
            } else if ctx.input(|i| i.key_pressed(egui::Key::Num6)) {
                self.set_active_photo_color_label(ColorLabel::Red);
                ctx.request_repaint();
            } else if ctx.input(|i| i.key_pressed(egui::Key::Num7)) {
                self.set_active_photo_color_label(ColorLabel::Yellow);
                ctx.request_repaint();
            } else if ctx.input(|i| i.key_pressed(egui::Key::Num8)) {
                self.set_active_photo_color_label(ColorLabel::Green);
                ctx.request_repaint();
            } else if ctx.input(|i| i.key_pressed(egui::Key::Num9)) {
                self.set_active_photo_color_label(ColorLabel::Blue);
                ctx.request_repaint();
            }
        }

        // Milestone 13: Navigation (Left/Right Arrows, PageUp/PageDown)
        if !typing
            && self.app_config.shortcuts.arrow_keys_navigate
            && !ctx.input(|i| i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt)
        {
            if ctx
                .input(|i| i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::PageUp))
            {
                self.navigate_photo(false);
                ctx.request_repaint();
            } else if ctx.input(|i| {
                i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::PageDown)
            }) {
                self.navigate_photo(true);
                ctx.request_repaint();
            }
        }

        // Milestone 13: Fullscreen (F11 or F)
        if !typing
            && self.app_config.shortcuts.f_fullscreen
            && ctx.input(|i| {
                (i.key_pressed(egui::Key::F11) || i.key_pressed(egui::Key::F))
                    && !i.modifiers.command
                    && !i.modifiers.ctrl
            })
        {
            self.is_fullscreen = !self.is_fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.is_fullscreen));
            ctx.request_repaint();
        }

        // Milestone 13: Zoom to 100% / Fit (Z)
        if !typing
            && self.app_config.shortcuts.z_zoom
            && self.current_view == AppView::Develop
            && ctx.input(|i| {
                i.key_pressed(egui::Key::Z)
                    && !i.modifiers.command
                    && !i.modifiers.ctrl
                    && !i.modifiers.shift
            })
        {
            self.is_one_to_one = !self.is_one_to_one;
            if self.is_one_to_one {
                self.ensure_full_res_rendered(frame);
            } else {
                self.zoom_level = 1.0;
                self.pan_offset = Vec2::ZERO;
            }
            ctx.request_repaint();
        }

        // Milestone 13: Clipping Warnings toggle (J)
        if !typing
            && self.app_config.shortcuts.j_clipping
            && self.current_view == AppView::Develop
            && ctx
                .input(|i| i.key_pressed(egui::Key::J) && !i.modifiers.command && !i.modifiers.ctrl)
        {
            let next_clipping = !(self.show_highlight_clipping || self.show_shadow_clipping);
            self.show_highlight_clipping = next_clipping;
            self.show_shadow_clipping = next_clipping;
            self.re_render_edits(frame);
            ctx.request_repaint();
        }

        // Bottom status bar
        Panel::bottom("status_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(path) = &self.current_path {
                    let fname = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Unknown");
                    ui.label(RichText::new(fname).monospace());

                    if let Some(img) = &self.current_image {
                        ui.separator();
                        ui.label(format!("{} × {} px", img.width, img.height));
                    }

                    if let Some(meta) = &self.current_metadata {
                        if let (Some(make), Some(model)) = (&meta.camera_make, &meta.camera_model) {
                            ui.separator();
                            ui.label(format!("{make} {model}"));
                        }
                    }

                    ui.separator();
                    ui.label(t!("status_sidecar", "status" => &self.sidecar_status));
                } else {
                    ui.label(t!("status_no_image_hint"));
                }

                if self.is_loading {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spinner();
                        if let Some(fname) = &self.loading_filename {
                            ui.label(t!("develop_decoding_with_name", "name" => fname));
                        } else {
                            ui.label(t!("develop_decoding"));
                        }
                    });
                } else if self.current_view == AppView::Develop {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let filmstrip_text = if self.filmstrip_visible {
                            t!("develop_filmstrip")
                        } else {
                            format!("{} (▲)", t!("develop_filmstrip"))
                        };
                        if ui
                            .selectable_label(self.filmstrip_visible, filmstrip_text)
                            .clicked()
                        {
                            self.filmstrip_visible = !self.filmstrip_visible;
                            self.app_config.filmstrip_visible = self.filmstrip_visible;
                            let _ = self.app_config.save();
                        }
                    });
                }
            });
        });

        if self.current_view == AppView::Library && self.error_message.is_some() {
            Panel::top("library_error_banner").show(ui, |ui| {
                let mut clear_error = false;
                if let Some(err) = &self.error_message {
                    ui.horizontal(|ui| {
                        ui.colored_label(Color32::RED, t!("develop_error_title"));
                        ui.label(RichText::new(err).color(Color32::LIGHT_RED));
                        if ui.button(t!("develop_dismiss")).clicked() {
                            clear_error = true;
                        }
                    });
                }
                if clear_error {
                    self.error_message = None;
                }
            });
        }

        match self.current_view {
            AppView::Library => {
                let action = show_library_view(ui, &mut self.library_state, &self.library_db);
                if !self.library_state.pending_thumbnail_requests.is_empty() {
                    let reqs = std::mem::take(&mut self.library_state.pending_thumbnail_requests);
                    self.queue_thumbnail_requests(reqs);
                }
                match action {
                    LibraryAction::OpenPhoto(path) => {
                        self.request_load_file_path(path);
                    }
                    LibraryAction::SelectFolderDialog => {
                        let choose_title = t!("browser_choose_folder");
                        let mut dialog = rfd::FileDialog::new().set_title(choose_title);
                        if let Some(ref current) = self.library_state.active_folder {
                            if current.exists() {
                                dialog = dialog.set_directory(current);
                            }
                        }
                        if let Some(folder) = dialog.pick_folder() {
                            self.browse_folder(folder);
                        }
                    }
                    LibraryAction::ChangeFolder(folder) => {
                        self.browse_folder(folder);
                    }
                    LibraryAction::ToggleBookmark(folder) => {
                        if let Some(pos) = self
                            .library_state
                            .bookmarked_folders
                            .iter()
                            .position(|p| p == &folder)
                        {
                            self.library_state.bookmarked_folders.remove(pos);
                        } else {
                            self.library_state.bookmarked_folders.push(folder);
                        }
                        self.app_config.bookmarked_folders =
                            self.library_state.bookmarked_folders.clone();
                        let _ = self.app_config.save();
                    }
                    LibraryAction::ToggleRecursive(rec) => {
                        self.library_state.filter.recursive = rec;
                        self.app_config.recursive_browse = rec;
                        let _ = self.app_config.save();
                        if let Some(ref folder) = self.library_state.active_folder {
                            self.import_folder_path(folder.clone());
                        }
                        self.library_state.needs_refresh = true;
                    }
                    LibraryAction::ExportPhotos(paths) => {
                        self.open_export_dialog_with_paths(paths);
                    }
                    LibraryAction::RebuildThumbnails => {
                        self.rebuild_thumbnails();
                    }
                    LibraryAction::SyncSettings => {
                        if let Some(source_id) = self.library_state.selected_photo_id {
                            if let Ok(Some(photo)) = self.library_db.get_photo(source_id) {
                                let path = PathBuf::from(&photo.file_path);
                                let sidecar_p = Sidecar::path_for(&path);
                                let stack = if let Ok(sidecar) = Sidecar::load_from_file(&sidecar_p)
                                {
                                    sidecar.edit_stack
                                } else {
                                    EditStack::default()
                                };
                                let fname = path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("image")
                                    .to_string();
                                self.sync_dialog_source_photo = Some((source_id, fname, stack));
                                self.sync_dialog_mask = PresetGroupMask::default();
                                self.show_sync_dialog = true;
                            }
                        }
                    }
                    LibraryAction::OpenFileDialog => {
                        self.request_open_file_dialog();
                    }
                    LibraryAction::RelocatePhoto(photo_id, old_path) => {
                        let locate_title = t!("library_locate_title");
                        let mut dialog = rfd::FileDialog::new().set_title(locate_title);
                        if let Some(parent) = old_path.parent() {
                            if parent.exists() {
                                dialog = dialog.set_directory(parent);
                            }
                        }
                        if let Some(name) = old_path.file_name().and_then(|n| n.to_str()) {
                            dialog = dialog.set_file_name(name);
                        }
                        if let Some(new_file) = dialog.pick_file() {
                            let _ = self.library_db.update_photo_path(photo_id, &new_file);
                            if let (Some(old_parent), Some(new_parent)) =
                                (old_path.parent(), new_file.parent())
                            {
                                if old_parent != new_parent {
                                    let _ = self.library_db.relink_folder(old_parent, new_parent);
                                }
                            }
                            self.library_state.needs_refresh = true;
                        }
                    }
                    LibraryAction::RelinkFolder => {
                        let relink_title = t!("library_relink_folder_title");
                        if let Some(new_dir) =
                            rfd::FileDialog::new().set_title(relink_title).pick_folder()
                        {
                            let mut old_dirs = std::collections::HashSet::new();
                            for id in &self.library_state.missing_photo_ids {
                                if let Ok(Some(photo)) = self.library_db.get_photo(*id) {
                                    if let Some(parent) = Path::new(&photo.file_path).parent() {
                                        old_dirs.insert(parent.to_path_buf());
                                    }
                                }
                            }
                            for old_dir in old_dirs {
                                let _ = self.library_db.relink_folder(&old_dir, &new_dir);
                            }
                            self.library_state.needs_refresh = true;
                        }
                    }
                    LibraryAction::CleanMissingPhotos => {
                        let _ = self.library_db.remove_missing_photos();
                        self.library_state.needs_refresh = true;
                    }
                    LibraryAction::RemovePhoto(photo_id) => {
                        let _ = self.library_db.delete_photo(photo_id);
                        self.library_state.needs_refresh = true;
                    }
                    LibraryAction::CreateVirtualCopy(photo_id) => {
                        if let Err(e) = self.library_db.create_virtual_copy(photo_id) {
                            log::error!("Failed to create virtual copy: {e}");
                        } else {
                            self.library_state.needs_refresh = true;
                        }
                    }
                    LibraryAction::OpenVirtualCopy(photo_path, vc_sidecar_path) => {
                        if let Ok(sc) = Sidecar::load_from_file(&vc_sidecar_path) {
                            self.pending_project_sidecar = Some((vc_sidecar_path, sc));
                        }
                        self.request_load_file_path(photo_path);
                    }
                    LibraryAction::DeleteVirtualCopy(copy_id) => {
                        let _ = self.library_db.delete_virtual_copy(copy_id);
                        self.library_state.needs_refresh = true;
                    }
                    LibraryAction::None => {}
                }
            }
            AppView::Develop => {
                // Left-side History Panel
                if self.show_history_panel && self.current_image.is_some() {
                    Panel::left("develop_history_panel")
                        .resizable(true)
                        .default_size(250.0)
                        .min_size(200.0)
                        .show(ui, |ui| {
                            egui::Frame::new()
                                .fill(Color32::from_rgb(17, 18, 23))
                                .stroke(Stroke::new(1.0, Color32::from_rgb(38, 41, 52)))
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(2, 2))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 2.0;
                                        let tab_w = ((ui.available_width() - 4.0) / 3.0).max(10.0);
                                        let is_presets =
                                            self.left_panel_tab == LeftPanelTab::Presets;
                                        let (p_bg, p_stroke, p_fg) = if is_presets {
                                            (
                                                Color32::from_rgb(46, 56, 78),
                                                Stroke::new(1.0, Color32::from_rgb(70, 92, 138)),
                                                Color32::from_rgb(250, 252, 255),
                                            )
                                        } else {
                                            (
                                                Color32::TRANSPARENT,
                                                Stroke::NONE,
                                                Color32::from_rgb(160, 166, 178),
                                            )
                                        };
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(t!("develop_presets"))
                                                        .size(11.5)
                                                        .color(p_fg)
                                                        .strong(),
                                                )
                                                .fill(p_bg)
                                                .stroke(p_stroke)
                                                .corner_radius(5.0)
                                                .min_size(Vec2::new(tab_w, 24.0)),
                                            )
                                            .clicked()
                                        {
                                            self.left_panel_tab = LeftPanelTab::Presets;
                                        }

                                        let is_hist = self.left_panel_tab == LeftPanelTab::History;
                                        let (h_bg, h_stroke, h_fg) = if is_hist {
                                            (
                                                Color32::from_rgb(46, 56, 78),
                                                Stroke::new(1.0, Color32::from_rgb(70, 92, 138)),
                                                Color32::from_rgb(250, 252, 255),
                                            )
                                        } else {
                                            (
                                                Color32::TRANSPARENT,
                                                Stroke::NONE,
                                                Color32::from_rgb(160, 166, 178),
                                            )
                                        };
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(t!("develop_history"))
                                                        .size(11.5)
                                                        .color(h_fg)
                                                        .strong(),
                                                )
                                                .fill(h_bg)
                                                .stroke(h_stroke)
                                                .corner_radius(5.0)
                                                .min_size(Vec2::new(tab_w, 24.0)),
                                            )
                                            .clicked()
                                        {
                                            self.left_panel_tab = LeftPanelTab::History;
                                        }

                                        let is_snap =
                                            self.left_panel_tab == LeftPanelTab::Snapshots;
                                        let (s_bg, s_stroke, s_fg) = if is_snap {
                                            (
                                                Color32::from_rgb(46, 56, 78),
                                                Stroke::new(1.0, Color32::from_rgb(70, 92, 138)),
                                                Color32::from_rgb(250, 252, 255),
                                            )
                                        } else {
                                            (
                                                Color32::TRANSPARENT,
                                                Stroke::NONE,
                                                Color32::from_rgb(160, 166, 178),
                                            )
                                        };
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(t!("develop_snapshots"))
                                                        .size(11.5)
                                                        .color(s_fg)
                                                        .strong(),
                                                )
                                                .fill(s_bg)
                                                .stroke(s_stroke)
                                                .corner_radius(5.0)
                                                .min_size(Vec2::new(tab_w, 24.0)),
                                            )
                                            .clicked()
                                        {
                                            self.left_panel_tab = LeftPanelTab::Snapshots;
                                        }
                                    });
                                });
                            ui.add_space(4.0);
                            ui.separator();
                            ui.add_space(4.0);

                            match self.left_panel_tab {
                                LeftPanelTab::Presets => {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        let btn_w = ((ui.available_width() - 4.0) / 2.0).max(10.0);
                                        let save_btn = egui::Button::new(
                                            RichText::new(format!("+ {}", t!("preset_save_btn")))
                                                .size(11.5)
                                                .color(Color32::from_rgb(230, 240, 255))
                                                .strong(),
                                        )
                                        .fill(Color32::from_rgb(36, 54, 86))
                                        .stroke(Stroke::new(1.0, Color32::from_rgb(62, 92, 148)))
                                        .corner_radius(5.0)
                                        .min_size(Vec2::new(btn_w, 26.0));
                                        if ui.add(save_btn).clicked() {
                                            self.show_new_preset_dialog = true;
                                            self.new_preset_name = String::new();
                                            self.new_preset_mask = PresetGroupMask::default();
                                        }

                                        let import_btn = egui::Button::new(
                                            RichText::new(t!("preset_import_btn"))
                                                .size(11.5)
                                                .color(Color32::from_rgb(200, 208, 222)),
                                        )
                                        .fill(Color32::from_rgb(26, 28, 36))
                                        .stroke(Stroke::new(1.0, Color32::from_rgb(46, 50, 64)))
                                        .corner_radius(5.0)
                                        .min_size(Vec2::new(btn_w, 26.0));
                                        if ui.add(import_btn).clicked() {
                                            self.import_preset_dialog();
                                        }
                                    });
                                    ui.add_space(6.0);

                                    let mut apply_preset = None;
                                    let mut delete_preset_id = None;
                                    let mut export_preset = None;
                                    let mut current_hovered_preset = None;

                                    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(
                                        ui,
                                        |ui| {
                                            // 1. Built-in Category
                                            ui.label(
                                                RichText::new(
                                                    t!("preset_builtin_category").to_uppercase(),
                                                )
                                                .size(10.5)
                                                .strong()
                                                .color(Color32::from_rgb(130, 136, 150)),
                                            );
                                            ui.add_space(3.0);
                                            for preset in &built_in_presets() {
                                                let localized_name = match preset.name.as_str() {
                                                    "preset_name_neutral" => {
                                                        t!("preset_name_neutral")
                                                    }
                                                    "preset_name_vivid" => t!("preset_name_vivid"),
                                                    "preset_name_matte" => t!("preset_name_matte"),
                                                    "preset_name_bw" => t!("preset_name_bw"),
                                                    "preset_name_warm_film" => {
                                                        t!("preset_name_warm_film")
                                                    }
                                                    "preset_name_cool_cinematic" => {
                                                        t!("preset_name_cool_cinematic")
                                                    }
                                                    "preset_name_punchy_contrast" => {
                                                        t!("preset_name_punchy_contrast")
                                                    }
                                                    "preset_name_moody_landscape" => {
                                                        t!("preset_name_moody_landscape")
                                                    }
                                                    _ => preset.name.clone(),
                                                };
                                                let desc = preset
                                                    .description
                                                    .as_deref()
                                                    .map(|d| match d {
                                                        "preset_desc_neutral" => {
                                                            t!("preset_desc_neutral")
                                                        }
                                                        "preset_desc_vivid" => {
                                                            t!("preset_desc_vivid")
                                                        }
                                                        "preset_desc_matte" => {
                                                            t!("preset_desc_matte")
                                                        }
                                                        "preset_desc_bw" => t!("preset_desc_bw"),
                                                        "preset_desc_warm_film" => {
                                                            t!("preset_desc_warm_film")
                                                        }
                                                        "preset_desc_cool_cinematic" => {
                                                            t!("preset_desc_cool_cinematic")
                                                        }
                                                        "preset_desc_punchy_contrast" => {
                                                            t!("preset_desc_punchy_contrast")
                                                        }
                                                        "preset_desc_moody_landscape" => {
                                                            t!("preset_desc_moody_landscape")
                                                        }
                                                        _ => d.to_string(),
                                                    })
                                                    .unwrap_or_default();

                                                let p_btn = egui::Button::new(
                                                    RichText::new(format!("▸  {localized_name}"))
                                                        .size(11.5)
                                                        .color(Color32::from_rgb(205, 212, 225)),
                                                )
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(Stroke::NONE)
                                                .corner_radius(4.0)
                                                .min_size(Vec2::new(ui.available_width(), 24.0));

                                                let resp = ui.add(p_btn).on_hover_text(format!(
                                                    "{}\n\n{}",
                                                    desc,
                                                    t!("preset_hover_preview")
                                                ));
                                                if resp.hovered() {
                                                    current_hovered_preset = Some(preset.clone());
                                                }
                                                if resp.clicked() {
                                                    apply_preset =
                                                        Some((localized_name, preset.clone()));
                                                }
                                            }

                                            ui.add_space(8.0);
                                            ui.separator();
                                            ui.add_space(4.0);

                                            // 2. User Presets Category
                                            ui.label(
                                                RichText::new(
                                                    t!("preset_user_category").to_uppercase(),
                                                )
                                                .size(10.5)
                                                .strong()
                                                .color(Color32::from_rgb(130, 136, 150)),
                                            );
                                            ui.add_space(3.0);
                                            if self.user_presets.is_empty() {
                                                ui.label(
                                                    RichText::new(t!("preset_empty_user"))
                                                        .weak()
                                                        .size(11.0),
                                                );
                                            } else {
                                                for preset in &self.user_presets {
                                                    ui.horizontal(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        let row_w =
                                                            (ui.available_width() - 52.0).max(10.0);
                                                        let p_btn =
                                                            egui::Button::new(
                                                                RichText::new(format!(
                                                                    "★  {}",
                                                                    preset.name
                                                                ))
                                                                .size(11.5)
                                                                .color(Color32::from_rgb(
                                                                    215, 222, 235,
                                                                )),
                                                            )
                                                            .fill(Color32::TRANSPARENT)
                                                            .stroke(Stroke::NONE)
                                                            .corner_radius(4.0)
                                                            .min_size(Vec2::new(row_w, 24.0));

                                                        let resp = ui.add(p_btn).on_hover_text(t!(
                                                            "preset_hover_preview"
                                                        ));
                                                        if resp.hovered() {
                                                            current_hovered_preset =
                                                                Some(preset.clone());
                                                        }
                                                        if resp.clicked() {
                                                            apply_preset = Some((
                                                                preset.name.clone(),
                                                                preset.clone(),
                                                            ));
                                                        }

                                                        let exp_btn = egui::Button::new(
                                                            RichText::new("↗").size(11.0),
                                                        )
                                                        .fill(Color32::from_rgb(24, 26, 34))
                                                        .stroke(Stroke::new(
                                                            1.0,
                                                            Color32::from_rgb(42, 46, 58),
                                                        ))
                                                        .corner_radius(3.0)
                                                        .min_size(Vec2::new(22.0, 22.0));
                                                        if ui
                                                            .add(exp_btn)
                                                            .on_hover_text(t!("preset_export_btn"))
                                                            .clicked()
                                                        {
                                                            export_preset = Some(preset.clone());
                                                        }

                                                        let del_btn = egui::Button::new(
                                                            RichText::new("✕").size(11.0).color(
                                                                Color32::from_rgb(220, 100, 100),
                                                            ),
                                                        )
                                                        .fill(Color32::from_rgb(28, 20, 20))
                                                        .stroke(Stroke::new(
                                                            1.0,
                                                            Color32::from_rgb(58, 30, 30),
                                                        ))
                                                        .corner_radius(3.0)
                                                        .min_size(Vec2::new(22.0, 22.0));
                                                        if ui
                                                            .add(del_btn)
                                                            .on_hover_text(t!("preset_delete_btn"))
                                                            .clicked()
                                                        {
                                                            delete_preset_id =
                                                                Some(preset.id.clone());
                                                        }
                                                    });
                                                }
                                            }
                                        },
                                    );

                                    // Handle hover preview
                                    if current_hovered_preset != self.hovered_preset {
                                        if let Some(p) = &current_hovered_preset {
                                            let orig_stack = self
                                                .stack_before_hover
                                                .get_or_insert_with(|| self.edit_stack.clone());
                                            let mut preview_stack = orig_stack.clone();
                                            p.apply_to(&mut preview_stack);
                                            self.edit_stack = preview_stack;
                                            self.re_render_edits(frame);
                                        } else if let Some(orig) = self.stack_before_hover.take() {
                                            self.edit_stack = orig;
                                            self.re_render_edits(frame);
                                        }
                                        self.hovered_preset = current_hovered_preset;
                                        ctx.request_repaint();
                                    }

                                    // Handle apply preset click
                                    if let Some((name, preset)) = apply_preset {
                                        if let Some(orig) = self.stack_before_hover.take() {
                                            self.edit_stack = orig;
                                        }
                                        self.hovered_preset = None;
                                        preset.apply_to(&mut self.edit_stack);
                                        self.push_history_step(
                                            t!("history_apply_preset", "name" => name),
                                        );
                                        self.re_render_edits(frame);
                                        ctx.request_repaint();
                                    }

                                    // Handle delete preset
                                    if let Some(id) = delete_preset_id {
                                        let _ = delete_user_preset(&id, &self.preset_dir);
                                        self.user_presets = load_user_presets(&self.preset_dir);
                                    }

                                    // Handle export preset
                                    if let Some(preset) = export_preset {
                                        if let Some(dest) = rfd::FileDialog::new()
                                            .set_file_name(format!("{}.beam.preset.ron", preset.id))
                                            .add_filter("beaM Preset", &["ron"])
                                            .save_file()
                                        {
                                            if let Ok(s) = preset.to_ron_string() {
                                                let _ = std::fs::write(dest, s);
                                            }
                                        }
                                    }
                                }
                                LeftPanelTab::History => {
                                    // If hover was active when switching tabs, restore
                                    if let Some(orig) = self.stack_before_hover.take() {
                                        self.edit_stack = orig;
                                        self.re_render_edits(frame);
                                        self.hovered_preset = None;
                                    }

                                    ui.horizontal(|ui| {
                                        let can_undo =
                                            self.history_index > 0 && !self.history.is_empty();
                                        if ui
                                            .add_enabled(
                                                can_undo,
                                                egui::Button::new(t!("develop_undo_btn")),
                                            )
                                            .clicked()
                                        {
                                            self.undo(frame);
                                        }
                                        let can_redo = self.history_index + 1 < self.history.len();
                                        if ui
                                            .add_enabled(
                                                can_redo,
                                                egui::Button::new(t!("develop_redo_btn")),
                                            )
                                            .clicked()
                                        {
                                            self.redo(frame);
                                        }
                                    });
                                    ui.add_space(4.0);

                                    let mut jump_target = None;
                                    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(
                                        ui,
                                        |ui| {
                                            if self.history.is_empty() {
                                                ui.label(t!("develop_history_empty"));
                                            } else {
                                                for (idx, step) in self.history.iter().enumerate() {
                                                    let is_current = idx == self.history_index;
                                                    let is_past = idx <= self.history_index;

                                                    let label_text = if idx == 0 {
                                                        format!(
                                                            "0. {}",
                                                            t!("develop_history_initial")
                                                        )
                                                    } else {
                                                        format!("{}. {}", idx, step.name)
                                                    };

                                                    let text_color = if is_current {
                                                        Color32::from_rgb(100, 200, 255)
                                                    } else if is_past {
                                                        Color32::WHITE
                                                    } else {
                                                        Color32::GRAY
                                                    };

                                                    let text =
                                                        RichText::new(label_text).color(text_color);
                                                    let response = if is_current {
                                                        ui.selectable_label(true, text.strong())
                                                    } else {
                                                        ui.selectable_label(false, text)
                                                    };

                                                    if response.clicked() {
                                                        jump_target = Some(idx);
                                                    }
                                                }
                                            }
                                        },
                                    );

                                    if let Some(target) = jump_target {
                                        self.jump_to_history_step(frame, target);
                                        ctx.request_repaint();
                                    }
                                }
                                LeftPanelTab::Snapshots => {
                                    if let Some(orig) = self.stack_before_hover.take() {
                                        self.edit_stack = orig;
                                        self.re_render_edits(frame);
                                        self.hovered_preset = None;
                                    }

                                    ui.horizontal(|ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(
                                                &mut self.snapshot_name_input,
                                            )
                                            .hint_text(t!("develop_snapshot_placeholder"))
                                            .desired_width((ui.available_width() - 95.0).max(60.0)),
                                        );
                                        if ui
                                            .button(format!("+ {}", t!("develop_snapshot_save")))
                                            .clicked()
                                        {
                                            let trimmed = self.snapshot_name_input.trim();
                                            let name = if trimmed.is_empty() {
                                                format!("Snapshot {}", self.snapshots.len() + 1)
                                            } else {
                                                trimmed.to_string()
                                            };
                                            self.snapshots.push(HistoryStep::new(
                                                name,
                                                self.edit_stack.clone(),
                                            ));
                                            self.snapshot_name_input.clear();
                                            self.save_sidecar();
                                        }
                                    });
                                    ui.add_space(6.0);

                                    let mut apply_idx: Option<usize> = None;
                                    let mut delete_idx: Option<usize> = None;
                                    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(
                                        ui,
                                        |ui| {
                                            if self.snapshots.is_empty() {
                                                ui.label(
                                                    RichText::new(t!("develop_snapshots_empty"))
                                                        .color(Color32::from_rgb(140, 146, 160))
                                                        .size(12.0),
                                                );
                                            } else {
                                                for (idx, snap) in self.snapshots.iter().enumerate()
                                                {
                                                    ui.horizontal(|ui| {
                                                        let row_w =
                                                            (ui.available_width() - 28.0).max(60.0);
                                                        if ui
                                                            .add_sized(
                                                                [row_w, 24.0],
                                                                egui::Button::new(&snap.name),
                                                            )
                                                            .clicked()
                                                        {
                                                            apply_idx = Some(idx);
                                                        }
                                                        if ui.small_button("×").clicked() {
                                                            delete_idx = Some(idx);
                                                        }
                                                    });
                                                }
                                            }
                                        },
                                    );

                                    if let Some(idx) = apply_idx {
                                        if let Some(snap) = self.snapshots.get(idx).cloned() {
                                            self.edit_stack = snap.stack;
                                            self.re_render_edits(frame);
                                            self.push_history_step(
                                                t!("history_snapshot", "name" => snap.name),
                                            );
                                            ctx.request_repaint();
                                        }
                                    }
                                    if let Some(idx) = delete_idx {
                                        if idx < self.snapshots.len() {
                                            self.snapshots.remove(idx);
                                            self.save_sidecar();
                                        }
                                    }
                                }
                            }
                        });
                }

                // Right-side Develop Adjustment Panel
                let mut edits_changed = false;
                let mut action_label: Option<String> = None;
                let mut immediate_action = false;

                if self.show_develop_panel && self.current_image.is_some() {
                    Panel::right("develop_panel_v7")
                        .resizable(true)
                        .default_size(285.0)
                        .min_size(260.0)
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false; 2])
                                .show(ui, |ui| {
                                    ui.spacing_mut().slider_width = 100.0;
                                    ui.heading(
                                        RichText::new(t!("view_develop"))
                                            .strong()
                                            .size(15.0)
                                            .color(Color32::from_rgb(245, 247, 252)),
                                    );
                                    ui.add_space(4.0);
                                    ui.separator();
                                    ui.add_space(6.0);

                                    let card_frame = egui::Frame::new()
                                        .fill(Color32::from_rgb(26, 28, 34))
                                        .stroke(Stroke::new(1.0, Color32::from_rgb(42, 45, 54)))
                                        .corner_radius(8.0)
                                        .inner_margin(egui::Margin::symmetric(12, 10));

                                    let draw_slider_row =
                                        |ui: &mut egui::Ui,
                                         label: &str,
                                         slider: Slider,
                                         reset_tip: &str|
                                         -> (egui::Response, bool) {
                                            let mut reset_clicked = false;
                                            let resp = ui.horizontal(|ui| {
                                                ui.allocate_ui_with_layout(
                                                    Vec2::new(74.0, ui.spacing().interact_size.y),
                                                    egui::Layout::left_to_right(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        ui.label(
                                                            RichText::new(label)
                                                                .size(11.5)
                                                                .color(Color32::from_rgb(
                                                                    185, 190, 202,
                                                                )),
                                                        );
                                                    },
                                                );
                                                let prev_interact = ui.spacing().interact_size.x;
                                                let prev_slider_w = ui.spacing().slider_width;
                                                let val_w = 46.0;
                                                let reset_w = 24.0;
                                                let slider_w = (ui.available_width()
                                                    - val_w
                                                    - reset_w
                                                    - 24.0)
                                                    .max(40.0);
                                                ui.spacing_mut().interact_size.x = val_w;
                                                ui.spacing_mut().slider_width = slider_w;
                                                let resp = ui.add(slider);
                                                ui.spacing_mut().interact_size.x = prev_interact;
                                                ui.spacing_mut().slider_width = prev_slider_w;
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        if ui
                                                            .button("↺")
                                                            .on_hover_text(reset_tip)
                                                            .clicked()
                                                        {
                                                            reset_clicked = true;
                                                        }
                                                    },
                                                );
                                                resp
                                            }).inner;
                                            (resp, reset_clicked)
                                        };

                                    // 1. Histogram Widget
                                    let mut clipping_changed = false;
                                    let mut auto_tone_vals: Option<(f32, f32, f32, f32, f32)> = None;
                                    if let Some(hist_data) = &self.histogram_data {
                                        card_frame.show(ui, |ui| {
                                            if show_histogram_with_clipping(
                                                ui,
                                                hist_data,
                                                &mut self.histogram_mode,
                                                &mut self.show_shadow_clipping,
                                                &mut self.show_highlight_clipping,
                                            ) {
                                                clipping_changed = true;
                                            }
                                            ui.add_space(4.0);
                                            ui.horizontal(|ui| {
                                                let proof_label = match self.soft_proof_space {
                                                    None => t!("develop_soft_proof_off").to_string(),
                                                    Some(beam_core::ExportColorSpace::Srgb) => "sRGB".to_string(),
                                                    Some(beam_core::ExportColorSpace::DisplayP3) => "Display P3".to_string(),
                                                    Some(beam_core::ExportColorSpace::AdobeRgb) => "Adobe RGB".to_string(),
                                                };
                                                egui::ComboBox::from_id_salt("develop_soft_proof_combo")
                                                    .selected_text(format!("{}: {}", t!("develop_soft_proof"), proof_label))
                                                    .width(140.0)
                                                    .show_ui(ui, |ui| {
                                                        if ui.selectable_value(&mut self.soft_proof_space, None, t!("develop_soft_proof_off")).changed() {
                                                            clipping_changed = true;
                                                        }
                                                        if ui.selectable_value(&mut self.soft_proof_space, Some(beam_core::ExportColorSpace::Srgb), "sRGB").changed() {
                                                            clipping_changed = true;
                                                        }
                                                        if ui.selectable_value(&mut self.soft_proof_space, Some(beam_core::ExportColorSpace::DisplayP3), "Display P3").changed() {
                                                            clipping_changed = true;
                                                        }
                                                        if ui.selectable_value(&mut self.soft_proof_space, Some(beam_core::ExportColorSpace::AdobeRgb), "Adobe RGB").changed() {
                                                            clipping_changed = true;
                                                        }
                                                    });
                                                if self.soft_proof_space.is_some()
                                                    && ui
                                                        .selectable_label(
                                                            self.show_gamut_warning,
                                                            t!("develop_gamut_warning"),
                                                        )
                                                        .clicked()
                                                {
                                                    self.show_gamut_warning = !self.show_gamut_warning;
                                                    clipping_changed = true;
                                                }
                                                if ui
                                                    .selectable_label(
                                                        self.show_focus_peaking,
                                                        t!("develop_focus_peaking"),
                                                    )
                                                    .clicked()
                                                {
                                                    self.show_focus_peaking = !self.show_focus_peaking;
                                                    clipping_changed = true;
                                                }
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(egui::Align::Center),
                                                    |ui| {
                                                        if ui
                                                            .button(t!("develop_auto_tone"))
                                                            .on_hover_text(t!(
                                                                "develop_auto_tone_tooltip"
                                                            ))
                                                            .clicked()
                                                        {
                                                            auto_tone_vals =
                                                                Some(hist_data.compute_auto_tone());
                                                        }
                                                    },
                                                );
                                            });
                                            ui.add_space(4.0);
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(t!("develop_camera_profile"))
                                                        .size(11.5)
                                                        .color(Color32::from_rgb(180, 185, 200)),
                                                );
                                                egui::ComboBox::from_id_salt("develop_camera_profile_combo")
                                                    .selected_text(self.edit_stack.camera_profile.label())
                                                    .width(130.0)
                                                    .show_ui(ui, |ui| {
                                                        for prof in beam_core::sidecar::CameraProfile::ALL {
                                                            if ui
                                                                .selectable_value(
                                                                    &mut self.edit_stack.camera_profile,
                                                                    prof,
                                                                    prof.label(),
                                                                )
                                                                .changed()
                                                            {
                                                                clipping_changed = true;
                                                            }
                                                        }
                                                    });
                                            });
                                        });
                                        ui.add_space(6.0);
                                    }
                                    if let Some((ev, contrast, highlights, shadows, whites)) =
                                        auto_tone_vals
                                    {
                                        self.edit_stack.exposure.enabled = true;
                                        self.edit_stack.exposure.ev = ev;
                                        self.edit_stack.contrast.enabled = true;
                                        self.edit_stack.contrast.contrast = contrast;
                                        self.edit_stack.highlights_shadows.enabled = true;
                                        self.edit_stack.highlights_shadows.highlights = highlights;
                                        self.edit_stack.highlights_shadows.shadows = shadows;
                                        self.edit_stack.highlights_shadows.whites = whites;
                                        self.re_render_edits(frame);
                                        self.push_history_step(t!("history_auto_tone"));
                                        ctx.request_repaint();
                                    }
                                    if clipping_changed {
                                        self.re_render_edits(frame);
                                        ctx.request_repaint();
                                    }

                                    // 2. Crop & Straighten Section
                                    card_frame.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t!("develop_crop_tool"))
                                                    .strong()
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(240, 242, 248)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    // Reset Crop button
                                                    if ui
                                                        .button("↺")
                                                        .on_hover_text(t!("develop_crop_reset"))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.crop = Default::default();
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label =
                                                            Some(t!("history_crop_reset"));
                                                    }

                                                    // Toggle Crop Tool button
                                                    let btn_text = if self.crop_mode_active {
                                                        t!("develop_crop_done")
                                                    } else {
                                                        t!("develop_crop_btn")
                                                    };
                                                    let (btn_bg, btn_stroke, btn_fg) =
                                                        if self.crop_mode_active {
                                                            (
                                                                Color32::from_rgb(46, 75, 135),
                                                                Stroke::new(
                                                                    1.0,
                                                                    Color32::from_rgb(
                                                                        70, 110, 195,
                                                                    ),
                                                                ),
                                                                Color32::from_rgb(250, 252, 255),
                                                            )
                                                        } else {
                                                            (
                                                                Color32::from_rgb(32, 35, 43),
                                                                Stroke::new(
                                                                    1.0,
                                                                    Color32::from_rgb(48, 52, 64),
                                                                ),
                                                                Color32::from_rgb(200, 205, 215),
                                                            )
                                                        };
                                                    let resp = ui.add(
                                                        egui::Button::new(
                                                            RichText::new(btn_text)
                                                                .size(11.5)
                                                                .color(btn_fg)
                                                                .strong(),
                                                        )
                                                        .fill(btn_bg)
                                                        .stroke(btn_stroke)
                                                        .corner_radius(5.0),
                                                    );
                                                    if resp.clicked() {
                                                        self.crop_mode_active =
                                                            !self.crop_mode_active;
                                                        if self.crop_mode_active {
                                                            self.before_after_mode =
                                                                BeforeAfterMode::Normal;
                                                        }
                                                        edits_changed = true;
                                                        immediate_action = false;
                                                    }
                                                },
                                            );
                                        });

                                        ui.add_space(6.0);

                                        // Aspect Preset Selector
                                        ui.horizontal(|ui| {
                                            ui.allocate_ui_with_layout(
                                                Vec2::new(76.0, ui.spacing().interact_size.y),
                                                egui::Layout::left_to_right(
                                                    egui::Align::Center,
                                                ),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(t!("develop_crop_aspect"))
                                                            .size(12.0)
                                                            .color(Color32::from_rgb(
                                                                185, 190, 202,
                                                            )),
                                                    );
                                                },
                                            );
                                            let current_preset = self.edit_stack.crop.preset;
                                            let mut selected_preset = current_preset;
                                            egui::ComboBox::from_id_salt("crop_aspect_combo")
                                                .width(ui.available_width() - 4.0)
                                                .selected_text(match current_preset {
                                                    CropAspectPreset::Original => {
                                                        t!("develop_crop_aspect_original")
                                                    }
                                                    CropAspectPreset::Square => {
                                                        t!("develop_crop_aspect_1_1")
                                                    }
                                                    CropAspectPreset::Ratio4x5 => {
                                                        t!("develop_crop_aspect_4_5")
                                                    }
                                                    CropAspectPreset::Ratio3x2 => {
                                                        t!("develop_crop_aspect_3_2")
                                                    }
                                                    CropAspectPreset::Ratio16x9 => {
                                                        t!("develop_crop_aspect_16_9")
                                                    }
                                                    CropAspectPreset::Free => {
                                                        t!("develop_crop_aspect_free")
                                                    }
                                                })
                                                .show_ui(ui, |ui| {
                                                    ui.selectable_value(
                                                        &mut selected_preset,
                                                        CropAspectPreset::Original,
                                                        t!("develop_crop_aspect_original"),
                                                    );
                                                    ui.selectable_value(
                                                        &mut selected_preset,
                                                        CropAspectPreset::Square,
                                                        t!("develop_crop_aspect_1_1"),
                                                    );
                                                    ui.selectable_value(
                                                        &mut selected_preset,
                                                        CropAspectPreset::Ratio4x5,
                                                        t!("develop_crop_aspect_4_5"),
                                                    );
                                                    ui.selectable_value(
                                                        &mut selected_preset,
                                                        CropAspectPreset::Ratio3x2,
                                                        t!("develop_crop_aspect_3_2"),
                                                    );
                                                    ui.selectable_value(
                                                        &mut selected_preset,
                                                        CropAspectPreset::Ratio16x9,
                                                        t!("develop_crop_aspect_16_9"),
                                                    );
                                                    ui.selectable_value(
                                                        &mut selected_preset,
                                                        CropAspectPreset::Free,
                                                        t!("develop_crop_aspect_free"),
                                                    );
                                                });

                                            if selected_preset != current_preset {
                                                self.edit_stack.crop.preset = selected_preset;
                                                if let Some(img) = &self.current_image {
                                                    let orig_aspect =
                                                        if self.edit_stack.crop.rotation == 90
                                                            || self.edit_stack.crop.rotation == 270
                                                        {
                                                            img.height as f32 / img.width as f32
                                                        } else {
                                                            img.width as f32 / img.height as f32
                                                        };
                                                    let (x, y, w, h) = create_centered_crop_rect(
                                                        orig_aspect,
                                                        selected_preset,
                                                    );
                                                    self.edit_stack.crop.x = x;
                                                    self.edit_stack.crop.y = y;
                                                    self.edit_stack.crop.width = w;
                                                    self.edit_stack.crop.height = h;
                                                    if selected_preset != CropAspectPreset::Free
                                                        && (w < 0.999 || h < 0.999)
                                                    {
                                                        self.edit_stack.crop.enabled = true;
                                                    }
                                                }
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_crop_aspect",
                                                    "preset" => format!("{:?}", selected_preset)
                                                ));
                                            }
                                        });

                                        ui.add_space(4.0);

                                        // Rotate and Flip Buttons
                                        ui.horizontal_wrapped(|ui| {
                                            if ui
                                                .button("↶ 90°")
                                                .on_hover_text(t!("develop_crop_rotate_ccw"))
                                                .clicked()
                                            {
                                                self.edit_stack.crop.rotation =
                                                    (self.edit_stack.crop.rotation + 270) % 360;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_crop_rotate",
                                                    "angle" => {
                                                        format!("{}°", self.edit_stack.crop.rotation)
                                                    }
                                                ));
                                            }
                                            if ui
                                                .button("↷ 90°")
                                                .on_hover_text(t!("develop_crop_rotate_cw"))
                                                .clicked()
                                            {
                                                self.edit_stack.crop.rotation =
                                                    (self.edit_stack.crop.rotation + 90) % 360;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_crop_rotate",
                                                    "angle" => {
                                                        format!("{}°", self.edit_stack.crop.rotation)
                                                    }
                                                ));
                                            }
                                            if ui
                                                .button("⇄ Flip H")
                                                .on_hover_text(t!("develop_crop_flip_h"))
                                                .clicked()
                                            {
                                                self.edit_stack.crop.flip_h =
                                                    !self.edit_stack.crop.flip_h;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!("history_crop_flip_h"));
                                            }
                                            if ui
                                                .button("⇅ Flip V")
                                                .on_hover_text(t!("develop_crop_flip_v"))
                                                .clicked()
                                            {
                                                self.edit_stack.crop.flip_v =
                                                    !self.edit_stack.crop.flip_v;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!("history_crop_flip_v"));
                                            }
                                        });

                                        ui.add_space(4.0);

                                        // Straighten slider
                                        let tip_straighten_reset =
                                            t!("develop_crop_straighten_reset");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_crop_straighten"),
                                            Slider::new(
                                                &mut self.edit_stack.crop.straighten,
                                                -45.0..=45.0,
                                            )
                                            .suffix("°")
                                            .show_value(true),
                                            tip_straighten_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_crop_straighten",
                                                "val" => {
                                                    format!(
                                                        "{:+0.1}°",
                                                        self.edit_stack.crop.straighten
                                                    )
                                                }
                                            ));
                                        }
                                        if reset {
                                            self.edit_stack.crop.straighten = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => t!("develop_crop_straighten")
                                            ));
                                        }

                                        if self.crop_mode_active {
                                            ui.add_space(2.0);
                                            ui.label(
                                                RichText::new(t!("develop_crop_instruction"))
                                                    .weak()
                                                    .small(),
                                            );
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // 3. White Balance Section
                                    card_frame.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t!("develop_white_balance"))
                                                    .strong()
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(240, 242, 248)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button("↺")
                                                        .on_hover_text(t!("develop_reset_wb"))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.reset_white_balance();
                                                        self.wb_eyedropper_active = false;
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_white_balance")
                                                        ));
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self
                                                                .edit_stack
                                                                .white_balance
                                                                .enabled,
                                                            "",
                                                        )
                                                        .changed()
                                                    {
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_toggle_section",
                                                            "section" => t!("develop_white_balance")
                                                        ));
                                                    }
                                                    let picker_btn = egui::Button::new(
                                                        RichText::new(t!("develop_wb_eyedropper"))
                                                            .size(10.5)
                                                            .color(if self.wb_eyedropper_active {
                                                                Color32::from_rgb(130, 195, 255)
                                                            } else {
                                                                Color32::from_rgb(195, 200, 212)
                                                            }),
                                                    )
                                                    .fill(if self.wb_eyedropper_active {
                                                        Color32::from_rgb(38, 62, 96)
                                                    } else {
                                                        Color32::from_rgb(26, 28, 35)
                                                    })
                                                    .stroke(Stroke::new(
                                                        1.0,
                                                        if self.wb_eyedropper_active {
                                                            Color32::from_rgb(80, 145, 230)
                                                        } else {
                                                            Color32::from_rgb(46, 50, 60)
                                                        },
                                                    ))
                                                    .corner_radius(5.0);
                                                    if ui
                                                        .add(picker_btn)
                                                        .on_hover_text(t!("develop_wb_eyedropper_tooltip"))
                                                        .clicked()
                                                    {
                                                        self.wb_eyedropper_active =
                                                            !self.wb_eyedropper_active;
                                                        if self.wb_eyedropper_active {
                                                            self.crop_mode_active = false;
                                                        }
                                                    }
                                                },
                                            );
                                        });

                                        ui.add_space(6.0);

                                        // Temperature slider
                                        let tip_temp_reset = t!("develop_reset_temp");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_temp_slider"),
                                            Slider::new(
                                                &mut self.edit_stack.white_balance.temperature,
                                                -100.0..=100.0,
                                            )
                                            .show_value(true),
                                            tip_temp_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_temperature",
                                                "val" => {
                                                    format!(
                                                        "{:+0.0}",
                                                        self.edit_stack.white_balance.temperature
                                                    )
                                                }
                                            ));
                                        }
                                        if reset {
                                            self.edit_stack.white_balance.temperature = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => t!("develop_wb_temp")
                                            ));
                                        }

                                        // Tint slider
                                        let tip_tint_reset = t!("develop_reset_tint");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_tint_slider"),
                                            Slider::new(
                                                &mut self.edit_stack.white_balance.tint,
                                                -100.0..=100.0,
                                            )
                                            .show_value(true),
                                            tip_tint_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_tint",
                                                "val" => {
                                                    format!(
                                                        "{:+0.0}",
                                                        self.edit_stack.white_balance.tint
                                                    )
                                                }
                                            ));
                                        }
                                        if reset {
                                            self.edit_stack.white_balance.tint = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => t!("develop_wb_tint")
                                            ));
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // 4. Exposure Section
                                    card_frame.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t!("develop_exposure"))
                                                    .strong()
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(240, 242, 248)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button("↺")
                                                        .on_hover_text(t!("develop_reset_exposure"))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.reset_exposure();
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_exposure")
                                                        ));
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self.edit_stack.exposure.enabled,
                                                            "",
                                                        )
                                                        .changed()
                                                    {
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_toggle_section",
                                                            "section" => t!("develop_exposure")
                                                        ));
                                                    }
                                                },
                                            );
                                        });

                                        ui.add_space(6.0);

                                        // Exposure EV slider
                                        let tip_exp_reset = t!("develop_reset_exposure");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_ev_slider"),
                                            Slider::new(
                                                &mut self.edit_stack.exposure.ev,
                                                -5.00..=5.00,
                                            )
                                            .step_by(0.05)
                                            .custom_formatter(|v, _| format!("{:+.2}", v)),
                                            tip_exp_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_exposure",
                                                "val" => {
                                                    format!(
                                                        "{:+0.2}",
                                                        self.edit_stack.exposure.ev
                                                    )
                                                }
                                            ));
                                        }
                                        if reset {
                                            self.edit_stack.exposure.ev = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => t!("develop_exposure")
                                            ));
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // 5. Highlights & Shadows Section
                                    card_frame.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} & {}",
                                                    t!("develop_highlights"),
                                                    t!("develop_shadows")
                                                ))
                                                .strong()
                                                .size(13.0)
                                                .color(Color32::from_rgb(240, 242, 248)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button("↺")
                                                        .on_hover_text(t!(
                                                            "develop_reset_highlights_shadows"
                                                        ))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.reset_highlights_shadows();
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_reset_section",
                                                            "section" => {
                                                                format!(
                                                                    "{} & {}",
                                                                    t!("develop_highlights"),
                                                                    t!("develop_shadows")
                                                                )
                                                            }
                                                        ));
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self
                                                                .edit_stack
                                                                .highlights_shadows
                                                                .enabled,
                                                            "",
                                                        )
                                                        .changed()
                                                    {
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_toggle_section",
                                                            "section" => {
                                                                format!(
                                                                    "{} & {}",
                                                                    t!("develop_highlights"),
                                                                    t!("develop_shadows")
                                                                )
                                                            }
                                                        ));
                                                    }
                                                },
                                            );
                                        });

                                        ui.add_space(6.0);

                                        // Highlights slider
                                        let tip_hl_reset = t!("develop_reset_highlights");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_highlights_slider"),
                                            Slider::new(
                                                &mut self
                                                    .edit_stack
                                                    .highlights_shadows
                                                    .highlights,
                                                -100.0..=100.0,
                                            )
                                            .show_value(true),
                                            tip_hl_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_highlights",
                                                "val" => {
                                                    format!(
                                                        "{:+0.0}",
                                                        self.edit_stack
                                                            .highlights_shadows
                                                            .highlights
                                                    )
                                                }
                                            ));
                                        }
                                        if reset {
                                            self.edit_stack.highlights_shadows.highlights = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => t!("develop_highlights")
                                            ));
                                        }

                                        // Shadows slider
                                        let tip_sh_reset = t!("develop_reset_shadows");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_shadows_slider"),
                                            Slider::new(
                                                &mut self.edit_stack.highlights_shadows.shadows,
                                                -100.0..=100.0,
                                            )
                                            .show_value(true),
                                            tip_sh_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_shadows",
                                                "val" => {
                                                    format!(
                                                        "{:+0.0}",
                                                        self.edit_stack.highlights_shadows.shadows
                                                    )
                                                }
                                            ));
                                        }
                                        if reset {
                                            self.edit_stack.highlights_shadows.shadows = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => t!("develop_shadows")
                                            ));
                                        }

                                        // Whites slider
                                        let tip_wh_reset = t!("develop_reset_whites");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_whites_slider"),
                                            Slider::new(
                                                &mut self.edit_stack.highlights_shadows.whites,
                                                -100.0..=100.0,
                                            )
                                            .show_value(true),
                                            tip_wh_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_whites",
                                                "val" => {
                                                    format!(
                                                        "{:+0.0}",
                                                        self.edit_stack.highlights_shadows.whites
                                                    )
                                                }
                                            ));
                                        }
                                        if reset {
                                            self.edit_stack.highlights_shadows.whites = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => t!("develop_whites_slider")
                                            ));
                                        }

                                        // Blacks slider
                                        let tip_bl_reset = t!("develop_reset_blacks");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_blacks_slider"),
                                            Slider::new(
                                                &mut self.edit_stack.highlights_shadows.blacks,
                                                -100.0..=100.0,
                                            )
                                            .show_value(true),
                                            tip_bl_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_blacks",
                                                "val" => {
                                                    format!(
                                                        "{:+0.0}",
                                                        self.edit_stack.highlights_shadows.blacks
                                                    )
                                                }
                                            ));
                                        }
                                        if reset {
                                            self.edit_stack.highlights_shadows.blacks = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => t!("develop_blacks_slider")
                                            ));
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // 6. Contrast Section
                                    card_frame.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t!("develop_contrast"))
                                                    .strong()
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(240, 242, 248)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button("↺")
                                                        .on_hover_text(t!("develop_reset_contrast"))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.reset_contrast();
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_contrast")
                                                        ));
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self.edit_stack.contrast.enabled,
                                                            "",
                                                        )
                                                        .changed()
                                                    {
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_toggle_section",
                                                            "section" => t!("develop_contrast")
                                                        ));
                                                    }
                                                },
                                            );
                                        });

                                        ui.add_space(6.0);

                                        // Contrast factor slider
                                        let tip_contrast_reset = t!("develop_reset_contrast_val");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_contrast_slider"),
                                            Slider::new(
                                                &mut self.edit_stack.contrast.contrast,
                                                -100.0..=100.0,
                                            )
                                            .show_value(true),
                                            tip_contrast_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_contrast",
                                                "val" => {
                                                    format!(
                                                        "{:+0.0}",
                                                        self.edit_stack.contrast.contrast
                                                    )
                                                }
                                            ));
                                        }
                                        if reset {
                                            self.edit_stack.contrast.contrast = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => t!("develop_contrast")
                                            ));
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // 7. Tone Curve Section
                                    card_frame.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t!("develop_tone_curve"))
                                                    .strong()
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(240, 242, 248)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button("↺")
                                                        .on_hover_text(t!("develop_reset_curve"))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.reset_tone_curve();
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_tone_curve")
                                                        ));
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self.edit_stack.tone_curve.enabled,
                                                            "",
                                                        )
                                                        .changed()
                                                    {
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_toggle_section",
                                                            "section" => t!("develop_tone_curve")
                                                        ));
                                                    }
                                                },
                                            );
                                        });

                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            let channels = [
                                                (0usize, t!("curve_channel_master"), Color32::from_rgb(220, 220, 225)),
                                                (1usize, t!("curve_channel_red"), Color32::from_rgb(235, 95, 95)),
                                                (2usize, t!("curve_channel_green"), Color32::from_rgb(95, 215, 120)),
                                                (3usize, t!("curve_channel_blue"), Color32::from_rgb(95, 160, 240)),
                                            ];
                                            for (idx, label, col) in channels {
                                                let active = self.active_curve_channel == idx;
                                                let btn = egui::Button::new(
                                                    RichText::new(label).size(11.0).color(if active {
                                                        col
                                                    } else {
                                                        Color32::from_rgb(135, 142, 158)
                                                    }),
                                                )
                                                .fill(if active {
                                                    Color32::from_rgb(38, 42, 54)
                                                } else {
                                                    Color32::from_rgb(22, 24, 30)
                                                })
                                                .corner_radius(4.0);
                                                if ui.add(btn).clicked() {
                                                    self.active_curve_channel = idx;
                                                }
                                            }
                                        });
                                        ui.add_space(6.0);

                                        let (active_points, curve_color) = match self.active_curve_channel {
                                            1 => (
                                                &mut self.edit_stack.tone_curve.red_points,
                                                Color32::from_rgb(235, 95, 95),
                                            ),
                                            2 => (
                                                &mut self.edit_stack.tone_curve.green_points,
                                                Color32::from_rgb(95, 215, 120),
                                            ),
                                            3 => (
                                                &mut self.edit_stack.tone_curve.blue_points,
                                                Color32::from_rgb(95, 160, 240),
                                            ),
                                            _ => (
                                                &mut self.edit_stack.tone_curve.points,
                                                Color32::from_rgb(220, 220, 225),
                                            ),
                                        };

                                        if show_tone_curve_colored(ui, active_points, curve_color) {
                                            edits_changed = true;
                                            action_label = Some(t!("history_tone_curve"));
                                        }

                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(t!("develop_curve_instruction"))
                                                .weak()
                                                .size(10.5),
                                        );
                                    });

                                    ui.add_space(6.0);

                                    // 8. HSL / Color Section (8 Color Bands)
                                    card_frame.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t!("develop_hsl"))
                                                    .strong()
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(240, 242, 248)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button("↺")
                                                        .on_hover_text(t!("develop_reset_hsl"))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.reset_hsl();
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_hsl")
                                                        ));
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self.edit_stack.hsl.enabled,
                                                            "",
                                                        )
                                                        .changed()
                                                    {
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_toggle_section",
                                                            "section" => t!("develop_hsl")
                                                        ));
                                                    }
                                                },
                                            );
                                        });

                                        ui.add_space(6.0);

                                        const BAND_KEYS: [&str; 8] = [
                                            "develop_hsl_red",
                                            "develop_hsl_orange",
                                            "develop_hsl_yellow",
                                            "develop_hsl_green",
                                            "develop_hsl_aqua",
                                            "develop_hsl_blue",
                                            "develop_hsl_purple",
                                            "develop_hsl_magenta",
                                        ];
                                        const BAND_COLORS: [Color32; 8] = [
                                            Color32::from_rgb(235, 60, 60),
                                            Color32::from_rgb(245, 140, 40),
                                            Color32::from_rgb(240, 215, 50),
                                            Color32::from_rgb(60, 200, 80),
                                            Color32::from_rgb(50, 210, 215),
                                            Color32::from_rgb(65, 130, 245),
                                            Color32::from_rgb(160, 80, 240),
                                            Color32::from_rgb(230, 80, 180),
                                        ];

                                        // 8 Color Band selector with refined rounded chips
                                        ui.horizontal_wrapped(|ui| {
                                            for (i, (&key, &col)) in
                                                BAND_KEYS.iter().zip(BAND_COLORS.iter()).enumerate()
                                            {
                                                let is_selected = self.hsl_selected_band == i;
                                                let text = RichText::new(format!("● {}", t!(key)))
                                                    .color(col)
                                                    .size(11.0);
                                                let (bg, stroke) = if is_selected {
                                                    (
                                                        Color32::from_rgb(46, 56, 78),
                                                        Stroke::new(
                                                            1.0,
                                                            Color32::from_rgb(70, 92, 138),
                                                        ),
                                                    )
                                                } else {
                                                    (
                                                        Color32::from_rgb(22, 23, 28),
                                                        Stroke::new(
                                                            1.0,
                                                            Color32::from_rgb(36, 39, 48),
                                                        ),
                                                    )
                                                };
                                                if ui
                                                    .add(
                                                        egui::Button::new(text)
                                                            .fill(bg)
                                                            .stroke(stroke)
                                                            .corner_radius(10.0),
                                                    )
                                                    .clicked()
                                                {
                                                    self.hsl_selected_band = i;
                                                }
                                            }
                                        });

                                        ui.add_space(6.0);

                                        let sel = self.hsl_selected_band;
                                        let localized_band_name = t!(BAND_KEYS[sel]);
                                        let band = &mut self.edit_stack.hsl.bands[sel];

                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t!(
                                                    "develop_band_label",
                                                    "band" => &localized_band_name
                                                ))
                                                .strong(),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button("↺")
                                                        .on_hover_text(t!(
                                                            "develop_reset_band",
                                                            "band" => &localized_band_name
                                                        ))
                                                        .clicked()
                                                    {
                                                        *band =
                                                            beam_core::sidecar::HslBand::default();
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_reset_section",
                                                            "section" => &localized_band_name
                                                        ));
                                                    }
                                                },
                                            );
                                        });

                                        // Hue
                                        let tip_hue_reset = t!("develop_reset_hue");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_hue_slider"),
                                            Slider::new(&mut band.hue, -100.0..=100.0)
                                                .show_value(true),
                                            tip_hue_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_hsl_band",
                                                "band" => &localized_band_name
                                            ));
                                        }
                                        if reset {
                                            band.hue = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => format!("{}: {}", &localized_band_name, t!("develop_hsl_hue"))
                                            ));
                                        }

                                        // Saturation
                                        let tip_sat_reset = t!("develop_reset_sat");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_sat_slider"),
                                            Slider::new(&mut band.saturation, -100.0..=100.0)
                                                .show_value(true),
                                            tip_sat_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_hsl_band",
                                                "band" => &localized_band_name
                                            ));
                                        }
                                        if reset {
                                            band.saturation = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => format!("{}: {}", &localized_band_name, t!("develop_hsl_saturation"))
                                            ));
                                        }

                                        // Luminance
                                        let tip_lum_reset = t!("develop_reset_lum");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_lum_slider"),
                                            Slider::new(&mut band.luminance, -100.0..=100.0)
                                                .show_value(true),
                                            tip_lum_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_hsl_band",
                                                "band" => &localized_band_name
                                            ));
                                        }
                                        if reset {
                                            band.luminance = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => format!("{}: {}", &localized_band_name, t!("develop_hsl_luminance"))
                                            ));
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // 7b. Color & Presence (Vibrance & Saturation)
                                    card_frame.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t!("develop_vibrance_saturation"))
                                                    .strong()
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(240, 242, 248)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button("↺")
                                                        .on_hover_text(t!("develop_reset_vibrance_sat"))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.reset_vibrance_saturation();
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_vibrance_saturation")
                                                        ));
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self
                                                                .edit_stack
                                                                .vibrance_saturation
                                                                .enabled,
                                                            "",
                                                        )
                                                        .changed()
                                                    {
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_toggle_section",
                                                            "section" => t!("develop_vibrance_saturation")
                                                        ));
                                                    }
                                                },
                                            );
                                        });

                                        ui.add_space(6.0);

                                        let tip_vib_reset = t!("develop_reset_vibrance");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_vibrance_slider"),
                                            Slider::new(
                                                &mut self.edit_stack.vibrance_saturation.vibrance,
                                                -100.0..=100.0,
                                            )
                                            .show_value(true),
                                            tip_vib_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_vibrance",
                                                "val" => {
                                                    format!(
                                                        "{:+0.0}",
                                                        self.edit_stack.vibrance_saturation.vibrance
                                                    )
                                                }
                                            ));
                                        }
                                        if reset {
                                            self.edit_stack.vibrance_saturation.vibrance = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => t!("develop_vibrance_slider")
                                            ));
                                        }

                                        let tip_sat_reset = t!("develop_reset_saturation");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_saturation_slider"),
                                            Slider::new(
                                                &mut self.edit_stack.vibrance_saturation.saturation,
                                                -100.0..=100.0,
                                            )
                                            .show_value(true),
                                            tip_sat_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_saturation",
                                                "val" => {
                                                    format!(
                                                        "{:+0.0}",
                                                        self.edit_stack.vibrance_saturation.saturation
                                                    )
                                                }
                                            ));
                                        }
                                        if reset {
                                            self.edit_stack.vibrance_saturation.saturation = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => t!("develop_saturation_slider")
                                            ));
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // 7c. Effects (Vignette & Grain)
                                    card_frame.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t!("develop_vignette_grain"))
                                                    .strong()
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(240, 242, 248)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button("↺")
                                                        .on_hover_text(t!("develop_reset_vignette_grain"))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.reset_vignette_grain();
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_vignette_grain")
                                                        ));
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self
                                                                .edit_stack
                                                                .vignette_grain
                                                                .enabled,
                                                            "",
                                                        )
                                                        .changed()
                                                    {
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_toggle_section",
                                                            "section" => t!("develop_vignette_grain")
                                                        ));
                                                    }
                                                },
                                            );
                                        });

                                        ui.add_space(6.0);

                                        let tip_vg_reset = t!("develop_reset_vignette_grain");
                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_vignette_amount_slider"),
                                            Slider::new(
                                                &mut self.edit_stack.vignette_grain.vignette_amount,
                                                -100.0..=100.0,
                                            )
                                            .show_value(true),
                                            tip_vg_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_vignette",
                                                "val" => {
                                                    format!(
                                                        "{:+0.0}",
                                                        self.edit_stack.vignette_grain.vignette_amount
                                                    )
                                                }
                                            ));
                                        }
                                        if reset {
                                            self.edit_stack.vignette_grain.vignette_amount = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => t!("develop_vignette_amount_slider")
                                            ));
                                        }

                                        if self.edit_stack.vignette_grain.vignette_amount.abs() > 0.1 {
                                            let (resp, reset) = draw_slider_row(
                                                ui,
                                                &t!("develop_vignette_midpoint_slider"),
                                                Slider::new(
                                                    &mut self.edit_stack.vignette_grain.vignette_midpoint,
                                                    0.0..=100.0,
                                                )
                                                .show_value(true),
                                                tip_vg_reset.as_str(),
                                            );
                                            if resp.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!(
                                                    "history_vignette",
                                                    "val" => format!("{:+0.0}", self.edit_stack.vignette_grain.vignette_amount)
                                                ));
                                            }
                                            if reset {
                                                self.edit_stack.vignette_grain.vignette_midpoint = 50.0;
                                                edits_changed = true;
                                                immediate_action = true;
                                            }

                                            let (resp, reset) = draw_slider_row(
                                                ui,
                                                &t!("develop_vignette_feather_slider"),
                                                Slider::new(
                                                    &mut self.edit_stack.vignette_grain.vignette_feather,
                                                    0.0..=100.0,
                                                )
                                                .show_value(true),
                                                tip_vg_reset.as_str(),
                                            );
                                            if resp.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!(
                                                    "history_vignette",
                                                    "val" => format!("{:+0.0}", self.edit_stack.vignette_grain.vignette_amount)
                                                ));
                                            }
                                            if reset {
                                                self.edit_stack.vignette_grain.vignette_feather = 50.0;
                                                edits_changed = true;
                                                immediate_action = true;
                                            }
                                        }

                                        let (resp, reset) = draw_slider_row(
                                            ui,
                                            &t!("develop_grain_amount_slider"),
                                            Slider::new(
                                                &mut self.edit_stack.vignette_grain.grain_amount,
                                                0.0..=100.0,
                                            )
                                            .show_value(true),
                                            tip_vg_reset.as_str(),
                                        );
                                        if resp.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!(
                                                "history_grain",
                                                "val" => {
                                                    format!(
                                                        "{:.0}",
                                                        self.edit_stack.vignette_grain.grain_amount
                                                    )
                                                }
                                            ));
                                        }
                                        if reset {
                                            self.edit_stack.vignette_grain.grain_amount = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!(
                                                "history_reset_section",
                                                "section" => t!("develop_grain_amount_slider")
                                            ));
                                        }

                                        if self.edit_stack.vignette_grain.grain_amount > 0.1 {
                                            let (resp, reset) = draw_slider_row(
                                                ui,
                                                &t!("develop_grain_size_slider"),
                                                Slider::new(
                                                    &mut self.edit_stack.vignette_grain.grain_size,
                                                    1.0..=5.0,
                                                )
                                                .show_value(true),
                                                tip_vg_reset.as_str(),
                                            );
                                            if resp.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!(
                                                    "history_grain",
                                                    "val" => format!("{:.0}", self.edit_stack.vignette_grain.grain_amount)
                                                ));
                                            }
                                            if reset {
                                                self.edit_stack.vignette_grain.grain_size = 1.5;
                                                edits_changed = true;
                                                immediate_action = true;
                                            }
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // 10. Color Grading Section (3-Way Wheels)
                                    card_frame.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t!("develop_color_grading"))
                                                    .strong()
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(240, 242, 248)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .small_button("↺")
                                                        .on_hover_text(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_color_grading")
                                                        ))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.reset_color_grading();
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_color_grading")
                                                        ));
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self.edit_stack.color_grading.enabled,
                                                            "",
                                                        )
                                                        .changed()
                                                    {
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_toggle_section",
                                                            "section" => t!("develop_color_grading")
                                                        ));
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(6.0);

                                        let lum_lbl = t!("grading_luminance");
                                        ui.columns(3, |cols| {
                                            let (c, s) = show_color_wheel(
                                                &mut cols[0],
                                                &t!("grading_shadows"),
                                                &mut self.edit_stack.color_grading.shadows,
                                                &lum_lbl,
                                            );
                                            if c {
                                                edits_changed = true;
                                                action_label = Some(t!("history_color_grading"));
                                            }
                                            if s {
                                                immediate_action = true;
                                            }

                                            let (c, s) = show_color_wheel(
                                                &mut cols[1],
                                                &t!("grading_midtones"),
                                                &mut self.edit_stack.color_grading.midtones,
                                                &lum_lbl,
                                            );
                                            if c {
                                                edits_changed = true;
                                                action_label = Some(t!("history_color_grading"));
                                            }
                                            if s {
                                                immediate_action = true;
                                            }

                                            let (c, s) = show_color_wheel(
                                                &mut cols[2],
                                                &t!("grading_highlights"),
                                                &mut self.edit_stack.color_grading.highlights,
                                                &lum_lbl,
                                            );
                                            if c {
                                                edits_changed = true;
                                                action_label = Some(t!("history_color_grading"));
                                            }
                                            if s {
                                                immediate_action = true;
                                            }
                                        });

                                        ui.add_space(4.0);
                                        let (blend_res, blend_reset) = draw_slider_row(
                                            ui,
                                            &t!("grading_blending"),
                                            Slider::new(
                                                &mut self.edit_stack.color_grading.blending,
                                                0.0..=1.0,
                                            )
                                            .fixed_decimals(2),
                                            &t!("grading_blending"),
                                        );
                                        if blend_reset {
                                            self.edit_stack.color_grading.blending = 0.5;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!("history_color_grading"));
                                        }
                                        if blend_res.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!("history_color_grading"));
                                        }
                                        if blend_res.drag_stopped() {
                                            immediate_action = true;
                                        }

                                        let (bal_res, bal_reset) = draw_slider_row(
                                            ui,
                                            &t!("grading_balance"),
                                            Slider::new(
                                                &mut self.edit_stack.color_grading.balance,
                                                -1.0..=1.0,
                                            )
                                            .fixed_decimals(2),
                                            &t!("grading_balance"),
                                        );
                                        if bal_reset {
                                            self.edit_stack.color_grading.balance = 0.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!("history_color_grading"));
                                        }
                                        if bal_res.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!("history_color_grading"));
                                        }
                                        if bal_res.drag_stopped() {
                                            immediate_action = true;
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // 11. Detail Section (Clarity, Dehaze, Sharpening, NR)
                                    card_frame.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t!("develop_detail"))
                                                    .strong()
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(240, 242, 248)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .small_button("↺")
                                                        .on_hover_text(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_detail")
                                                        ))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.reset_detail();
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_detail")
                                                        ));
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self.edit_stack.detail.enabled,
                                                            "",
                                                        )
                                                        .changed()
                                                    {
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_toggle_section",
                                                            "section" => t!("develop_detail")
                                                        ));
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(6.0);

                                        let detail_sliders: [(&str, &mut f32, std::ops::RangeInclusive<f32>); 6] = [
                                            ("detail_clarity", &mut self.edit_stack.detail.clarity, -1.0..=1.0),
                                            ("detail_dehaze", &mut self.edit_stack.detail.dehaze, -1.0..=1.0),
                                            ("detail_sharpening", &mut self.edit_stack.detail.sharpening, 0.0..=1.0),
                                            ("detail_nr_luma", &mut self.edit_stack.detail.noise_reduction_luma, 0.0..=1.0),
                                            ("detail_nr_color", &mut self.edit_stack.detail.noise_reduction_color, 0.0..=1.0),
                                            ("develop_ai_denoise", &mut self.edit_stack.detail.ai_denoise, 0.0..=1.0),
                                        ];
                                        for (key, val, range) in detail_sliders {
                                            let lbl = t!(key);
                                            let (res, reset) = draw_slider_row(
                                                ui,
                                                &lbl,
                                                Slider::new(val, range).fixed_decimals(2),
                                                &lbl,
                                            );
                                            if reset {
                                                *val = 0.0;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!("history_detail"));
                                            }
                                            if res.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!("history_detail"));
                                            }
                                            if res.drag_stopped() {
                                                immediate_action = true;
                                            }
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // 11b. Lens Correction Section
                                    card_frame.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t!("develop_lens"))
                                                    .strong()
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(240, 242, 248)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .small_button("↺")
                                                        .on_hover_text(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_lens")
                                                        ))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.lens =
                                                            beam_core::LensAdjustment::default();
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_lens")
                                                        ));
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self.edit_stack.lens.enabled,
                                                            "",
                                                        )
                                                        .changed()
                                                    {
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_toggle_section",
                                                            "section" => t!("develop_lens")
                                                        ));
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(6.0);

                                        let lens_sliders: [(&str, &mut f32); 6] = [
                                            (
                                                "develop_lens_distortion",
                                                &mut self.edit_stack.lens.distortion,
                                            ),
                                            (
                                                "develop_lens_ca_red",
                                                &mut self.edit_stack.lens.ca_red_cyan,
                                            ),
                                            (
                                                "develop_lens_ca_blue",
                                                &mut self.edit_stack.lens.ca_blue_yellow,
                                            ),
                                            (
                                                "develop_lens_vignette",
                                                &mut self.edit_stack.lens.vignette,
                                            ),
                                            (
                                                "develop_lens_perspective_v",
                                                &mut self.edit_stack.lens.perspective_v,
                                            ),
                                            (
                                                "develop_lens_perspective_h",
                                                &mut self.edit_stack.lens.perspective_h,
                                            ),
                                        ];
                                        for (key, val) in lens_sliders {
                                            let lbl = t!(key);
                                            let (res, reset) = draw_slider_row(
                                                ui,
                                                &lbl,
                                                Slider::new(val, -100.0..=100.0).fixed_decimals(1),
                                                &lbl,
                                            );
                                            if reset {
                                                *val = 0.0;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!("history_lens"));
                                            }
                                            if res.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!("history_lens"));
                                            }
                                            if res.drag_stopped() {
                                                immediate_action = true;
                                            }
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // 12. 3D LUT Section (.cube)
                                    card_frame.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t!("develop_lut"))
                                                    .strong()
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(240, 242, 248)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .small_button("↺")
                                                        .on_hover_text(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_lut")
                                                        ))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.reset_lut();
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_lut")
                                                        ));
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self.edit_stack.lut.enabled,
                                                            "",
                                                        )
                                                        .changed()
                                                    {
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_toggle_section",
                                                            "section" => t!("develop_lut")
                                                        ));
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(6.0);

                                        ui.horizontal(|ui| {
                                            if ui.button(t!("lut_load_cube")).clicked() {
                                                if let Some(path) = rfd::FileDialog::new()
                                                    .add_filter("3D LUT", &["cube", "CUBE"])
                                                    .pick_file()
                                                {
                                                    self.edit_stack.lut.path =
                                                        Some(path.to_string_lossy().to_string());
                                                    self.edit_stack.lut.enabled = true;
                                                    edits_changed = true;
                                                    immediate_action = true;
                                                    action_label = Some(t!("history_lut"));
                                                }
                                            }
                                            if self.edit_stack.lut.path.is_some()
                                                && ui.button(t!("lut_clear")).clicked()
                                            {
                                                self.edit_stack.lut.path = None;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!("history_lut"));
                                            }
                                        });

                                        let lut_name = self
                                            .edit_stack
                                            .lut
                                            .path
                                            .as_deref()
                                            .and_then(|p| std::path::Path::new(p).file_name())
                                            .and_then(|n| n.to_str())
                                            .map(|s| s.to_string())
                                            .unwrap_or_else(|| t!("lut_none"));
                                        ui.label(
                                            RichText::new(lut_name).weak().size(11.0),
                                        );

                                        let int_lbl = t!("lut_intensity");
                                        let (int_res, int_reset) = draw_slider_row(
                                            ui,
                                            &int_lbl,
                                            Slider::new(
                                                &mut self.edit_stack.lut.intensity,
                                                0.0..=1.0,
                                            )
                                            .fixed_decimals(2),
                                            &int_lbl,
                                        );
                                        if int_reset {
                                            self.edit_stack.lut.intensity = 1.0;
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!("history_lut"));
                                        }
                                        if int_res.changed() {
                                            edits_changed = true;
                                            action_label = Some(t!("history_lut"));
                                        }
                                        if int_res.drag_stopped() {
                                            immediate_action = true;
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // 13. Parametric Masks Section (Linear & Radial Gradients)
                                    card_frame.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(t!("develop_masks"))
                                                    .strong()
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(240, 242, 248)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .small_button("↺")
                                                        .on_hover_text(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_masks")
                                                        ))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.reset_masks();
                                                        self.selected_mask_id = None;
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!(
                                                            "history_reset_section",
                                                            "section" => t!("develop_masks")
                                                        ));
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(6.0);

                                        ui.horizontal_wrapped(|ui| {
                                            let can_add = self.edit_stack.masks.len() < 8;
                                            if ui
                                                .add_enabled(
                                                    can_add,
                                                    egui::Button::new(t!("masks_add_linear")),
                                                )
                                                .clicked()
                                            {
                                                let id = self.next_mask_id;
                                                self.next_mask_id += 1;
                                                let name = t!(
                                                    "masks_linear_name",
                                                    "num" => (self.edit_stack.masks.len() + 1).to_string()
                                                );
                                                self.edit_stack.masks.push(
                                                    beam_core::sidecar::MaskLayer::new_linear(
                                                        id, name,
                                                    ),
                                                );
                                                self.selected_mask_id = Some(id);
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!("history_masks"));
                                            }
                                            if ui
                                                .add_enabled(
                                                    can_add,
                                                    egui::Button::new(t!("masks_add_radial")),
                                                )
                                                .clicked()
                                            {
                                                let id = self.next_mask_id;
                                                self.next_mask_id += 1;
                                                let name = t!(
                                                    "masks_radial_name",
                                                    "num" => (self.edit_stack.masks.len() + 1).to_string()
                                                );
                                                self.edit_stack.masks.push(
                                                    beam_core::sidecar::MaskLayer::new_radial(
                                                        id, name,
                                                    ),
                                                );
                                                self.selected_mask_id = Some(id);
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!("history_masks"));
                                            }
                                            if ui
                                                .add_enabled(
                                                    can_add,
                                                    egui::Button::new(t!("masks_add_brush")),
                                                )
                                                .clicked()
                                            {
                                                let id = self.next_mask_id;
                                                self.next_mask_id += 1;
                                                let name = t!(
                                                    "masks_brush_name",
                                                    "num" => (self.edit_stack.masks.len() + 1).to_string()
                                                );
                                                self.edit_stack.masks.push(
                                                    beam_core::sidecar::MaskLayer::new_brush(
                                                        id, name,
                                                    ),
                                                );
                                                self.selected_mask_id = Some(id);
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!("history_masks"));
                                            }
                                            if ui
                                                .add_enabled(
                                                    can_add && self.current_image.is_some(),
                                                    egui::Button::new(t!("masks_auto_sky")),
                                                )
                                                .clicked()
                                            {
                                                if let Some(img) = &self.current_image {
                                                    let id = self.next_mask_id;
                                                    self.next_mask_id += 1;
                                                    let name = t!(
                                                        "masks_sky_name",
                                                        "num" => (self.edit_stack.masks.len() + 1).to_string()
                                                    );
                                                    let layer = beam_core::detect_sky_mask(
                                                        &img.proxy_data,
                                                        img.proxy_width,
                                                        img.proxy_height,
                                                        id,
                                                        name,
                                                    );
                                                    self.edit_stack.masks.push(layer);
                                                    self.selected_mask_id = Some(id);
                                                    edits_changed = true;
                                                    immediate_action = true;
                                                    action_label = Some(t!("history_masks"));
                                                }
                                            }
                                            if ui
                                                .add_enabled(
                                                    can_add && self.current_image.is_some(),
                                                    egui::Button::new(t!("masks_auto_subject")),
                                                )
                                                .clicked()
                                            {
                                                if let Some(img) = &self.current_image {
                                                    let id = self.next_mask_id;
                                                    self.next_mask_id += 1;
                                                    let name = t!(
                                                        "masks_subject_name",
                                                        "num" => (self.edit_stack.masks.len() + 1).to_string()
                                                    );
                                                    let layer = beam_core::detect_subject_mask(
                                                        &img.proxy_data,
                                                        img.proxy_width,
                                                        img.proxy_height,
                                                        id,
                                                        name,
                                                    );
                                                    self.edit_stack.masks.push(layer);
                                                    self.selected_mask_id = Some(id);
                                                    edits_changed = true;
                                                    immediate_action = true;
                                                    action_label = Some(t!("history_masks"));
                                                }
                                            }
                                        });

                                        if self.edit_stack.masks.is_empty() {
                                            ui.add_space(4.0);
                                            ui.label(
                                                RichText::new(t!("masks_none"))
                                                    .weak()
                                                    .size(11.0),
                                            );
                                        } else {
                                            ui.checkbox(
                                                &mut self.show_mask_overlay,
                                                t!("masks_show_overlay"),
                                            );
                                            ui.add_space(4.0);

                                            let mut delete_idx = None;
                                            for (idx, mask) in
                                                self.edit_stack.masks.iter_mut().enumerate()
                                            {
                                                let is_sel = self.selected_mask_id == Some(mask.id);
                                                ui.horizontal(|ui| {
                                                    if ui.checkbox(&mut mask.enabled, "").changed() {
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!("history_masks"));
                                                    }
                                                    let resp = ui.selectable_label(is_sel, &mask.name);
                                                    if resp.clicked() {
                                                        self.selected_mask_id = Some(mask.id);
                                                    }
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            if ui
                                                                .small_button("X")
                                                                .on_hover_text(t!("masks_delete"))
                                                                .clicked()
                                                            {
                                                                delete_idx = Some(idx);
                                                            }
                                                        },
                                                    );
                                                });
                                            }
                                            if let Some(idx) = delete_idx {
                                                let removed = self.edit_stack.masks.remove(idx);
                                                if self.selected_mask_id == Some(removed.id) {
                                                    self.selected_mask_id = self
                                                        .edit_stack
                                                        .masks
                                                        .first()
                                                        .map(|m| m.id);
                                                }
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!("history_masks"));
                                            }

                                            if let Some(sel_id) = self.selected_mask_id {
                                                if let Some(mask) = self
                                                    .edit_stack
                                                    .masks
                                                    .iter_mut()
                                                    .find(|m| m.id == sel_id)
                                                {
                                                    ui.separator();
                                                    if let beam_core::sidecar::MaskShape::RadialGradient {
                                                        feather,
                                                        invert,
                                                        ..
                                                    } = &mut mask.shape
                                                    {
                                                        if ui
                                                            .checkbox(invert, t!("masks_invert"))
                                                            .changed()
                                                        {
                                                            edits_changed = true;
                                                            immediate_action = true;
                                                            action_label =
                                                                Some(t!("history_masks"));
                                                        }
                                                        let f_lbl = t!("masks_feather");
                                                        let (f_res, f_reset) = draw_slider_row(
                                                            ui,
                                                            &f_lbl,
                                                            Slider::new(feather, 0.0..=1.0)
                                                                .fixed_decimals(2),
                                                            &f_lbl,
                                                        );
                                                        if f_reset {
                                                            *feather = 0.5;
                                                            edits_changed = true;
                                                            immediate_action = true;
                                                            action_label =
                                                                Some(t!("history_masks"));
                                                        }
                                                        if f_res.changed() {
                                                            edits_changed = true;
                                                            action_label =
                                                                Some(t!("history_masks"));
                                                        }
                                                        if f_res.drag_stopped() {
                                                            immediate_action = true;
                                                        }
                                                    } else if let beam_core::sidecar::MaskShape::Brush {
                                                        strokes,
                                                        radius,
                                                        feather,
                                                        invert,
                                                    } = &mut mask.shape
                                                    {
                                                        ui.horizontal(|ui| {
                                                            if ui
                                                                .checkbox(invert, t!("masks_invert"))
                                                                .changed()
                                                            {
                                                                edits_changed = true;
                                                                immediate_action = true;
                                                                action_label =
                                                                    Some(t!("history_masks"));
                                                            }
                                                            if !strokes.is_empty()
                                                                && ui
                                                                    .small_button(t!("masks_brush_clear"))
                                                                    .clicked()
                                                            {
                                                                strokes.clear();
                                                                edits_changed = true;
                                                                immediate_action = true;
                                                                action_label =
                                                                    Some(t!("history_masks"));
                                                            }
                                                        });
                                                        let r_lbl = t!("masks_brush_radius");
                                                        let (r_res, r_reset) = draw_slider_row(
                                                            ui,
                                                            &r_lbl,
                                                            Slider::new(radius, 0.01..=0.25)
                                                                .fixed_decimals(3),
                                                            &r_lbl,
                                                        );
                                                        if r_reset {
                                                            *radius = 0.06;
                                                            edits_changed = true;
                                                            immediate_action = true;
                                                            action_label =
                                                                Some(t!("history_masks"));
                                                        }
                                                        if r_res.changed() {
                                                            edits_changed = true;
                                                            action_label =
                                                                Some(t!("history_masks"));
                                                        }
                                                        if r_res.drag_stopped() {
                                                            immediate_action = true;
                                                        }
                                                        let f_lbl = t!("masks_feather");
                                                        let (f_res, f_reset) = draw_slider_row(
                                                            ui,
                                                            &f_lbl,
                                                            Slider::new(feather, 0.0..=1.0)
                                                                .fixed_decimals(2),
                                                            &f_lbl,
                                                        );
                                                        if f_reset {
                                                            *feather = 0.5;
                                                            edits_changed = true;
                                                            immediate_action = true;
                                                            action_label =
                                                                Some(t!("history_masks"));
                                                        }
                                                        if f_res.changed() {
                                                            edits_changed = true;
                                                            action_label =
                                                                Some(t!("history_masks"));
                                                        }
                                                        if f_res.drag_stopped() {
                                                            immediate_action = true;
                                                        }
                                                    }

                                                    if ui
                                                        .checkbox(
                                                            &mut mask.color_range_enabled,
                                                            t!("masks_color_range"),
                                                        )
                                                        .changed()
                                                    {
                                                        edits_changed = true;
                                                        immediate_action = true;
                                                        action_label = Some(t!("history_masks"));
                                                    }
                                                    if mask.color_range_enabled {
                                                        let h_lbl = t!("masks_color_hue");
                                                        let (h_res, h_reset) = draw_slider_row(
                                                            ui,
                                                            &h_lbl,
                                                            Slider::new(
                                                                &mut mask.color_range_hue,
                                                                0.0..=360.0,
                                                            )
                                                            .fixed_decimals(0),
                                                            &h_lbl,
                                                        );
                                                        if h_reset {
                                                            mask.color_range_hue = 0.0;
                                                            edits_changed = true;
                                                            immediate_action = true;
                                                            action_label =
                                                                Some(t!("history_masks"));
                                                        }
                                                        if h_res.changed() {
                                                            edits_changed = true;
                                                            action_label =
                                                                Some(t!("history_masks"));
                                                        }
                                                        if h_res.drag_stopped() {
                                                            immediate_action = true;
                                                        }

                                                        let t_lbl = t!("masks_color_tolerance");
                                                        let (t_res, t_reset) = draw_slider_row(
                                                            ui,
                                                            &t_lbl,
                                                            Slider::new(
                                                                &mut mask.color_range_tolerance,
                                                                5.0..=180.0,
                                                            )
                                                            .fixed_decimals(0),
                                                            &t_lbl,
                                                        );
                                                        if t_reset {
                                                            mask.color_range_tolerance = 35.0;
                                                            edits_changed = true;
                                                            immediate_action = true;
                                                            action_label =
                                                                Some(t!("history_masks"));
                                                        }
                                                        if t_res.changed() {
                                                            edits_changed = true;
                                                            action_label =
                                                                Some(t!("history_masks"));
                                                        }
                                                        if t_res.drag_stopped() {
                                                            immediate_action = true;
                                                        }
                                                    }

                                                    let luma_sliders: [(&str, &mut f32, std::ops::RangeInclusive<f32>, f32); 3] = [
                                                        ("masks_luma_min", &mut mask.luma_min, 0.0..=1.0, 0.0),
                                                        ("masks_luma_max", &mut mask.luma_max, 0.0..=1.0, 1.0),
                                                        ("masks_luma_smoothness", &mut mask.luma_smoothness, 0.01..=1.0, 0.15),
                                                    ];
                                                    for (key, val, range, default_val) in luma_sliders {
                                                        let lbl = t!(key);
                                                        let (res, reset) = draw_slider_row(
                                                            ui,
                                                            &lbl,
                                                            Slider::new(val, range).fixed_decimals(2),
                                                            &lbl,
                                                        );
                                                        if reset {
                                                            *val = default_val;
                                                            edits_changed = true;
                                                            immediate_action = true;
                                                            action_label = Some(t!("history_masks"));
                                                        }
                                                        if res.changed() {
                                                            edits_changed = true;
                                                            action_label = Some(t!("history_masks"));
                                                        }
                                                        if res.drag_stopped() {
                                                            immediate_action = true;
                                                        }
                                                    }
                                                    if mask.luma_max < mask.luma_min {
                                                        mask.luma_max = mask.luma_min;
                                                    }

                                                    let mask_sliders: [(&str, &mut f32, std::ops::RangeInclusive<f32>, usize); 7] = [
                                                        ("develop_exposure", &mut mask.exposure, -5.0..=5.0, 2),
                                                        ("develop_contrast", &mut mask.contrast, -100.0..=100.0, 0),
                                                        ("develop_highlights", &mut mask.highlights, -100.0..=100.0, 0),
                                                        ("develop_shadows", &mut mask.shadows, -100.0..=100.0, 0),
                                                        ("develop_wb_temp", &mut mask.temperature, -100.0..=100.0, 0),
                                                        ("develop_wb_tint", &mut mask.tint, -100.0..=100.0, 0),
                                                        ("develop_saturation_slider", &mut mask.saturation, -100.0..=100.0, 0),
                                                    ];
                                                    for (key, val, range, dec) in mask_sliders {
                                                        let lbl = t!(key);
                                                        let (res, reset) = draw_slider_row(
                                                            ui,
                                                            &lbl,
                                                            Slider::new(val, range)
                                                                .fixed_decimals(dec),
                                                            &lbl,
                                                        );
                                                        if reset {
                                                            *val = 0.0;
                                                            edits_changed = true;
                                                            immediate_action = true;
                                                            action_label =
                                                                Some(t!("history_masks"));
                                                        }
                                                        if res.changed() {
                                                            edits_changed = true;
                                                            action_label =
                                                                Some(t!("history_masks"));
                                                        }
                                                        if res.drag_stopped() {
                                                            immediate_action = true;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    });

                                    ui.add_space(14.0);

                                    // 8. Action Buttons
                                    ui.horizontal_wrapped(|ui| {
                                        let reset_btn = egui::Button::new(
                                            RichText::new(format!("↺ {}", t!("develop_reset_all")))
                                                .size(11.5)
                                                .color(Color32::from_rgb(235, 115, 125)),
                                        )
                                        .fill(Color32::from_rgb(32, 24, 26))
                                        .stroke(Stroke::new(1.0, Color32::from_rgb(64, 38, 42)))
                                        .corner_radius(6.0)
                                        .min_size(egui::vec2(0.0, 26.0));

                                        if ui
                                            .add(reset_btn)
                                            .on_hover_text(t!("develop_reset_all_tooltip"))
                                            .clicked()
                                        {
                                            self.edit_stack.reset_all();
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!("history_reset_all"));
                                        }

                                        let sidecar_btn = egui::Button::new(
                                            RichText::new(t!("develop_save_sidecar"))
                                                .size(11.5)
                                                .color(Color32::from_rgb(210, 215, 225)),
                                        )
                                        .fill(Color32::from_rgb(26, 29, 36))
                                        .stroke(Stroke::new(1.0, Color32::from_rgb(44, 48, 58)))
                                        .corner_radius(6.0)
                                        .min_size(egui::vec2(0.0, 26.0));

                                        if ui.add(sidecar_btn).clicked() {
                                            self.save_sidecar();
                                        }

                                        let export_btn = egui::Button::new(
                                            RichText::new(t!("develop_export_btn"))
                                                .size(12.0)
                                                .color(Color32::from_rgb(245, 248, 255))
                                                .strong(),
                                        )
                                        .fill(Color32::from_rgb(46, 92, 195))
                                        .stroke(Stroke::new(1.0, Color32::from_rgb(70, 120, 235)))
                                        .corner_radius(6.0)
                                        .min_size(egui::vec2(0.0, 26.0));

                                        if ui.add(export_btn).clicked() {
                                            self.open_export_dialog();
                                        }
                                    });
                                });
                        });
                }

                let pointer_released = ctx.input(|i| i.pointer.any_released());
                let pointer_down = ctx.input(|i| i.pointer.primary_down());

                // Apply edits if sliders or reset buttons were clicked
                if edits_changed {
                    if immediate_action {
                        let label = action_label.unwrap_or_else(|| t!("history_initial"));
                        self.push_history_step(label);
                        self.slider_drag_start_stack = None;
                        self.slider_drag_action_name = None;
                    } else {
                        // Continuous edit (slider drag or tone curve point drag)
                        if self.slider_drag_start_stack.is_none() {
                            let baseline = if self.history_index < self.history.len() {
                                self.history[self.history_index].stack.clone()
                            } else {
                                self.edit_stack.clone()
                            };
                            self.slider_drag_start_stack = Some(baseline);
                        }
                        if let Some(lbl) = action_label {
                            self.slider_drag_action_name = Some(lbl);
                        }
                        // If modified without pointer being held down (e.g. keyboard navigation)
                        if !pointer_down && !pointer_released {
                            if let Some(lbl) = self.slider_drag_action_name.take() {
                                self.push_history_step(lbl);
                            }
                            self.slider_drag_start_stack = None;
                        }
                    }

                    self.re_render_edits(frame);
                    ctx.request_repaint();
                }

                if pointer_released {
                    if let Some(start_stack) = self.slider_drag_start_stack.take() {
                        if start_stack != self.edit_stack {
                            let label = self
                                .slider_drag_action_name
                                .take()
                                .unwrap_or_else(|| t!("history_initial"));
                            self.push_history_step(label);
                        }
                    }
                    self.slider_drag_action_name = None;
                }

                // Filmstrip at bottom of Develop view
                if self.filmstrip_visible && self.current_image.is_some() {
                    Panel::bottom("develop_filmstrip")
                        .resizable(true)
                        .default_size(self.filmstrip_height)
                        .min_size(70.0)
                        .max_size(200.0)
                        .show(ui, |ui| {
                            self.show_develop_filmstrip(ui);
                        });
                }

                // Central viewport panel
                egui::CentralPanel::default().show(ui, |ui| {
                    let mut clear_error = false;
                    if let Some(err) = &self.error_message {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.colored_label(Color32::RED, t!("develop_error_title"));
                                ui.label(RichText::new(err).color(Color32::LIGHT_RED));
                                if ui.button(t!("develop_dismiss")).clicked() {
                                    clear_error = true;
                                }
                            });
                        });
                        ui.add_space(8.0);
                    }
                    if clear_error {
                        self.error_message = None;
                    }

                    let available_size = ui.available_size();

                    if let Some(texture_id) = self.egui_texture_id {
                        // Unified Before / After and Zoom control bar
                        egui::Frame::new()
                            .fill(Color32::from_rgb(18, 20, 26))
                            .stroke(Stroke::new(1.0, Color32::from_rgb(34, 38, 48)))
                            .corner_radius(6.0)
                            .inner_margin(egui::Margin::symmetric(10, 4))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;

                                    ui.label(
                                        RichText::new(t!("develop_before_after"))
                                            .strong()
                                            .size(11.5)
                                            .color(Color32::from_rgb(170, 175, 188)),
                                    );

                                    let tip_toggle = t!("develop_before_toggle_tooltip");
                                    let tip_split = t!("develop_split_tooltip");
                                    let tip_side = t!("develop_side_by_side_tooltip");
                                    let modes = [
                                        (BeforeAfterMode::Normal, t!("develop_mode_normal"), ""),
                                        (
                                            BeforeAfterMode::Toggle,
                                            t!("develop_mode_toggle"),
                                            tip_toggle.as_str(),
                                        ),
                                        (
                                            BeforeAfterMode::SplitView,
                                            t!("develop_mode_split"),
                                            tip_split.as_str(),
                                        ),
                                        (
                                            BeforeAfterMode::SideBySide,
                                            t!("develop_mode_side_by_side"),
                                            tip_side.as_str(),
                                        ),
                                    ];

                                    for (mode, text, tip) in modes {
                                        let is_sel = self.before_after_mode == mode;
                                        let (bg, stroke, fg) = if is_sel {
                                            (
                                                Color32::from_rgb(46, 56, 78),
                                                Stroke::new(1.0, Color32::from_rgb(70, 92, 138)),
                                                Color32::from_rgb(250, 252, 255),
                                            )
                                        } else {
                                            (
                                                Color32::from_rgb(24, 26, 33),
                                                Stroke::new(1.0, Color32::from_rgb(38, 42, 52)),
                                                Color32::from_rgb(165, 170, 182),
                                            )
                                        };
                                        let btn = egui::Button::new(
                                            RichText::new(text).size(11.0).color(fg).strong(),
                                        )
                                        .fill(bg)
                                        .stroke(stroke)
                                        .corner_radius(4.0)
                                        .min_size(Vec2::new(0.0, 22.0));

                                        let mut resp = ui.add(btn);
                                        if !tip.is_empty() {
                                            resp = resp.on_hover_text(tip);
                                        }
                                        if resp.clicked() {
                                            if mode != BeforeAfterMode::Normal
                                                && self.crop_mode_active
                                            {
                                                self.crop_mode_active = false;
                                                self.re_render_edits(frame);
                                            }
                                            self.before_after_mode = mode;
                                        }
                                    }

                                    ui.add_space(2.0);
                                    ui.separator();
                                    ui.add_space(2.0);

                                    // Zoom controls
                                    let is_fit =
                                        !self.is_one_to_one && (self.zoom_level - 1.0).abs() < 0.01;
                                    let (fit_bg, fit_stroke, fit_fg) = if is_fit {
                                        (
                                            Color32::from_rgb(46, 56, 78),
                                            Stroke::new(1.0, Color32::from_rgb(70, 92, 138)),
                                            Color32::from_rgb(250, 252, 255),
                                        )
                                    } else {
                                        (
                                            Color32::from_rgb(24, 26, 33),
                                            Stroke::new(1.0, Color32::from_rgb(38, 42, 52)),
                                            Color32::from_rgb(165, 170, 182),
                                        )
                                    };
                                    let fit_btn = egui::Button::new(
                                        RichText::new(t!("develop_zoom_fit"))
                                            .size(11.0)
                                            .color(fit_fg)
                                            .strong(),
                                    )
                                    .fill(fit_bg)
                                    .stroke(fit_stroke)
                                    .corner_radius(4.0)
                                    .min_size(Vec2::new(0.0, 22.0));

                                    if ui.add(fit_btn).clicked() {
                                        self.is_one_to_one = false;
                                        self.zoom_level = 1.0;
                                        self.pan_offset = Vec2::ZERO;
                                    }

                                    let (one_bg, one_stroke, one_fg) = if self.is_one_to_one {
                                        (
                                            Color32::from_rgb(46, 56, 78),
                                            Stroke::new(1.0, Color32::from_rgb(70, 92, 138)),
                                            Color32::from_rgb(250, 252, 255),
                                        )
                                    } else {
                                        (
                                            Color32::from_rgb(24, 26, 33),
                                            Stroke::new(1.0, Color32::from_rgb(38, 42, 52)),
                                            Color32::from_rgb(165, 170, 182),
                                        )
                                    };
                                    let one_btn = egui::Button::new(
                                        RichText::new(t!("develop_zoom_100"))
                                            .size(11.0)
                                            .color(one_fg)
                                            .strong(),
                                    )
                                    .fill(one_bg)
                                    .stroke(one_stroke)
                                    .corner_radius(4.0)
                                    .min_size(Vec2::new(0.0, 22.0));

                                    if ui.add(one_btn).clicked() {
                                        self.is_one_to_one = true;
                                        self.ensure_full_res_rendered(frame);
                                    }

                                    let current_zoom_pct = (self.zoom_level * 100.0).round() as i32;
                                    let zoom_badge = egui::Button::new(
                                        RichText::new(format!("{current_zoom_pct}%"))
                                            .size(11.0)
                                            .color(Color32::from_rgb(150, 155, 168)),
                                    )
                                    .fill(Color32::from_rgb(22, 24, 30))
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(34, 38, 48)))
                                    .corner_radius(4.0)
                                    .min_size(Vec2::new(42.0, 22.0));
                                    ui.add_enabled(false, zoom_badge);
                                });
                            });
                        ui.add_space(6.0);

                        let before_id = self.egui_before_texture_id.unwrap_or(texture_id);

                        match self.before_after_mode {
                            BeforeAfterMode::Normal => {
                                let available_aspect = available_size.x / available_size.y.max(1.0);
                                let fit_size = if available_aspect > self.texture_aspect_ratio {
                                    Vec2::new(
                                        available_size.y * self.texture_aspect_ratio,
                                        available_size.y,
                                    )
                                } else {
                                    Vec2::new(
                                        available_size.x,
                                        available_size.x / self.texture_aspect_ratio,
                                    )
                                };

                                let (display_size, active_tex_id) = if self.is_one_to_one {
                                    let (w, h) = if let Some(img) = &self.current_image {
                                        (img.width as f32, img.height as f32)
                                    } else {
                                        (fit_size.x, fit_size.y)
                                    };
                                    (
                                        Vec2::new(w, h) * self.zoom_level,
                                        self.egui_full_texture_id.unwrap_or(texture_id),
                                    )
                                } else {
                                    (fit_size * self.zoom_level, texture_id)
                                };

                                let (viewport_rect, viewport_resp) = ui.allocate_exact_size(
                                    available_size,
                                    egui::Sense::click_and_drag(),
                                );

                                // Mouse-wheel zoom
                                if viewport_resp.hovered() {
                                    let scroll_y = ctx.input(|i| i.smooth_scroll_delta.y);
                                    if scroll_y.abs() > 0.01 {
                                        let zoom_mult =
                                            if scroll_y > 0.0 { 1.15 } else { 1.0 / 1.15 };
                                        let new_zoom =
                                            (self.zoom_level * zoom_mult).clamp(0.2, 10.0);
                                        self.zoom_level = new_zoom;
                                        if self.zoom_level >= 1.0 && !self.is_one_to_one {
                                            self.is_one_to_one = true;
                                            self.ensure_full_res_rendered(frame);
                                        } else if self.zoom_level < 0.95 && self.is_one_to_one {
                                            self.is_one_to_one = false;
                                        }
                                        ctx.request_repaint();
                                    }
                                }

                                // Drag-panning
                                if viewport_resp.dragged() && !self.crop_mode_active {
                                    self.pan_offset += viewport_resp.drag_delta();
                                    ctx.request_repaint();
                                }

                                // Double-click toggle 1:1 / Fit
                                if viewport_resp.double_clicked() && !self.crop_mode_active {
                                    self.is_one_to_one = !self.is_one_to_one;
                                    if self.is_one_to_one {
                                        self.ensure_full_res_rendered(frame);
                                    } else {
                                        self.zoom_level = 1.0;
                                        self.pan_offset = Vec2::ZERO;
                                    }
                                    ctx.request_repaint();
                                }

                                let image_center = viewport_rect.center() + self.pan_offset;
                                let image_rect = Rect::from_center_size(image_center, display_size);

                                let painter = ui.painter_at(viewport_rect);
                                painter.image(
                                    active_tex_id,
                                    image_rect,
                                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                    Color32::WHITE,
                                );

                                // White balance eyedropper interaction
                                if self.wb_eyedropper_active {
                                    if viewport_resp.hovered() {
                                        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                                    }
                                    if viewport_resp.clicked() {
                                        if let Some(click_pos) = viewport_resp.interact_pointer_pos() {
                                            if image_rect.contains(click_pos) {
                                                if let Some(img) = &self.current_image {
                                                    let u_out = ((click_pos.x - image_rect.min.x)
                                                        / image_rect.width().max(1.0))
                                                    .clamp(0.0, 1.0);
                                                    let v_out = ((click_pos.y - image_rect.min.y)
                                                        / image_rect.height().max(1.0))
                                                    .clamp(0.0, 1.0);
                                                    let aspect = (img.width as f32)
                                                        / (img.height.max(1) as f32);
                                                    let (u, v) = beam_core::geometry::map_output_uv_to_source_with_aspect(
                                                        u_out,
                                                        v_out,
                                                        aspect,
                                                        &self.edit_stack.crop,
                                                    );

                                                    // Sample 3x3 neighborhood in source image
                                                    let cx = ((u * img.width as f32) as i32)
                                                        .clamp(0, img.width as i32 - 1);
                                                    let cy = ((v * img.height as f32) as i32)
                                                        .clamp(0, img.height as i32 - 1);
                                                    let mut sum = [0.0f32; 3];
                                                    let mut count = 0.0f32;
                                                    for dy in -1..=1 {
                                                        for dx in -1..=1 {
                                                            let sx = (cx + dx)
                                                                .clamp(0, img.width as i32 - 1)
                                                                as usize;
                                                            let sy = (cy + dy)
                                                                .clamp(0, img.height as i32 - 1)
                                                                as usize;
                                                            let idx = (sy * img.width as usize + sx) * 4;
                                                            if idx + 2 < img.data.len() {
                                                                sum[0] += img.data[idx];
                                                                sum[1] += img.data[idx + 1];
                                                                sum[2] += img.data[idx + 2];
                                                                count += 1.0;
                                                            }
                                                        }
                                                    }
                                                    if count > 0.0 {
                                                        let avg = [
                                                            sum[0] / count,
                                                            sum[1] / count,
                                                            sum[2] / count,
                                                        ];
                                                        let (temp, tint) = beam_core::color::compute_wb_from_neutral_rgb(avg);
                                                        self.edit_stack.white_balance.enabled = true;
                                                        self.edit_stack.white_balance.temperature = temp;
                                                        self.edit_stack.white_balance.tint = tint;
                                                        let label = t!(
                                                            "history_temperature",
                                                            "val" => format!("{:+0.0}", temp)
                                                        );
                                                        self.push_history_step(label);
                                                        self.re_render_edits(frame);
                                                    }
                                                }
                                                self.wb_eyedropper_active = false;
                                            }
                                        }
                                    }
                                }

                                if self.crop_mode_active {
                                    if let Some(img) = &self.current_image {
                                        let orig_aspect = if self.edit_stack.crop.rotation == 90
                                            || self.edit_stack.crop.rotation == 270
                                        {
                                            img.height as f32 / img.width as f32
                                        } else {
                                            img.width as f32 / img.height as f32
                                        };
                                        if show_crop_overlay(
                                            ui,
                                            image_rect,
                                            &mut self.edit_stack.crop,
                                            orig_aspect,
                                        ) {
                                            if self.slider_drag_start_stack.is_none() {
                                                let baseline = if self.history_index
                                                    < self.history.len()
                                                {
                                                    self.history[self.history_index].stack.clone()
                                                } else {
                                                    self.edit_stack.clone()
                                                };
                                                self.slider_drag_start_stack = Some(baseline);
                                            }
                                            self.slider_drag_action_name = Some(t!(
                                                "history_crop",
                                                "preset" => format!(
                                                    "{:?}",
                                                    self.edit_stack.crop.preset
                                                )
                                            ));
                                            self.re_render_edits(frame);
                                            ctx.request_repaint();
                                        }
                                    }
                                } else if self.show_mask_overlay {
                                    if let Some(sel_id) = self.selected_mask_id {
                                        if let Some(mask) = self
                                            .edit_stack
                                            .masks
                                            .iter_mut()
                                            .find(|m| m.id == sel_id)
                                        {
                                            let (m_changed, m_stopped) =
                                                show_mask_overlay(ui, image_rect, mask);
                                            if m_changed {
                                                self.re_render_edits(frame);
                                                ctx.request_repaint();
                                            }
                                            if m_stopped {
                                                self.push_history_step(t!("history_masks"));
                                            }
                                        }
                                    }
                                }
                            }
                            BeforeAfterMode::Toggle => {
                                let available_aspect = available_size.x / available_size.y.max(1.0);
                                let display_size = if available_aspect > self.texture_aspect_ratio {
                                    Vec2::new(
                                        available_size.y * self.texture_aspect_ratio,
                                        available_size.y,
                                    )
                                } else {
                                    Vec2::new(
                                        available_size.x,
                                        available_size.x / self.texture_aspect_ratio,
                                    )
                                };

                                ui.centered_and_justified(|ui| {
                                    let (rect, _) =
                                        ui.allocate_exact_size(display_size, egui::Sense::hover());
                                    ui.painter().image(
                                        before_id,
                                        rect,
                                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                        Color32::WHITE,
                                    );
                                    let badge_rect = Rect::from_min_size(
                                        rect.min + Vec2::new(12.0, 12.0),
                                        Vec2::new(80.0, 24.0),
                                    );
                                    ui.painter().rect_filled(
                                        badge_rect,
                                        4.0,
                                        Color32::from_black_alpha(200),
                                    );
                                    ui.painter().text(
                                        badge_rect.center(),
                                        Align2::CENTER_CENTER,
                                        t!("develop_before_badge"),
                                        FontId::proportional(12.0),
                                        Color32::WHITE,
                                    );
                                });
                            }
                            BeforeAfterMode::SideBySide => {
                                let half_w = ((available_size.x - 12.0) * 0.5).max(10.0);
                                let half_aspect = half_w / available_size.y.max(1.0);
                                let display_size = if half_aspect > self.texture_aspect_ratio {
                                    Vec2::new(
                                        available_size.y * self.texture_aspect_ratio,
                                        available_size.y,
                                    )
                                } else {
                                    Vec2::new(half_w, half_w / self.texture_aspect_ratio)
                                };

                                ui.centered_and_justified(|ui| {
                                    ui.horizontal(|ui| {
                                        // Left: Before
                                        let (rect_before, _) = ui.allocate_exact_size(
                                            display_size,
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().image(
                                            before_id,
                                            rect_before,
                                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                            Color32::WHITE,
                                        );
                                        let badge_rect = Rect::from_min_size(
                                            rect_before.min + Vec2::new(8.0, 8.0),
                                            Vec2::new(72.0, 22.0),
                                        );
                                        ui.painter().rect_filled(
                                            badge_rect,
                                            4.0,
                                            Color32::from_black_alpha(200),
                                        );
                                        ui.painter().text(
                                            badge_rect.center(),
                                            Align2::CENTER_CENTER,
                                            t!("develop_before_badge"),
                                            FontId::proportional(11.0),
                                            Color32::WHITE,
                                        );

                                        ui.add_space(8.0);

                                        // Right: After
                                        let (rect_after, _) = ui.allocate_exact_size(
                                            display_size,
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().image(
                                            texture_id,
                                            rect_after,
                                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                            Color32::WHITE,
                                        );
                                        let badge_rect = Rect::from_min_size(
                                            rect_after.min + Vec2::new(8.0, 8.0),
                                            Vec2::new(72.0, 22.0),
                                        );
                                        ui.painter().rect_filled(
                                            badge_rect,
                                            4.0,
                                            Color32::from_black_alpha(200),
                                        );
                                        ui.painter().text(
                                            badge_rect.center(),
                                            Align2::CENTER_CENTER,
                                            t!("develop_after_badge"),
                                            FontId::proportional(11.0),
                                            Color32::WHITE,
                                        );
                                    });
                                });
                            }
                            BeforeAfterMode::SplitView => {
                                let available_aspect = available_size.x / available_size.y.max(1.0);
                                let display_size = if available_aspect > self.texture_aspect_ratio {
                                    Vec2::new(
                                        available_size.y * self.texture_aspect_ratio,
                                        available_size.y,
                                    )
                                } else {
                                    Vec2::new(
                                        available_size.x,
                                        available_size.x / self.texture_aspect_ratio,
                                    )
                                };

                                ui.centered_and_justified(|ui| {
                                    let (rect, response) = ui.allocate_exact_size(
                                        display_size,
                                        egui::Sense::click_and_drag(),
                                    );

                                    if response.dragged() {
                                        if let Some(pos) = response.interact_pointer_pos() {
                                            let fraction = ((pos.x - rect.min.x)
                                                / rect.width().max(1.0))
                                            .clamp(0.02, 0.98);
                                            self.split_view_divider = fraction;
                                        }
                                    }

                                    let split_x =
                                        rect.min.x + rect.width() * self.split_view_divider;

                                    // Left: Before
                                    let left_rect = Rect::from_min_max(
                                        rect.min,
                                        Pos2::new(split_x, rect.max.y),
                                    );
                                    let left_uv = Rect::from_min_max(
                                        Pos2::ZERO,
                                        Pos2::new(self.split_view_divider, 1.0),
                                    );
                                    ui.painter().image(
                                        before_id,
                                        left_rect,
                                        left_uv,
                                        Color32::WHITE,
                                    );

                                    // Right: After
                                    let right_rect = Rect::from_min_max(
                                        Pos2::new(split_x, rect.min.y),
                                        rect.max,
                                    );
                                    let right_uv = Rect::from_min_max(
                                        Pos2::new(self.split_view_divider, 0.0),
                                        Pos2::new(1.0, 1.0),
                                    );
                                    ui.painter().image(
                                        texture_id,
                                        right_rect,
                                        right_uv,
                                        Color32::WHITE,
                                    );

                                    // Divider Line
                                    ui.painter().line_segment(
                                        [
                                            Pos2::new(split_x, rect.min.y),
                                            Pos2::new(split_x, rect.max.y),
                                        ],
                                        Stroke::new(2.0, Color32::WHITE),
                                    );

                                    // Center Handle
                                    let center_pos = Pos2::new(split_x, rect.center().y);
                                    ui.painter().circle_filled(
                                        center_pos,
                                        14.0,
                                        Color32::from_black_alpha(200),
                                    );
                                    ui.painter().circle_stroke(
                                        center_pos,
                                        14.0,
                                        Stroke::new(1.5, Color32::WHITE),
                                    );
                                    ui.painter().text(
                                        center_pos,
                                        Align2::CENTER_CENTER,
                                        "↔",
                                        FontId::proportional(13.0),
                                        Color32::WHITE,
                                    );

                                    // Badges
                                    let before_badge = Rect::from_min_size(
                                        rect.min + Vec2::new(8.0, 8.0),
                                        Vec2::new(72.0, 22.0),
                                    );
                                    ui.painter().rect_filled(
                                        before_badge,
                                        4.0,
                                        Color32::from_black_alpha(200),
                                    );
                                    ui.painter().text(
                                        before_badge.center(),
                                        Align2::CENTER_CENTER,
                                        t!("develop_before_badge"),
                                        FontId::proportional(11.0),
                                        Color32::WHITE,
                                    );

                                    let after_badge = Rect::from_min_size(
                                        Pos2::new(rect.max.x - 80.0, rect.min.y + 8.0),
                                        Vec2::new(72.0, 22.0),
                                    );
                                    ui.painter().rect_filled(
                                        after_badge,
                                        4.0,
                                        Color32::from_black_alpha(200),
                                    );
                                    ui.painter().text(
                                        after_badge.center(),
                                        Align2::CENTER_CENTER,
                                        t!("develop_after_badge"),
                                        FontId::proportional(11.0),
                                        Color32::WHITE,
                                    );
                                });
                            }
                        }
                    } else if !self.is_loading {
                        // Empty state
                        ui.centered_and_justified(|ui| {
                            ui.vertical_centered(|ui| {
                                ui.add_space(available_size.y * 0.15);
                                ui.image(egui::load::SizedTexture::new(
                                    self.logo_texture.id(),
                                    Vec2::new(128.0, 128.0),
                                ));
                                ui.add_space(16.0);
                                ui.heading(RichText::new(APP_NAME).size(32.0));
                                ui.label(t!("develop_empty_desc"));
                                ui.add_space(20.0);
                                if ui
                                    .button(RichText::new(t!("develop_open_btn")).size(16.0))
                                    .clicked()
                                {
                                    self.open_file_dialog();
                                }
                            });
                        });
                    } else {
                        // Loading state
                        ui.centered_and_justified(|ui| {
                            ui.vertical_centered(|ui| {
                                ui.add_space(available_size.y * 0.4);
                                ui.spinner();
                                ui.add_space(10.0);
                                ui.label(t!("develop_decoding"));
                            });
                        });
                    }
                });
            }
        }

        // About dialog
        if self.show_about {
            egui::Window::new(t!("about_title"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(&ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(8.0);
                        ui.image(egui::load::SizedTexture::new(
                            self.logo_texture.id(),
                            Vec2::new(96.0, 96.0),
                        ));
                        ui.add_space(10.0);
                        ui.heading(RichText::new(APP_NAME).size(26.0).strong());
                        ui.label(t!("about_version", "version" => env!("CARGO_PKG_VERSION")));
                        ui.label(t!("about_license_text"));
                        ui.add_space(10.0);
                        ui.label(t!("about_desc_1"));
                        ui.label(t!("about_desc_2"));
                        ui.add_space(15.0);
                        if ui.button(t!("about_close")).clicked() {
                            self.show_about = false;
                        }
                    });
                });
        }

        // Settings dialog
        self.show_settings_dialog_window(&ctx);

        // Presets & Sync dialogs
        self.show_new_preset_dialog_window(&ctx);
        self.show_copy_dialog_window(&ctx);
        self.show_sync_dialog_window(&ctx);

        // Export dialogs
        self.show_export_dialog_window(&ctx);
        self.show_export_progress_window(&ctx);

        // Unsaved changes dialog
        self.show_unsaved_changes_dialog_window(&ctx);
    }
}

impl BeamApp {
    /// Open export dialog for current photo or library selection
    pub fn open_export_dialog(&mut self) {
        let mut targets = Vec::new();
        match self.current_view {
            AppView::Develop => {
                if let Some(path) = &self.current_path {
                    targets.push((path.clone(), self.edit_stack.clone()));
                }
            }
            AppView::Library => {
                let selected = &self.library_state.selected_photo_ids;
                let photos: Vec<_> = if !selected.is_empty() {
                    self.library_state
                        .photos
                        .iter()
                        .filter(|p| selected.contains(&p.id))
                        .collect()
                } else if let Some(id) = self.library_state.selected_photo_id {
                    self.library_state
                        .photos
                        .iter()
                        .filter(|p| p.id == id)
                        .collect()
                } else {
                    self.library_state.photos.iter().collect()
                };

                for photo in photos {
                    let path = PathBuf::from(&photo.file_path);
                    let sidecar_p = Sidecar::path_for(&path);
                    let stack = Sidecar::load_from_file(&sidecar_p)
                        .map(|s| s.edit_stack)
                        .unwrap_or_default();
                    targets.push((path, stack));
                }
            }
        }

        if !targets.is_empty() {
            self.export_targets = targets;
            self.show_export_dialog = true;
        }
    }

    /// Open export dialog with explicitly provided photo paths
    pub fn open_export_dialog_with_paths(&mut self, paths: Vec<PathBuf>) {
        let mut targets = Vec::new();
        for path in paths {
            let sidecar_p = Sidecar::path_for(&path);
            let stack = Sidecar::load_from_file(&sidecar_p)
                .map(|s| s.edit_stack)
                .unwrap_or_default();
            targets.push((path, stack));
        }
        if !targets.is_empty() {
            self.export_targets = targets;
            self.show_export_dialog = true;
        }
    }

    /// Launch background export thread
    pub fn start_export(&mut self) {
        if self.export_is_running || self.export_targets.is_empty() {
            return;
        }

        self.show_export_dialog = false;
        self.show_export_progress = true;
        self.export_is_running = true;
        self.export_status_message = None;
        self.export_progress_data = None;

        let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.export_cancel_flag = Some(cancel_flag.clone());

        let tx = self.tx.clone();
        let resize = match self.export_resize_mode_index {
            1 => beam_media::ResizeMode::LongEdge(self.export_resize_long_edge.max(1)),
            2 => beam_media::ResizeMode::Percentage(self.export_resize_percentage.max(1.0)),
            3 => beam_media::ResizeMode::Exact {
                width: self.export_resize_width.max(1),
                height: self.export_resize_height.max(1),
                lock_aspect: self.export_resize_lock_aspect,
            },
            _ => beam_media::ResizeMode::Original,
        };

        let sharpening = match self.export_sharpening_index {
            1 => beam_media::OutputSharpening::Screen,
            2 => beam_media::OutputSharpening::Print,
            _ => beam_media::OutputSharpening::None,
        };

        let config = beam_media::ExportConfig {
            output_dir: self.export_output_dir.clone(),
            format: match self.export_format_index {
                1 => beam_media::ExportFormat::Png,
                2 => beam_media::ExportFormat::Tiff16,
                3 => beam_media::ExportFormat::WebP,
                _ => beam_media::ExportFormat::Jpeg {
                    quality: self.export_jpeg_quality,
                },
            },
            color_space: self.export_color_space,
            resize,
            sharpening,
            export_exif: self.export_include_exif,
            filename_template: if self.export_template.trim().is_empty() {
                "{name}".to_string()
            } else {
                self.export_template.clone()
            },
            custom_suffix: if self.export_suffix.trim().is_empty() {
                None
            } else {
                Some(self.export_suffix.clone())
            },
            watermark: beam_media::WatermarkConfig {
                enabled: self.export_watermark_enabled,
                text: self.export_watermark_text.clone(),
                image_path: self.export_watermark_logo_path.clone(),
                anchor: self.export_watermark_anchor,
                opacity: self.export_watermark_opacity,
                scale: self.export_watermark_scale,
                margin: 0.03,
            },
        };
        let targets = self.export_targets.clone();

        thread::spawn(move || {
            let cancel_clone = cancel_flag.clone();
            let tx_prog = tx.clone();
            let batch_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                beam_media::export_batch(&targets, &config, &cancel_clone, move |progress| {
                    let _ = tx_prog.send(BackgroundMsg::ExportProgress(progress));
                })
            }));

            let cancelled = cancel_flag.load(std::sync::atomic::Ordering::Relaxed);
            let success_count = match batch_res {
                Ok(Ok(ref list)) => list.len(),
                Ok(Err(ref e)) => {
                    log::error!("Export batch error: {e}");
                    0
                }
                Err(panic_err) => {
                    log::error!("Export batch panicked: {panic_err:?}");
                    0
                }
            };
            let _ = tx.send(BackgroundMsg::ExportFinished {
                success_count,
                total: targets.len(),
                cancelled,
            });
        });
    }

    /// Save current window layout and panel dimensions to AppConfig
    pub fn save_session_state(&mut self) {
        self.app_config.filmstrip_visible = self.filmstrip_visible;
        self.app_config.filmstrip_height = Some(self.filmstrip_height);
        self.app_config.show_history_panel = self.show_history_panel;
        self.app_config.show_develop_panel = self.show_develop_panel;
        let _ = self.app_config.save();
    }

    /// Update star rating for active photo
    pub fn set_active_photo_rating(&mut self, rating: u8) {
        if let Some(path) = &self.current_path {
            let sidecar_p = Sidecar::path_for(path);
            let mut sidecar = Sidecar::load_from_file(&sidecar_p).unwrap_or_else(|_| {
                let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
                Sidecar::new(fname)
            });
            sidecar.set_rating(rating);
            let _ = sidecar.save_to_file(&sidecar_p);

            let path_str = path.to_string_lossy().to_string();
            if let Some(photo) = self
                .library_state
                .photos
                .iter_mut()
                .find(|p| p.file_path == path_str)
            {
                photo.rating = rating;
                let _ = self.library_db.update_rating(photo.id, rating);
            }
            self.sidecar_status = format!("Rating: {rating}★");
        }
    }

    /// Update color label for active photo
    pub fn set_active_photo_color_label(&mut self, label: ColorLabel) {
        if let Some(path) = &self.current_path {
            let sidecar_p = Sidecar::path_for(path);
            let mut sidecar = Sidecar::load_from_file(&sidecar_p).unwrap_or_else(|_| {
                let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
                Sidecar::new(fname)
            });
            sidecar.set_color_label(label);
            let _ = sidecar.save_to_file(&sidecar_p);

            let path_str = path.to_string_lossy().to_string();
            if let Some(photo) = self
                .library_state
                .photos
                .iter_mut()
                .find(|p| p.file_path == path_str)
            {
                photo.color_label = label;
                let _ = self.library_db.update_color_label(photo.id, label);
            }
            self.sidecar_status = format!("Label: {:?}", label);
        }
    }

    /// Navigate to next or previous photo in the active library / filmstrip list
    pub fn navigate_photo(&mut self, forward: bool) {
        if self.library_state.photos.is_empty() {
            return;
        }
        let current_pos = self.current_path.as_ref().and_then(|path| {
            let p_str = path.to_string_lossy();
            self.library_state
                .photos
                .iter()
                .position(|p| p.file_path == p_str)
        });

        let next_idx = match current_pos {
            Some(idx) => {
                if forward {
                    (idx + 1) % self.library_state.photos.len()
                } else if idx == 0 {
                    self.library_state.photos.len() - 1
                } else {
                    idx - 1
                }
            }
            None => 0,
        };

        let target_path = PathBuf::from(&self.library_state.photos[next_idx].file_path);
        self.request_load_file_path(target_path);
    }

    /// Render horizontal filmstrip thumbnail carousel at bottom of Develop view
    fn show_develop_filmstrip(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!("develop_filmstrip")).strong().size(12.0));
            if ui
                .small_button("▼")
                .on_hover_text("Hide Filmstrip")
                .clicked()
            {
                self.filmstrip_visible = false;
                self.app_config.filmstrip_visible = false;
                let _ = self.app_config.save();
            }
        });

        ui.add_space(2.0);

        if self.library_state.photos.is_empty() {
            ui.label(t!("develop_filmstrip_empty"));
            return;
        }

        let mut clicked_photo = None;

        egui::ScrollArea::horizontal()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for photo in &self.library_state.photos {
                        let is_active = self
                            .current_path
                            .as_ref()
                            .map(|p| p.to_string_lossy() == photo.file_path)
                            .unwrap_or(false);

                        let file_path = PathBuf::from(&photo.file_path);
                        let file_name = file_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("image");

                        ui.vertical(|ui| {
                            let thumb_size = egui::vec2(60.0, 60.0);
                            let status = self.library_state.thumbnail_status.get(&photo.id);

                            let frame_stroke = if is_active {
                                Stroke::new(2.0, Color32::from_rgb(60, 140, 255))
                            } else {
                                Stroke::new(1.0, Color32::from_rgb(50, 50, 55))
                            };

                            let resp = egui::Frame::canvas(ui.style())
                                .stroke(frame_stroke)
                                .inner_margin(2.0)
                                .show(ui, |ui| match status {
                                    Some(ThumbnailStatus::Ready(tex_id)) => ui.image(
                                        egui::load::SizedTexture::new(tex_id.id(), thumb_size),
                                    ),
                                    _ => {
                                        let (rect, resp) = ui
                                            .allocate_exact_size(thumb_size, egui::Sense::click());
                                        ui.painter().rect_filled(
                                            rect,
                                            2.0,
                                            Color32::from_rgb(30, 30, 35),
                                        );
                                        resp
                                    }
                                })
                                .response;

                            // Truncated filename
                            let truncated_name = if file_name.len() > 10 {
                                format!("{}...", &file_name[..8])
                            } else {
                                file_name.to_string()
                            };
                            let label_resp = ui.small(truncated_name);

                            if resp.clicked() || label_resp.clicked() {
                                clicked_photo = Some(photo.file_path.clone());
                            }
                        });
                        ui.add_space(4.0);
                    }
                });
            });

        if let Some(target_path) = clicked_photo {
            self.request_load_file_path(PathBuf::from(target_path));
        }
    }

    /// Check if current image or project has unsaved edits
    pub fn has_unsaved_changes(&self) -> bool {
        if self.current_image.is_none() && self.current_path.is_none() {
            return false;
        }
        match &self.last_saved_stack {
            Some(saved) => *saved != self.edit_stack,
            None => self.edit_stack != EditStack::default(),
        }
    }
}

#[cfg(test)]
mod tests {
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
}
