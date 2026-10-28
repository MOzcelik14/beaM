use crate::views::library::{
    show_library_view, LibraryAction, LibraryViewState, ThumbnailRequest, ThumbnailStatus,
};
use crate::widgets::crop::show_crop_overlay;
use crate::widgets::curve::show_tone_curve;
use crate::widgets::histogram::{show_histogram_with_clipping, HistogramData, HistogramMode};
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
}

enum BackgroundMsg {
    Started(PathBuf),
    Success(Box<DecodedImage>),
    Error(String),
    ImportComplete {
        count: usize,
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

    // Clipping overlays
    show_shadow_clipping: bool,
    show_highlight_clipping: bool,

    // Filmstrip & Fullscreen
    filmstrip_visible: bool,
    filmstrip_height: f32,
    is_fullscreen: bool,

    // Settings modal tab
    settings_tab: SettingsTab,

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
    export_format_is_png: bool,
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
    export_targets: Vec<(PathBuf, EditStack)>,
    export_cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    export_progress_data: Option<beam_media::BatchProgress>,
    export_is_running: bool,
    export_status_message: Option<String>,
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
                    let mut q = lock.lock().unwrap();
                    while q.is_empty() {
                        q = cvar.wait(q).unwrap();
                    }
                    q.pop_front()
                };

                let Some(req) = req else { continue };

                let thumb_path =
                    beam_media::thumbnail_cache_path(&req.source_path, &worker_thumb_dir);

                let result = if thumb_path.exists() {
                    match image::open(&thumb_path) {
                        Ok(img) => {
                            let rgba = img.to_rgba8();
                            let size = [rgba.width() as usize, rgba.height() as usize];
                            Ok((
                                thumb_path.clone(),
                                egui::ColorImage::from_rgba_unmultiplied(size, &rgba.into_raw()),
                            ))
                        }
                        Err(_) => generate_and_save_thumb(&req, &thumb_path),
                    }
                } else {
                    generate_and_save_thumb(&req, &thumb_path)
                };

                match result {
                    Ok((path, color_image)) => {
                        let _ = worker_tx.send(BackgroundMsg::ThumbnailReady {
                            photo_id: req.photo_id,
                            thumb_path: path.to_string_lossy().to_string(),
                            image: color_image,
                        });
                    }
                    Err(e) => {
                        let _ = worker_tx.send(BackgroundMsg::ThumbnailFailed {
                            photo_id: req.photo_id,
                            error: e,
                        });
                    }
                }
            }
        });

        let preset_dir = default_presets_dir();
        let user_presets = load_user_presets(&preset_dir);

        Self {
            current_view: AppView::Library,
            library_db,
            library_state: LibraryViewState::default(),
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
            filmstrip_visible: app_config.filmstrip_visible,
            filmstrip_height: app_config.filmstrip_height.unwrap_or(110.0),
            is_fullscreen: false,
            settings_tab: SettingsTab::General,
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
            export_format_is_png: false,
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
            export_targets: Vec::new(),
            export_cancel_flag: None,
            export_progress_data: None,
            export_is_running: false,
            export_status_message: None,
        }
    }

    /// Open file dialog and spawn background thread to decode the RAW image
    pub fn open_file_dialog(&mut self) {
        let tx = self.tx.clone();
        let dialog_title = t!("menu_file_open");
        thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .set_title(dialog_title)
                .add_filter(
                    "RAW & Raster Photos",
                    &[
                        "arw", "cr2", "cr3", "nef", "dng", "raf", "rw2", "orf", "pef", "jpg",
                        "jpeg", "png", "tif", "tiff", "webp",
                    ],
                )
                .pick_file();

            if let Some(path) = file {
                let _ = tx.send(BackgroundMsg::Started(path.clone()));
                match decode_image(&path) {
                    Ok(decoded) => {
                        let _ = tx.send(BackgroundMsg::Success(Box::new(decoded)));
                    }
                    Err(err) => {
                        let _ = tx.send(BackgroundMsg::Error(err.to_string()));
                    }
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

                match db.scan_and_import_folder(&dir, &thumb_dir, true) {
                    Ok(count) => {
                        let _ = tx.send(BackgroundMsg::ImportComplete { count });
                    }
                    Err(e) => {
                        let _ = tx.send(BackgroundMsg::Error(format!("Folder import failed: {e}")));
                    }
                }
            }
        });
    }

    /// Load a specific file path asynchronously
    pub fn load_file_path(&mut self, path: PathBuf) {
        self.current_view = AppView::Develop;
        let tx = self.tx.clone();
        let _ = tx.send(BackgroundMsg::Started(path.clone()));
        thread::spawn(move || match decode_image(&path) {
            Ok(decoded) => {
                let _ = tx.send(BackgroundMsg::Success(Box::new(decoded)));
            }
            Err(err) => {
                let _ = tx.send(BackgroundMsg::Error(err.to_string()));
            }
        });
    }

    /// Process messages from the background decoding and import threads
    fn check_background_messages(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                BackgroundMsg::Started(path) => {
                    self.is_loading = true;
                    self.loading_filename = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string());
                    self.current_path = Some(path.clone());
                    self.error_message = None;

                    // Compute sidecar path and load existing edits if present
                    let sidecar_p = Sidecar::path_for(&path);
                    if sidecar_p.exists() {
                        match Sidecar::load_from_file(&sidecar_p) {
                            Ok(sidecar) => {
                                self.edit_stack = sidecar.edit_stack;
                                self.history = sidecar.history;
                                self.history_index = sidecar.history_index;
                                self.sidecar_status = "Loaded from disk".to_string();
                            }
                            Err(e) => {
                                log::warn!("Failed to read existing sidecar: {e}");
                                self.edit_stack = EditStack::default();
                                self.history = vec![HistoryStep::new(
                                    t!("history_initial"),
                                    EditStack::default(),
                                )];
                                self.history_index = 0;
                                self.sidecar_status = "Failed to parse sidecar".to_string();
                            }
                        }
                    } else {
                        self.edit_stack = EditStack::default();
                        self.history = vec![HistoryStep::new(
                            t!("history_initial"),
                            EditStack::default(),
                        )];
                        self.history_index = 0;
                        self.sidecar_status = "New sidecar".to_string();
                    }
                    self.slider_drag_start_stack = None;
                    self.slider_drag_action_name = None;
                    self.sidecar_path = Some(sidecar_p);
                    ctx.request_repaint();
                }
                BackgroundMsg::Success(decoded) => {
                    self.is_loading = false;
                    self.loading_filename = None;
                    self.current_metadata = Some(decoded.metadata.clone());
                    self.upload_proxy_and_render(frame, *decoded);
                    ctx.request_repaint();
                }
                BackgroundMsg::Error(err_msg) => {
                    self.is_loading = false;
                    self.loading_filename = None;
                    self.library_state.is_importing = false;
                    self.error_message = Some(err_msg);
                    ctx.request_repaint();
                }
                BackgroundMsg::ImportComplete { count } => {
                    self.library_state.is_importing = false;
                    self.library_state.import_status = None;
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
                        .thumbnail_status
                        .insert(photo_id, ThumbnailStatus::Ready(tex));
                    let _ = self.library_db.update_thumbnail_path(photo_id, &thumb_path);
                    ctx.request_repaint();
                }
                BackgroundMsg::ThumbnailFailed { photo_id, error } => {
                    log::warn!("Thumbnail generation failed for photo {photo_id}: {error}");
                    self.library_state
                        .thumbnail_status
                        .insert(photo_id, ThumbnailStatus::Failed(error));
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

        // Run edit pipeline (White balance -> Exposure -> Contrast -> Display)
        self.edit_pipeline.as_mut().unwrap().render(
            device,
            queue,
            &gpu_proxy,
            &display_view,
            &self.edit_stack,
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
        self.edit_pipeline.as_mut().unwrap().render(
            device,
            queue,
            &gpu_proxy,
            &before_view,
            &EditStack::default(),
        );

        // Register or update egui native textures
        let mut renderer = render_state.renderer.write();
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

        self.histogram_data = Some(HistogramData::compute_from_rgba32f(&decoded.proxy_data));
        self.texture_aspect_ratio =
            calculate_effective_aspect_ratio(decoded.width, decoded.height, &self.edit_stack.crop);
        self.gpu_display_texture = Some(display_texture);
        self.gpu_before_texture = Some(before_texture);
        self.gpu_proxy_image = Some(gpu_proxy);
        self.current_image = Some(Box::new(decoded));
        self.gpu_full_image = None;
        self.gpu_full_texture = None;
        self.egui_full_texture_id = None;
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

        pipeline.render_with_options(
            device,
            queue,
            gpu_full,
            &full_view,
            &self.edit_stack,
            self.crop_mode_active,
            self.show_shadow_clipping,
            self.show_highlight_clipping,
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

        pipeline.render_with_options(
            device,
            queue,
            gpu_proxy,
            &display_view,
            &self.edit_stack,
            self.crop_mode_active,
            self.show_shadow_clipping,
            self.show_highlight_clipping,
        );

        if self.is_one_to_one {
            self.ensure_full_res_rendered(frame);
        }

        // Save sidecar automatically
        self.save_sidecar();
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
        }
    }

    /// Redo to next history step
    pub fn redo(&mut self, frame: &mut eframe::Frame) {
        if self.history_index + 1 < self.history.len() {
            self.history_index += 1;
            self.edit_stack = self.history[self.history_index].stack.clone();
            self.re_render_edits(frame);
        }
    }

    /// Jump directly to a step by index
    pub fn jump_to_history_step(&mut self, frame: &mut eframe::Frame, index: usize) {
        if index < self.history.len() {
            self.history_index = index;
            self.edit_stack = self.history[index].stack.clone();
            self.re_render_edits(frame);
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
        if let (Some(path), Some(sidecar_path)) = (&self.current_path, &self.sidecar_path) {
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

            if let Err(e) = sidecar.save_to_file(sidecar_path) {
                log::error!("Failed to save sidecar to {}: {e}", sidecar_path.display());
                self.sidecar_status = t!("status_save_error", "error" => e.to_string());
            } else {
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
        let mut q = lock.lock().unwrap();
        for req in requests {
            if !q.iter().any(|r| r.photo_id == req.photo_id) {
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
            let mut q = lock.lock().unwrap();
            q.clear();
        }

        // 2. Clear disk cache files
        if let Ok(entries) = std::fs::read_dir(&self.thumbnail_cache_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    let _ = std::fs::remove_file(p);
                }
            }
        }

        // 3. Clear SQLite thumbnail paths
        let _ = self.library_db.clear_all_thumbnails();

        // 4. Reset in-memory textures and status
        self.library_state.thumbnail_status.clear();
        self.library_state.needs_refresh = true;
    }
}

impl eframe::App for BeamApp {
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.save_session_state();
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.check_background_messages(&ctx, frame);

        // Top menu bar
        Panel::top("menu_bar").show(ui, |ui| {
            MenuBar::new().ui(ui, |ui| {
                ui.label(RichText::new(APP_NAME).strong().size(15.0));
                ui.separator();

                // Navigation View Tabs
                let is_lib = self.current_view == AppView::Library;
                if ui
                    .selectable_label(is_lib, format!("📷 {} (G)", t!("view_library")))
                    .clicked()
                {
                    self.current_view = AppView::Library;
                }
                let is_dev = self.current_view == AppView::Develop;
                if ui
                    .selectable_label(is_dev, format!("🎨 {} (D)", t!("view_develop")))
                    .clicked()
                {
                    self.current_view = AppView::Develop;
                }

                ui.separator();

                ui.menu_button(t!("menu_file"), |ui| {
                    if ui.button(t!("menu_import_folder_shortcut")).clicked() {
                        self.import_folder_dialog();
                    }
                    if ui.button(t!("menu_open_photo_shortcut")).clicked() {
                        self.open_file_dialog();
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
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
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
                    let can_paste = self.copied_settings.is_some() && self.current_image.is_some();
                    if ui
                        .add_enabled(
                            can_paste,
                            egui::Button::new(format!("{} (Ctrl+V)", t!("menu_paste_settings"))),
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
                        .button(format!("🔄 {}", t!("menu_rebuild_thumbnails")))
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
        });

        // Global Keyboard shortcuts
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

        if ctx.input(|i| {
            i.key_pressed(egui::Key::Backslash) && !i.modifiers.command && !i.modifiers.ctrl
        }) {
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
            } else if ctx
                .input(|i| i.key_pressed(egui::Key::C) && !i.modifiers.command && !i.modifiers.ctrl)
            {
                self.crop_mode_active = !self.crop_mode_active;
                if self.crop_mode_active {
                    self.before_after_mode = BeforeAfterMode::Normal;
                }
                self.re_render_edits(frame);
                ctx.request_repaint();
            } else if self.crop_mode_active
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

        if ctx.input(|i| i.key_pressed(egui::Key::G) && !i.modifiers.command && !i.modifiers.ctrl) {
            if self.crop_mode_active {
                self.crop_mode_active = false;
                self.re_render_edits(frame);
            }
            self.current_view = AppView::Library;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::D) && !i.modifiers.command && !i.modifiers.ctrl) {
            self.current_view = AppView::Develop;
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
            self.open_file_dialog();
        }
        if ctx.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::O)) {
            self.import_folder_dialog();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::E)) {
            self.open_export_dialog();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Comma)) {
            self.show_settings = true;
        }

        // Milestone 13: Ratings (0-5)
        if self.app_config.shortcuts.number_keys_rate
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
        if self.app_config.shortcuts.arrow_keys_navigate
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
        if self.app_config.shortcuts.f_fullscreen
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
        if self.app_config.shortcuts.z_zoom
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
        if self.app_config.shortcuts.j_clipping
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
                            format!("🎞 {}", t!("develop_filmstrip"))
                        } else {
                            format!("🎞 {} (▲)", t!("develop_filmstrip"))
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

        match self.current_view {
            AppView::Library => {
                let action = show_library_view(ui, &mut self.library_state, &self.library_db);
                if !self.library_state.pending_thumbnail_requests.is_empty() {
                    let reqs = std::mem::take(&mut self.library_state.pending_thumbnail_requests);
                    self.queue_thumbnail_requests(reqs);
                }
                match action {
                    LibraryAction::OpenPhoto(path) => {
                        self.load_file_path(path);
                    }
                    LibraryAction::ImportFolder => {
                        self.import_folder_dialog();
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
                    LibraryAction::None => {}
                }
            }
            AppView::Develop => {
                // Left-side History Panel
                if self.show_history_panel && self.current_image.is_some() {
                    Panel::left("develop_history_panel")
                        .resizable(true)
                        .default_size(240.0)
                        .min_size(180.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.selectable_value(
                                    &mut self.left_panel_tab,
                                    LeftPanelTab::Presets,
                                    format!("🎨 {}", t!("develop_presets")),
                                );
                                ui.selectable_value(
                                    &mut self.left_panel_tab,
                                    LeftPanelTab::History,
                                    format!("📜 {}", t!("develop_history")),
                                );
                            });
                            ui.separator();

                            match self.left_panel_tab {
                                LeftPanelTab::Presets => {
                                    ui.horizontal(|ui| {
                                        if ui
                                            .button(format!("➕ {}", t!("preset_save_btn")))
                                            .clicked()
                                        {
                                            self.show_new_preset_dialog = true;
                                            self.new_preset_name = String::new();
                                            self.new_preset_mask = PresetGroupMask::default();
                                        }
                                        if ui
                                            .button(format!("📥 {}", t!("preset_import_btn")))
                                            .clicked()
                                        {
                                            self.import_preset_dialog();
                                        }
                                    });
                                    ui.add_space(4.0);

                                    let mut apply_preset = None;
                                    let mut delete_preset_id = None;
                                    let mut export_preset = None;
                                    let mut current_hovered_preset = None;

                                    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(
                                        ui,
                                        |ui| {
                                            // 1. Built-in Category
                                            ui.strong(t!("preset_builtin_category"));
                                            ui.add_space(2.0);
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

                                                let resp = ui
                                                    .selectable_label(false, &localized_name)
                                                    .on_hover_text(format!(
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

                                            // 2. User Presets Category
                                            ui.strong(t!("preset_user_category"));
                                            ui.add_space(2.0);
                                            if self.user_presets.is_empty() {
                                                ui.label(
                                                    RichText::new(t!("preset_empty_user"))
                                                        .weak()
                                                        .small(),
                                                );
                                            } else {
                                                for preset in &self.user_presets {
                                                    ui.horizontal(|ui| {
                                                        let resp = ui
                                                            .selectable_label(false, &preset.name)
                                                            .on_hover_text(t!(
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

                                                        ui.with_layout(
                                                            egui::Layout::right_to_left(
                                                                egui::Align::Center,
                                                            ),
                                                            |ui| {
                                                                if ui
                                                                    .small_button("🗑")
                                                                    .on_hover_text(t!(
                                                                        "preset_delete_btn"
                                                                    ))
                                                                    .clicked()
                                                                {
                                                                    delete_preset_id =
                                                                        Some(preset.id.clone());
                                                                }
                                                                if ui
                                                                    .small_button("📤")
                                                                    .on_hover_text(t!(
                                                                        "preset_export_btn"
                                                                    ))
                                                                    .clicked()
                                                                {
                                                                    export_preset =
                                                                        Some(preset.clone());
                                                                }
                                                            },
                                                        );
                                                    });
                                                }
                                            }
                                        },
                                    );

                                    // Handle hover preview
                                    if current_hovered_preset != self.hovered_preset {
                                        if let Some(p) = &current_hovered_preset {
                                            if self.stack_before_hover.is_none() {
                                                self.stack_before_hover =
                                                    Some(self.edit_stack.clone());
                                            }
                                            let mut preview_stack =
                                                self.stack_before_hover.as_ref().unwrap().clone();
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
                            }
                        });
                }

                // Right-side Develop Adjustment Panel
                let mut edits_changed = false;
                let mut action_label: Option<String> = None;
                let mut immediate_action = false;

                if self.show_develop_panel && self.current_image.is_some() {
                    Panel::right("develop_panel")
                        .resizable(true)
                        .default_size(320.0)
                        .min_size(280.0)
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false; 2])
                                .show(ui, |ui| {
                                    ui.heading(t!("view_develop"));
                                    ui.separator();

                                    // 1. Histogram Widget
                                    let mut clipping_changed = false;
                                    if let Some(hist_data) = &self.histogram_data {
                                        ui.group(|ui| {
                                            if show_histogram_with_clipping(
                                                ui,
                                                hist_data,
                                                &mut self.histogram_mode,
                                                &mut self.show_shadow_clipping,
                                                &mut self.show_highlight_clipping,
                                            ) {
                                                clipping_changed = true;
                                            }
                                        });
                                        ui.add_space(4.0);
                                    }
                                    if clipping_changed {
                                        self.re_render_edits(frame);
                                        ctx.request_repaint();
                                    }

                                    // 2. Crop & Straighten Section
                                    ui.group(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.strong(t!("develop_crop_tool"));
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
                                                        action_label = Some(t!("history_crop_reset"));
                                                    }

                                                    // Toggle Crop Tool button
                                                    let btn_text = if self.crop_mode_active {
                                                        format!("✔ {}", t!("develop_crop_done"))
                                                    } else {
                                                        format!("⛶ {}", t!("develop_crop_btn"))
                                                    };
                                                    let resp = ui.selectable_label(self.crop_mode_active, btn_text);
                                                    if resp.clicked() {
                                                        self.crop_mode_active = !self.crop_mode_active;
                                                        if self.crop_mode_active {
                                                            self.before_after_mode = BeforeAfterMode::Normal;
                                                        }
                                                        edits_changed = true;
                                                        immediate_action = false;
                                                    }
                                                },
                                            );
                                        });

                                        ui.add_space(4.0);

                                        // Aspect Preset Selector
                                        ui.horizontal(|ui| {
                                            ui.label(t!("develop_crop_aspect"));
                                            let current_preset = self.edit_stack.crop.preset;
                                            let mut selected_preset = current_preset;
                                            egui::ComboBox::from_id_salt("crop_aspect_combo")
                                                .selected_text(match current_preset {
                                                    CropAspectPreset::Original => t!("develop_crop_aspect_original"),
                                                    CropAspectPreset::Square => t!("develop_crop_aspect_1_1"),
                                                    CropAspectPreset::Ratio4x5 => t!("develop_crop_aspect_4_5"),
                                                    CropAspectPreset::Ratio3x2 => t!("develop_crop_aspect_3_2"),
                                                    CropAspectPreset::Ratio16x9 => t!("develop_crop_aspect_16_9"),
                                                    CropAspectPreset::Free => t!("develop_crop_aspect_free"),
                                                })
                                                .show_ui(ui, |ui| {
                                                    ui.selectable_value(&mut selected_preset, CropAspectPreset::Original, t!("develop_crop_aspect_original"));
                                                    ui.selectable_value(&mut selected_preset, CropAspectPreset::Square, t!("develop_crop_aspect_1_1"));
                                                    ui.selectable_value(&mut selected_preset, CropAspectPreset::Ratio4x5, t!("develop_crop_aspect_4_5"));
                                                    ui.selectable_value(&mut selected_preset, CropAspectPreset::Ratio3x2, t!("develop_crop_aspect_3_2"));
                                                    ui.selectable_value(&mut selected_preset, CropAspectPreset::Ratio16x9, t!("develop_crop_aspect_16_9"));
                                                    ui.selectable_value(&mut selected_preset, CropAspectPreset::Free, t!("develop_crop_aspect_free"));
                                                });

                                            if selected_preset != current_preset {
                                                self.edit_stack.crop.preset = selected_preset;
                                                if let Some(img) = &self.current_image {
                                                    let orig_aspect = if self.edit_stack.crop.rotation == 90
                                                        || self.edit_stack.crop.rotation == 270
                                                    {
                                                        img.height as f32 / img.width as f32
                                                    } else {
                                                        img.width as f32 / img.height as f32
                                                    };
                                                    let (x, y, w, h) = create_centered_crop_rect(orig_aspect, selected_preset);
                                                    self.edit_stack.crop.x = x;
                                                    self.edit_stack.crop.y = y;
                                                    self.edit_stack.crop.width = w;
                                                    self.edit_stack.crop.height = h;
                                                    if selected_preset != CropAspectPreset::Free && (w < 0.999 || h < 0.999) {
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

                                        // Rotate and Flip Buttons
                                        ui.horizontal(|ui| {
                                            if ui.button("↶ 90°").on_hover_text(t!("develop_crop_rotate_ccw")).clicked() {
                                                self.edit_stack.crop.rotation = (self.edit_stack.crop.rotation + 270) % 360;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_crop_rotate",
                                                    "angle" => format!("{}°", self.edit_stack.crop.rotation)
                                                ));
                                            }
                                            if ui.button("↷ 90°").on_hover_text(t!("develop_crop_rotate_cw")).clicked() {
                                                self.edit_stack.crop.rotation = (self.edit_stack.crop.rotation + 90) % 360;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_crop_rotate",
                                                    "angle" => format!("{}°", self.edit_stack.crop.rotation)
                                                ));
                                            }
                                            if ui.button("⇄ Flip H").on_hover_text(t!("develop_crop_flip_h")).clicked() {
                                                self.edit_stack.crop.flip_h = !self.edit_stack.crop.flip_h;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!("history_crop_flip_h"));
                                            }
                                            if ui.button("⇅ Flip V").on_hover_text(t!("develop_crop_flip_v")).clicked() {
                                                self.edit_stack.crop.flip_v = !self.edit_stack.crop.flip_v;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!("history_crop_flip_v"));
                                            }
                                        });

                                        // Straighten slider
                                        ui.horizontal(|ui| {
                                            ui.label(t!("develop_crop_straighten"));
                                            let resp = ui.add(
                                                Slider::new(
                                                    &mut self.edit_stack.crop.straighten,
                                                    -45.0..=45.0,
                                                )
                                                .suffix("°")
                                                .show_value(true),
                                            );
                                            if resp.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!(
                                                    "history_crop_straighten",
                                                    "val" => format!("{:+0.1}°", self.edit_stack.crop.straighten)
                                                ));
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_crop_straighten_reset"))
                                                .clicked()
                                            {
                                                self.edit_stack.crop.straighten = 0.0;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_reset_section",
                                                    "section" => t!("develop_crop_straighten")
                                                ));
                                            }
                                        });

                                        if self.crop_mode_active {
                                            ui.add_space(2.0);
                                            ui.label(RichText::new(t!("develop_crop_instruction")).weak().small());
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // 3. White Balance Section
                                    ui.group(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.strong(t!("develop_white_balance"));
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button("↺")
                                                        .on_hover_text(t!("develop_reset_wb"))
                                                        .clicked()
                                                    {
                                                        self.edit_stack.reset_white_balance();
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
                                                },
                                            );
                                        });

                                        ui.add_space(4.0);

                                        // Temperature slider
                                        ui.horizontal(|ui| {
                                            ui.label(t!("develop_temp_slider"));
                                            let resp = ui.add(
                                                Slider::new(
                                                    &mut self.edit_stack.white_balance.temperature,
                                                    -100.0..=100.0,
                                                )
                                                .show_value(true),
                                            );
                                            if resp.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!(
                                                    "history_temperature",
                                                    "val" => format!("{:+0.0}", self.edit_stack.white_balance.temperature)
                                                ));
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_temp"))
                                                .clicked()
                                            {
                                                self.edit_stack.white_balance.temperature = 0.0;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_reset_section",
                                                    "section" => t!("develop_wb_temp")
                                                ));
                                            }
                                        });

                                        // Tint slider
                                        ui.horizontal(|ui| {
                                            ui.label(t!("develop_tint_slider"));
                                            let resp = ui.add(
                                                Slider::new(
                                                    &mut self.edit_stack.white_balance.tint,
                                                    -100.0..=100.0,
                                                )
                                                .show_value(true),
                                            );
                                            if resp.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!(
                                                    "history_tint",
                                                    "val" => format!("{:+0.0}", self.edit_stack.white_balance.tint)
                                                ));
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_tint"))
                                                .clicked()
                                            {
                                                self.edit_stack.white_balance.tint = 0.0;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_reset_section",
                                                    "section" => t!("develop_wb_tint")
                                                ));
                                            }
                                        });
                                    });

                                    ui.add_space(6.0);

                                    // 3. Exposure Section
                                    ui.group(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.strong(t!("develop_exposure"));
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

                                        ui.add_space(4.0);

                                        // Exposure EV slider
                                        ui.horizontal(|ui| {
                                            ui.label(t!("develop_ev_slider"));
                                            let resp = ui.add(
                                                Slider::new(
                                                    &mut self.edit_stack.exposure.ev,
                                                    -5.00..=5.00,
                                                )
                                                .step_by(0.05)
                                                .custom_formatter(|v, _| format!("{:+.2} EV", v)),
                                            );
                                            if resp.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!(
                                                    "history_exposure",
                                                    "val" => format!("{:+0.2}", self.edit_stack.exposure.ev)
                                                ));
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_exposure"))
                                                .clicked()
                                            {
                                                self.edit_stack.exposure.ev = 0.0;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_reset_section",
                                                    "section" => t!("develop_exposure")
                                                ));
                                            }
                                        });
                                    });

                                    ui.add_space(6.0);

                                    // 4. Highlights & Shadows Section
                                    ui.group(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.strong(format!(
                                                "{} & {}",
                                                t!("develop_highlights"),
                                                t!("develop_shadows")
                                            ));
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
                                                            "section" => format!(
                                                                "{} & {}",
                                                                t!("develop_highlights"),
                                                                t!("develop_shadows")
                                                            )
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
                                                            "section" => format!(
                                                                "{} & {}",
                                                                t!("develop_highlights"),
                                                                t!("develop_shadows")
                                                            )
                                                        ));
                                                    }
                                                },
                                            );
                                        });

                                        ui.add_space(4.0);

                                        // Highlights slider
                                        ui.horizontal(|ui| {
                                            ui.label(t!("develop_highlights_slider"));
                                            let resp = ui.add(
                                                Slider::new(
                                                    &mut self
                                                        .edit_stack
                                                        .highlights_shadows
                                                        .highlights,
                                                    -100.0..=100.0,
                                                )
                                                .show_value(true),
                                            );
                                            if resp.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!(
                                                    "history_highlights",
                                                    "val" => format!("{:+0.0}", self.edit_stack.highlights_shadows.highlights)
                                                ));
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_highlights"))
                                                .clicked()
                                            {
                                                self.edit_stack.highlights_shadows.highlights = 0.0;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_reset_section",
                                                    "section" => t!("develop_highlights")
                                                ));
                                            }
                                        });

                                        // Shadows slider
                                        ui.horizontal(|ui| {
                                            ui.label(t!("develop_shadows_slider"));
                                            let resp = ui.add(
                                                Slider::new(
                                                    &mut self.edit_stack.highlights_shadows.shadows,
                                                    -100.0..=100.0,
                                                )
                                                .show_value(true),
                                            );
                                            if resp.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!(
                                                    "history_shadows",
                                                    "val" => format!("{:+0.0}", self.edit_stack.highlights_shadows.shadows)
                                                ));
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_shadows"))
                                                .clicked()
                                            {
                                                self.edit_stack.highlights_shadows.shadows = 0.0;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_reset_section",
                                                    "section" => t!("develop_shadows")
                                                ));
                                            }
                                        });
                                    });

                                    ui.add_space(6.0);

                                    // 5. Contrast Section
                                    ui.group(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.strong(t!("develop_contrast"));
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

                                        ui.add_space(4.0);

                                        // Contrast factor slider
                                        ui.horizontal(|ui| {
                                            ui.label(t!("develop_contrast_slider"));
                                            let resp = ui.add(
                                                Slider::new(
                                                    &mut self.edit_stack.contrast.contrast,
                                                    -100.0..=100.0,
                                                )
                                                .show_value(true),
                                            );
                                            if resp.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!(
                                                    "history_contrast",
                                                    "val" => format!("{:+0.0}", self.edit_stack.contrast.contrast)
                                                ));
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_contrast_val"))
                                                .clicked()
                                            {
                                                self.edit_stack.contrast.contrast = 0.0;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_reset_section",
                                                    "section" => t!("develop_contrast")
                                                ));
                                            }
                                        });
                                    });

                                    ui.add_space(6.0);

                                    // 6. Tone Curve Section
                                    ui.group(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.strong(t!("develop_tone_curve"));
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

                                        if show_tone_curve(
                                            ui,
                                            &mut self.edit_stack.tone_curve.points,
                                        ) {
                                            edits_changed = true;
                                            action_label = Some(t!("history_tone_curve"));
                                        }

                                        ui.add_space(2.0);
                                        ui.label(
                                            RichText::new(t!("develop_curve_instruction"))
                                                .weak()
                                                .size(10.5),
                                        );
                                    });

                                    ui.add_space(6.0);

                                    // 7. HSL / Color Section (8 Color Bands)
                                    ui.group(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.strong(t!("develop_hsl"));
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

                                        ui.add_space(4.0);

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

                                        // 8 Color Band selector
                                        ui.horizontal_wrapped(|ui| {
                                            for (i, (&key, &col)) in
                                                BAND_KEYS.iter().zip(BAND_COLORS.iter()).enumerate()
                                            {
                                                let is_selected = self.hsl_selected_band == i;
                                                let text = RichText::new(format!("● {}", t!(key)))
                                                    .color(col);
                                                if ui.selectable_label(is_selected, text).clicked()
                                                {
                                                    self.hsl_selected_band = i;
                                                }
                                            }
                                        });

                                        ui.add_space(4.0);

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
                                        ui.horizontal(|ui| {
                                            ui.label(t!("develop_hue_slider"));
                                            let resp = ui.add(
                                                Slider::new(&mut band.hue, -100.0..=100.0)
                                                    .show_value(true),
                                            );
                                            if resp.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!(
                                                    "history_hsl_band",
                                                    "band" => &localized_band_name
                                                ));
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_hue"))
                                                .clicked()
                                            {
                                                band.hue = 0.0;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_reset_section",
                                                    "section" => format!("{}: {}", &localized_band_name, t!("develop_hsl_hue"))
                                                ));
                                            }
                                        });

                                        // Saturation
                                        ui.horizontal(|ui| {
                                            ui.label(t!("develop_sat_slider"));
                                            let resp = ui.add(
                                                Slider::new(&mut band.saturation, -100.0..=100.0)
                                                    .show_value(true),
                                            );
                                            if resp.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!(
                                                    "history_hsl_band",
                                                    "band" => &localized_band_name
                                                ));
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_sat"))
                                                .clicked()
                                            {
                                                band.saturation = 0.0;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_reset_section",
                                                    "section" => format!("{}: {}", &localized_band_name, t!("develop_hsl_saturation"))
                                                ));
                                            }
                                        });

                                        // Luminance
                                        ui.horizontal(|ui| {
                                            ui.label(t!("develop_lum_slider"));
                                            let resp = ui.add(
                                                Slider::new(&mut band.luminance, -100.0..=100.0)
                                                    .show_value(true),
                                            );
                                            if resp.changed() {
                                                edits_changed = true;
                                                action_label = Some(t!(
                                                    "history_hsl_band",
                                                    "band" => &localized_band_name
                                                ));
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_lum"))
                                                .clicked()
                                            {
                                                band.luminance = 0.0;
                                                edits_changed = true;
                                                immediate_action = true;
                                                action_label = Some(t!(
                                                    "history_reset_section",
                                                    "section" => format!("{}: {}", &localized_band_name, t!("develop_hsl_luminance"))
                                                ));
                                            }
                                        });
                                    });

                                    ui.add_space(16.0);

                                    // 8. Action Buttons
                                    ui.horizontal(|ui| {
                                        if ui
                                            .button(
                                                RichText::new(format!(
                                                    "↺ {}",
                                                    t!("develop_reset_all")
                                                ))
                                                .color(Color32::LIGHT_RED),
                                            )
                                            .on_hover_text(t!("develop_reset_all_tooltip"))
                                            .clicked()
                                        {
                                            self.edit_stack.reset_all();
                                            edits_changed = true;
                                            immediate_action = true;
                                            action_label = Some(t!("history_reset_all"));
                                        }

                                        if ui
                                            .button(format!("💾 {}", t!("develop_save_sidecar")))
                                            .clicked()
                                        {
                                            self.save_sidecar();
                                        }

                                        if ui
                                            .button(
                                                RichText::new(format!(
                                                    "📤 {}",
                                                    t!("develop_export_btn")
                                                ))
                                                .strong(),
                                            )
                                            .clicked()
                                        {
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
                                ui.colored_label(
                                    Color32::RED,
                                    format!("⚠ {}", t!("develop_error_title")),
                                );
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
                        // Before / After mode and Zoom toolbar
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(t!("develop_before_after")).strong());
                            if ui
                                .selectable_label(
                                    self.before_after_mode == BeforeAfterMode::Normal,
                                    t!("develop_mode_normal"),
                                )
                                .clicked()
                            {
                                self.before_after_mode = BeforeAfterMode::Normal;
                            }
                            if ui
                                .selectable_label(
                                    self.before_after_mode == BeforeAfterMode::Toggle,
                                    t!("develop_mode_toggle"),
                                )
                                .on_hover_text(t!("develop_before_toggle_tooltip"))
                                .clicked()
                            {
                                if self.crop_mode_active {
                                    self.crop_mode_active = false;
                                    self.re_render_edits(frame);
                                }
                                self.before_after_mode = BeforeAfterMode::Toggle;
                            }
                            if ui
                                .selectable_label(
                                    self.before_after_mode == BeforeAfterMode::SplitView,
                                    t!("develop_mode_split"),
                                )
                                .on_hover_text(t!("develop_split_tooltip"))
                                .clicked()
                            {
                                if self.crop_mode_active {
                                    self.crop_mode_active = false;
                                    self.re_render_edits(frame);
                                }
                                self.before_after_mode = BeforeAfterMode::SplitView;
                            }
                            if ui
                                .selectable_label(
                                    self.before_after_mode == BeforeAfterMode::SideBySide,
                                    t!("develop_mode_side_by_side"),
                                )
                                .on_hover_text(t!("develop_side_by_side_tooltip"))
                                .clicked()
                            {
                                if self.crop_mode_active {
                                    self.crop_mode_active = false;
                                    self.re_render_edits(frame);
                                }
                                self.before_after_mode = BeforeAfterMode::SideBySide;
                            }

                            ui.separator();
                            let is_fit =
                                !self.is_one_to_one && (self.zoom_level - 1.0).abs() < 0.01;
                            if ui
                                .selectable_label(is_fit, t!("develop_zoom_fit"))
                                .clicked()
                            {
                                self.is_one_to_one = false;
                                self.zoom_level = 1.0;
                                self.pan_offset = Vec2::ZERO;
                            }
                            if ui
                                .selectable_label(self.is_one_to_one, t!("develop_zoom_100"))
                                .clicked()
                            {
                                self.is_one_to_one = true;
                                self.ensure_full_res_rendered(frame);
                            }
                            let current_zoom_pct = (self.zoom_level * 100.0).round() as i32;
                            ui.label(t!("develop_zoom_pct", "zoom" => current_zoom_pct));
                        });
                        ui.separator();

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
                                ui.add_space(available_size.y * 0.3);
                                ui.heading(RichText::new(APP_NAME).size(32.0));
                                ui.label(t!("develop_empty_desc"));
                                ui.add_space(20.0);
                                if ui
                                    .button(
                                        RichText::new(format!("📂  {}", t!("develop_open_btn")))
                                            .size(16.0),
                                    )
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
                        ui.heading(RichText::new(APP_NAME).size(26.0).strong());
                        ui.label(t!("about_version"));
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
        if self.export_targets.is_empty() {
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
            format: if self.export_format_is_png {
                beam_media::ExportFormat::Png
            } else {
                beam_media::ExportFormat::Jpeg {
                    quality: self.export_jpeg_quality,
                }
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
        };
        let targets = self.export_targets.clone();

        thread::spawn(move || {
            let cancel_clone = cancel_flag.clone();
            let tx_prog = tx.clone();
            let results =
                beam_media::export_batch(&targets, &config, &cancel_clone, move |progress| {
                    let _ = tx_prog.send(BackgroundMsg::ExportProgress(progress));
                });

            let cancelled = cancel_flag.load(std::sync::atomic::Ordering::Relaxed);
            let success_count = results.as_ref().map(|r| r.len()).unwrap_or(0);
            let _ = tx.send(BackgroundMsg::ExportFinished {
                success_count,
                total: targets.len(),
                cancelled,
            });
        });
    }

    /// Render export settings configuration modal
    fn show_export_dialog_window(&mut self, ctx: &egui::Context) {
        if !self.show_export_dialog {
            return;
        }

        let mut start_requested = false;
        let mut cancel_requested = false;

        egui::Window::new(t!("export_settings_title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(460.0);
                egui::ScrollArea::vertical()
                    .max_height(600.0)
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.heading(t!("export_title"));
                            ui.separator();

                            let count = self.export_targets.len();
                            ui.label(
                                RichText::new(t!("export_photos_to_export", "count" => count))
                                    .strong(),
                            );
                            ui.add_space(8.0);

                            // Destination folder
                            ui.label(t!("export_destination_folder"));
                            ui.horizontal(|ui| {
                                let path_str = self.export_output_dir.display().to_string();
                                let mut text = path_str.as_str();
                                ui.add(
                                    egui::TextEdit::singleline(&mut text)
                                        .desired_width(330.0)
                                        .interactive(false),
                                );
                                if ui.button(t!("export_browse")).clicked() {
                                    if let Some(folder) = rfd::FileDialog::new()
                                        .set_title(t!("export_choose_dest"))
                                        .set_directory(&self.export_output_dir)
                                        .pick_folder()
                                    {
                                        self.export_output_dir = folder;
                                    }
                                }
                            });
                            ui.add_space(8.0);

                            // Format & Quality
                            ui.label(t!("export_format"));
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut self.export_format_is_png, false, "JPEG");
                                ui.radio_value(&mut self.export_format_is_png, true, "PNG");
                            });

                            if !self.export_format_is_png {
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label(t!("export_jpeg_quality"));
                                    ui.add(
                                        Slider::new(&mut self.export_jpeg_quality, 1..=100)
                                            .text("%"),
                                    );
                                });
                            }
                            ui.add_space(8.0);

                            // Color Space
                            ui.label(t!("export_colorspace"));
                            egui::ComboBox::from_id_salt("export_colorspace_combo")
                                .selected_text(t!(self.export_color_space.translation_key()))
                                .show_ui(ui, |ui| {
                                    for cs in [
                                        beam_core::ExportColorSpace::Srgb,
                                        beam_core::ExportColorSpace::DisplayP3,
                                        beam_core::ExportColorSpace::AdobeRgb,
                                    ] {
                                        ui.selectable_value(
                                            &mut self.export_color_space,
                                            cs,
                                            t!(cs.translation_key()),
                                        );
                                    }
                                });
                            ui.add_space(8.0);

                            // Resize
                            ui.label(t!("export_resize_mode"));
                            let resize_label = match self.export_resize_mode_index {
                                1 => t!("export_resize_long_edge"),
                                2 => t!("export_resize_percentage"),
                                3 => t!("export_resize_exact"),
                                _ => t!("export_resize_original"),
                            };
                            egui::ComboBox::from_id_salt("export_resize_combo")
                                .selected_text(resize_label)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.export_resize_mode_index,
                                        0,
                                        t!("export_resize_original"),
                                    );
                                    ui.selectable_value(
                                        &mut self.export_resize_mode_index,
                                        1,
                                        t!("export_resize_long_edge"),
                                    );
                                    ui.selectable_value(
                                        &mut self.export_resize_mode_index,
                                        2,
                                        t!("export_resize_percentage"),
                                    );
                                    ui.selectable_value(
                                        &mut self.export_resize_mode_index,
                                        3,
                                        t!("export_resize_exact"),
                                    );
                                });

                            match self.export_resize_mode_index {
                                1 => {
                                    ui.horizontal(|ui| {
                                        ui.label(t!("export_resize_long_edge"));
                                        ui.add(
                                            egui::DragValue::new(&mut self.export_resize_long_edge)
                                                .range(64..=16384)
                                                .suffix(" px"),
                                        );
                                    });
                                }
                                2 => {
                                    ui.horizontal(|ui| {
                                        ui.label(t!("export_resize_percentage"));
                                        ui.add(
                                            Slider::new(
                                                &mut self.export_resize_percentage,
                                                5.0..=200.0,
                                            )
                                            .suffix("%"),
                                        );
                                    });
                                }
                                3 => {
                                    ui.horizontal(|ui| {
                                        ui.label(t!("export_resize_width"));
                                        ui.add(
                                            egui::DragValue::new(&mut self.export_resize_width)
                                                .range(16..=16384)
                                                .suffix(" px"),
                                        );
                                        ui.label(t!("export_resize_height"));
                                        ui.add(
                                            egui::DragValue::new(&mut self.export_resize_height)
                                                .range(16..=16384)
                                                .suffix(" px"),
                                        );
                                    });
                                    ui.checkbox(
                                        &mut self.export_resize_lock_aspect,
                                        t!("export_resize_lock_aspect"),
                                    );
                                }
                                _ => {}
                            }
                            ui.add_space(8.0);

                            // Output Sharpening
                            ui.label(t!("export_sharpening"));
                            let sharpening_label = match self.export_sharpening_index {
                                1 => t!("export_sharpening_screen"),
                                2 => t!("export_sharpening_print"),
                                _ => t!("export_sharpening_none"),
                            };
                            egui::ComboBox::from_id_salt("export_sharpening_combo")
                                .selected_text(sharpening_label)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.export_sharpening_index,
                                        0,
                                        t!("export_sharpening_none"),
                                    );
                                    ui.selectable_value(
                                        &mut self.export_sharpening_index,
                                        1,
                                        t!("export_sharpening_screen"),
                                    );
                                    ui.selectable_value(
                                        &mut self.export_sharpening_index,
                                        2,
                                        t!("export_sharpening_print"),
                                    );
                                });
                            ui.add_space(8.0);

                            // Metadata (EXIF)
                            ui.checkbox(&mut self.export_include_exif, t!("export_include_exif"))
                                .on_hover_text(t!("export_include_exif_tooltip"));
                            ui.add_space(8.0);

                            // File Naming Pattern & Suffix
                            ui.label(t!("export_template_label"));
                            ui.text_edit_singleline(&mut self.export_template);
                            ui.label(RichText::new(t!("export_template_hint")).weak().size(11.0));

                            ui.horizontal(|ui| {
                                ui.label(t!("export_filename_suffix"));
                                ui.text_edit_singleline(&mut self.export_suffix);
                            });
                            ui.label(RichText::new(t!("export_suffix_example")).weak().size(11.0));

                            // Live preview
                            let sample_name = self
                                .export_targets
                                .first()
                                .and_then(|(p, _)| p.file_stem().and_then(|s| s.to_str()))
                                .unwrap_or("sample_photo");
                            let mut preview = beam_media::expand_filename_template(
                                &self.export_template,
                                sample_name,
                                Some("2026-10-28"),
                                1,
                                5,
                            );
                            if !self.export_suffix.trim().is_empty() {
                                preview.push_str(&self.export_suffix);
                            }
                            let ext = if self.export_format_is_png {
                                "png"
                            } else {
                                "jpg"
                            };
                            preview = format!("{preview}.{ext}");
                            ui.label(
                                RichText::new(format!(
                                    "{} {}",
                                    t!("export_template_preview"),
                                    preview
                                ))
                                .color(Color32::from_rgb(140, 200, 255))
                                .size(12.0),
                            );

                            ui.add_space(16.0);
                            ui.separator();

                            // Dialog buttons
                            ui.horizontal(|ui| {
                                if ui
                                    .button(
                                        RichText::new(t!("export_start_btn")).strong().size(14.0),
                                    )
                                    .clicked()
                                {
                                    start_requested = true;
                                }
                                if ui.button(t!("export_cancel")).clicked() {
                                    cancel_requested = true;
                                }
                            });
                        });
                    });
            });

        if start_requested {
            self.start_export();
        }
        if cancel_requested {
            self.show_export_dialog = false;
        }
    }

    /// Render export progress modal with real-time status and cancel button
    fn show_export_progress_window(&mut self, ctx: &egui::Context) {
        if !self.show_export_progress {
            return;
        }

        let mut close_requested = false;

        egui::Window::new(t!("export_in_progress"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(380.0);
                ui.vertical(|ui| {
                    if self.export_is_running {
                        ui.heading(t!("export_in_progress"));
                    } else if let Some(msg) = &self.export_status_message {
                        ui.heading(msg);
                    } else {
                        ui.heading(t!("export_finished"));
                    }
                    ui.separator();
                    ui.add_space(8.0);

                    if let Some(prog) = &self.export_progress_data {
                        let frac = if prog.total > 0 {
                            prog.current_index as f32 / prog.total as f32
                        } else {
                            0.0
                        };
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .show_percentage()
                                .animate(self.export_is_running),
                        );
                        ui.add_space(4.0);

                        let fname = prog
                            .current_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("photo");
                        ui.label(t!(
                            "export_processing",
                            "current" => prog.current_index,
                            "total" => prog.total,
                            "name" => fname
                        ));

                        if let Some(err) = &prog.error {
                            ui.colored_label(
                                Color32::from_rgb(255, 100, 100),
                                format!("{}: {err}", t!("develop_error_title")),
                            );
                        }
                    } else if self.export_is_running {
                        ui.add(egui::ProgressBar::new(0.0).animate(true));
                        ui.label(t!("export_starting"));
                    }

                    ui.add_space(16.0);
                    ui.separator();

                    ui.horizontal(|ui| {
                        if self.export_is_running {
                            if ui
                                .button(
                                    RichText::new(t!("export_cancel_btn"))
                                        .color(Color32::LIGHT_RED),
                                )
                                .clicked()
                            {
                                if let Some(flag) = &self.export_cancel_flag {
                                    flag.store(true, std::sync::atomic::Ordering::Relaxed);
                                }
                            }
                        } else if ui
                            .button(RichText::new(t!("settings_close")).strong())
                            .clicked()
                        {
                            close_requested = true;
                        }
                    });
                });
            });

        if close_requested {
            self.show_export_progress = false;
        }
    }

    /// Render settings modal with language switcher, catalog storage info, and keyboard shortcuts
    fn show_settings_dialog_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }

        let mut close_requested = false;

        egui::Window::new(t!("settings_title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.settings_tab,
                            SettingsTab::General,
                            t!("settings_tab_general"),
                        );
                        ui.selectable_value(
                            &mut self.settings_tab,
                            SettingsTab::Shortcuts,
                            t!("settings_tab_shortcuts"),
                        );
                    });
                    ui.separator();
                    ui.add_space(4.0);

                    match self.settings_tab {
                        SettingsTab::General => {
                            ui.heading(t!("settings_general"));
                            ui.separator();
                            ui.add_space(4.0);

                            ui.horizontal(|ui| {
                                ui.label(t!("settings_language"));
                                let mut current_lang = self.app_config.language;
                                let prev_lang = current_lang;
                                egui::ComboBox::from_id_salt("settings_lang_combo")
                                    .selected_text(current_lang.display_name())
                                    .show_ui(ui, |ui| {
                                        for lang in [Language::En, Language::Tr] {
                                            ui.selectable_value(
                                                &mut current_lang,
                                                lang,
                                                lang.display_name(),
                                            );
                                        }
                                    });

                                if current_lang != prev_lang {
                                    self.app_config.language = current_lang;
                                    beam_core::set_language(current_lang);
                                    if let Err(e) = self.app_config.save() {
                                        log::warn!("Failed to save app config: {e}");
                                    }
                                    ctx.request_repaint();
                                }
                            });

                            ui.add_space(12.0);
                            ui.heading(t!("settings_storage"));
                            ui.separator();
                            ui.add_space(4.0);

                            ui.label(RichText::new(t!("settings_catalog_path")).strong());
                            let mut db_path = default_catalog_path().display().to_string();
                            ui.add(
                                egui::TextEdit::singleline(&mut db_path)
                                    .desired_width(380.0)
                                    .interactive(false),
                            );

                            ui.add_space(4.0);
                            ui.label(RichText::new(t!("settings_cache_path")).strong());
                            let mut cache_path = self.thumbnail_cache_dir.display().to_string();
                            ui.add(
                                egui::TextEdit::singleline(&mut cache_path)
                                    .desired_width(380.0)
                                    .interactive(false),
                            );
                        }
                        SettingsTab::Shortcuts => {
                            ui.heading(t!("settings_shortcuts_heading"));
                            ui.separator();
                            ui.add_space(4.0);

                            ui.horizontal(|ui| {
                                ui.label(t!("settings_shortcut_profile"));
                                let prev_prof = self.app_config.shortcuts.profile;
                                let mut curr_prof = prev_prof;
                                egui::ComboBox::from_id_salt("shortcut_profile_combo")
                                    .selected_text(curr_prof.display_name())
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut curr_prof,
                                            ShortcutProfile::Default,
                                            ShortcutProfile::Default.display_name(),
                                        );
                                        ui.selectable_value(
                                            &mut curr_prof,
                                            ShortcutProfile::Lightroom,
                                            ShortcutProfile::Lightroom.display_name(),
                                        );
                                    });
                                if curr_prof != prev_prof {
                                    self.app_config.shortcuts.profile = curr_prof;
                                    let _ = self.app_config.save();
                                }
                            });

                            ui.add_space(8.0);
                            let mut shortcuts_changed = false;
                            if ui
                                .checkbox(
                                    &mut self.app_config.shortcuts.number_keys_rate,
                                    t!("settings_shortcut_ratings"),
                                )
                                .changed()
                            {
                                shortcuts_changed = true;
                            }
                            if ui
                                .checkbox(
                                    &mut self.app_config.shortcuts.number_keys_rate,
                                    t!("settings_shortcut_labels"),
                                )
                                .changed()
                            {
                                shortcuts_changed = true;
                            }
                            if ui
                                .checkbox(
                                    &mut self.app_config.shortcuts.arrow_keys_navigate,
                                    t!("settings_shortcut_nav"),
                                )
                                .changed()
                            {
                                shortcuts_changed = true;
                            }
                            if ui
                                .checkbox(
                                    &mut self.app_config.shortcuts.f_fullscreen,
                                    t!("settings_shortcut_fullscreen"),
                                )
                                .changed()
                            {
                                shortcuts_changed = true;
                            }
                            if ui
                                .checkbox(
                                    &mut self.app_config.shortcuts.z_zoom,
                                    t!("settings_shortcut_zoom"),
                                )
                                .changed()
                            {
                                shortcuts_changed = true;
                            }
                            if ui
                                .checkbox(
                                    &mut self.app_config.shortcuts.j_clipping,
                                    t!("settings_shortcut_clipping"),
                                )
                                .changed()
                            {
                                shortcuts_changed = true;
                            }

                            if shortcuts_changed {
                                let _ = self.app_config.save();
                            }
                        }
                    }

                    ui.add_space(16.0);
                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new(t!("settings_close")).strong())
                            .clicked()
                        {
                            close_requested = true;
                        }
                    });
                });
            });

        if close_requested {
            self.show_settings = false;
        }
    }

    /// Render Save New Preset dialog window
    fn show_new_preset_dialog_window(&mut self, ctx: &egui::Context) {
        if !self.show_new_preset_dialog {
            return;
        }

        let mut close_dialog = false;
        egui::Window::new(t!("preset_dialog_title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t!("preset_dialog_name"));
                    ui.text_edit_singleline(&mut self.new_preset_name);
                });
                ui.separator();
                ui.strong(t!("preset_dialog_include_groups"));

                ui.horizontal(|ui| {
                    if ui.small_button(t!("preset_dialog_check_all")).clicked() {
                        self.new_preset_mask = PresetGroupMask::all();
                    }
                    if ui.small_button(t!("preset_dialog_check_none")).clicked() {
                        self.new_preset_mask = PresetGroupMask::none();
                    }
                });
                ui.add_space(4.0);

                ui.checkbox(
                    &mut self.new_preset_mask.white_balance,
                    t!("preset_dialog_group_wb"),
                );
                ui.checkbox(
                    &mut self.new_preset_mask.exposure,
                    t!("preset_dialog_group_exposure"),
                );
                ui.checkbox(
                    &mut self.new_preset_mask.highlights_shadows,
                    t!("preset_dialog_group_highlights_shadows"),
                );
                ui.checkbox(&mut self.new_preset_mask.contrast, t!("develop_contrast"));
                ui.checkbox(
                    &mut self.new_preset_mask.tone_curve,
                    t!("preset_dialog_group_tone_curve"),
                );
                ui.checkbox(&mut self.new_preset_mask.hsl, t!("preset_dialog_group_hsl"));
                ui.checkbox(
                    &mut self.new_preset_mask.crop,
                    t!("preset_dialog_group_crop"),
                );

                ui.separator();
                ui.horizontal(|ui| {
                    let can_save =
                        !self.new_preset_name.trim().is_empty() && self.new_preset_mask.has_any();
                    if ui
                        .add_enabled(
                            can_save,
                            egui::Button::new(RichText::new(t!("preset_dialog_save")).strong()),
                        )
                        .clicked()
                    {
                        let name = self.new_preset_name.trim().to_string();
                        let id = name.to_lowercase().replace(' ', "_");
                        let preset =
                            Preset::from_stack(id, name, &self.edit_stack, self.new_preset_mask);
                        let _ = save_user_preset(&preset, &self.preset_dir);
                        self.user_presets = load_user_presets(&self.preset_dir);
                        close_dialog = true;
                    }
                    if ui.button(t!("preset_dialog_cancel")).clicked() {
                        close_dialog = true;
                    }
                });
            });

        if close_dialog {
            self.show_new_preset_dialog = false;
        }
    }

    /// Render Copy Settings dialog window
    fn show_copy_dialog_window(&mut self, ctx: &egui::Context) {
        if !self.show_copy_dialog {
            return;
        }

        let mut close_dialog = false;
        egui::Window::new(t!("copy_dialog_title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.strong(t!("preset_dialog_include_groups"));

                ui.horizontal(|ui| {
                    if ui.small_button(t!("preset_dialog_check_all")).clicked() {
                        self.copy_dialog_mask = PresetGroupMask::all();
                    }
                    if ui.small_button(t!("preset_dialog_check_none")).clicked() {
                        self.copy_dialog_mask = PresetGroupMask::none();
                    }
                });
                ui.add_space(4.0);

                ui.checkbox(
                    &mut self.copy_dialog_mask.white_balance,
                    t!("preset_dialog_group_wb"),
                );
                ui.checkbox(
                    &mut self.copy_dialog_mask.exposure,
                    t!("preset_dialog_group_exposure"),
                );
                ui.checkbox(
                    &mut self.copy_dialog_mask.highlights_shadows,
                    t!("preset_dialog_group_highlights_shadows"),
                );
                ui.checkbox(&mut self.copy_dialog_mask.contrast, t!("develop_contrast"));
                ui.checkbox(
                    &mut self.copy_dialog_mask.tone_curve,
                    t!("preset_dialog_group_tone_curve"),
                );
                ui.checkbox(
                    &mut self.copy_dialog_mask.hsl,
                    t!("preset_dialog_group_hsl"),
                );
                ui.checkbox(
                    &mut self.copy_dialog_mask.crop,
                    t!("preset_dialog_group_crop"),
                );

                ui.separator();
                ui.horizontal(|ui| {
                    let can_copy = self.copy_dialog_mask.has_any();
                    if ui
                        .add_enabled(
                            can_copy,
                            egui::Button::new(RichText::new(t!("copy_dialog_copy_btn")).strong()),
                        )
                        .clicked()
                    {
                        self.copied_settings =
                            Some((self.edit_stack.clone(), self.copy_dialog_mask));
                        close_dialog = true;
                    }
                    if ui.button(t!("preset_dialog_cancel")).clicked() {
                        close_dialog = true;
                    }
                });
            });

        if close_dialog {
            self.show_copy_dialog = false;
        }
    }

    /// Render Multi-Photo Settings Synchronization dialog window
    fn show_sync_dialog_window(&mut self, ctx: &egui::Context) {
        if !self.show_sync_dialog {
            return;
        }

        let mut close_dialog = false;
        let mut do_sync = false;

        let Some((source_id, source_name, source_stack)) = self.sync_dialog_source_photo.clone()
        else {
            self.show_sync_dialog = false;
            return;
        };

        let target_count = self
            .library_state
            .selected_photo_ids
            .iter()
            .filter(|&&id| id != source_id)
            .count();

        egui::Window::new(t!("sync_dialog_title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(t!("sync_dialog_source", "name" => &source_name));
                ui.label(t!("sync_dialog_target_count", "count" => target_count));
                ui.separator();
                ui.strong(t!("preset_dialog_include_groups"));

                ui.horizontal(|ui| {
                    if ui.small_button(t!("preset_dialog_check_all")).clicked() {
                        self.sync_dialog_mask = PresetGroupMask::all();
                    }
                    if ui.small_button(t!("preset_dialog_check_none")).clicked() {
                        self.sync_dialog_mask = PresetGroupMask::none();
                    }
                });
                ui.add_space(4.0);

                ui.checkbox(
                    &mut self.sync_dialog_mask.white_balance,
                    t!("preset_dialog_group_wb"),
                );
                ui.checkbox(
                    &mut self.sync_dialog_mask.exposure,
                    t!("preset_dialog_group_exposure"),
                );
                ui.checkbox(
                    &mut self.sync_dialog_mask.highlights_shadows,
                    t!("preset_dialog_group_highlights_shadows"),
                );
                ui.checkbox(&mut self.sync_dialog_mask.contrast, t!("develop_contrast"));
                ui.checkbox(
                    &mut self.sync_dialog_mask.tone_curve,
                    t!("preset_dialog_group_tone_curve"),
                );
                ui.checkbox(
                    &mut self.sync_dialog_mask.hsl,
                    t!("preset_dialog_group_hsl"),
                );
                ui.checkbox(
                    &mut self.sync_dialog_mask.crop,
                    t!("preset_dialog_group_crop"),
                );

                ui.separator();
                ui.horizontal(|ui| {
                    let can_sync = target_count > 0 && self.sync_dialog_mask.has_any();
                    if ui
                        .add_enabled(
                            can_sync,
                            egui::Button::new(RichText::new(t!("sync_dialog_sync_btn")).strong()),
                        )
                        .clicked()
                    {
                        do_sync = true;
                        close_dialog = true;
                    }
                    if ui.button(t!("sync_dialog_cancel")).clicked() {
                        close_dialog = true;
                    }
                });
            });

        if do_sync {
            let preset = Preset::from_stack(
                "sync",
                "Synchronized Settings",
                &source_stack,
                self.sync_dialog_mask,
            );
            let mut sync_success_count = 0;
            for &target_id in &self.library_state.selected_photo_ids {
                if target_id == source_id {
                    continue;
                }
                if let Ok(Some(photo)) = self.library_db.get_photo(target_id) {
                    let path = PathBuf::from(&photo.file_path);
                    let sidecar_p = Sidecar::path_for(&path);
                    let mut sidecar = if let Ok(loaded) = Sidecar::load_from_file(&sidecar_p) {
                        loaded
                    } else {
                        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
                        Sidecar::new(fname)
                    };

                    preset.apply_to(&mut sidecar.edit_stack);
                    sidecar.history.push(HistoryStep::new(
                        t!("history_paste_settings"),
                        sidecar.edit_stack.clone(),
                    ));
                    sidecar.history_index = sidecar.history.len() - 1;

                    if sidecar.save_to_file(&sidecar_p).is_ok() {
                        self.library_state.thumbnail_status.remove(&target_id);
                        let new_thumb_path =
                            beam_media::thumbnail_cache_path(&path, &self.thumbnail_cache_dir);
                        let _ = self
                            .library_db
                            .update_thumbnail_path(target_id, &new_thumb_path.to_string_lossy());
                        sync_success_count += 1;
                    }
                }
            }
            self.sidecar_status = t!("sync_dialog_success", "count" => sync_success_count);
            self.library_state.needs_refresh = true;
        }

        if close_dialog {
            self.show_sync_dialog = false;
        }
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
        self.load_file_path(target_path);
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
                                        ui.painter().text(
                                            rect.center(),
                                            Align2::CENTER_CENTER,
                                            "📷",
                                            FontId::proportional(16.0),
                                            Color32::GRAY,
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
            self.load_file_path(PathBuf::from(target_path));
        }
    }
}
