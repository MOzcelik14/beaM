use crate::views::library::{
    show_library_view, LibraryAction, LibraryViewState, ThumbnailRequest, ThumbnailStatus,
};
use crate::widgets::curve::show_tone_curve;
use crate::widgets::histogram::{show_histogram, HistogramData, HistogramMode};
use beam_core::sidecar::{EditStack, Sidecar};
use beam_core::{set_language, t, AppConfig, Language, APP_NAME};
use beam_gpu::{EditPipeline, GpuImage};
use beam_library::{default_catalog_path, default_thumbnail_cache_dir, LibraryDb};
use beam_media::{decode_image, DecodedImage, ImageMetadata};
use eframe::egui::{self, Color32, MenuBar, Panel, RichText, Slider, Vec2};
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
    texture_aspect_ratio: f32,

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
    app_config: AppConfig,

    // Export dialog & process state
    show_export_dialog: bool,
    show_export_progress: bool,
    export_output_dir: PathBuf,
    export_format_is_png: bool,
    export_jpeg_quality: u8,
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
            texture_aspect_ratio: 1.0,
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
            app_config,
            show_export_dialog: false,
            show_export_progress: false,
            export_output_dir,
            export_format_is_png: false,
            export_jpeg_quality: 90,
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
                                self.sidecar_status = "Loaded from disk".to_string();
                            }
                            Err(e) => {
                                log::warn!("Failed to read existing sidecar: {e}");
                                self.edit_stack = EditStack::default();
                                self.sidecar_status = "Failed to parse sidecar".to_string();
                            }
                        }
                    } else {
                        self.edit_stack = EditStack::default();
                        self.sidecar_status = "New sidecar".to_string();
                    }
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

        // Register or update egui native texture
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

        self.histogram_data = Some(HistogramData::compute_from_rgba32f(&decoded.proxy_data));
        self.texture_aspect_ratio = decoded.width as f32 / decoded.height as f32;
        self.gpu_display_texture = Some(display_texture);
        self.gpu_proxy_image = Some(gpu_proxy);
        self.current_image = Some(Box::new(decoded));

        // Automatically register photo in library database
        if let Some(path) = &self.current_path {
            if let Ok(record) = self.library_db.import_file(path, &self.thumbnail_cache_dir) {
                self.library_state.selected_photo_id = Some(record.id);
            }
            self.library_state.needs_refresh = true;
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

        let device = &render_state.device;
        let queue = &render_state.queue;
        let display_view = display_tex.create_view(&wgpu::TextureViewDescriptor::default());

        pipeline.render(device, queue, gpu_proxy, &display_view, &self.edit_stack);

        // Save sidecar automatically
        self.save_sidecar();
    }

    /// Persist current edit stack to the sidecar file on disk
    fn save_sidecar(&mut self) {
        if let (Some(path), Some(sidecar_path)) = (&self.current_path, &self.sidecar_path) {
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
            let mut sidecar = Sidecar::new(filename);
            sidecar.edit_stack = self.edit_stack.clone();

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
                    if ui.button(t!("menu_settings")).clicked() {
                        self.show_settings = true;
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
                });

                ui.menu_button(t!("menu_help"), |ui| {
                    if ui.button(t!("menu_about")).clicked() {
                        self.show_about = true;
                    }
                });
            });
        });

        // Global Keyboard shortcuts
        if ctx.input(|i| i.key_pressed(egui::Key::G) && !i.modifiers.command && !i.modifiers.ctrl) {
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
                    LibraryAction::None => {}
                }
            }
            AppView::Develop => {
                // Right-side Develop Adjustment Panel
                let mut edits_changed = false;
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
                                    if let Some(hist_data) = &self.histogram_data {
                                        ui.group(|ui| {
                                            show_histogram(ui, hist_data, &mut self.histogram_mode);
                                        });
                                        ui.add_space(4.0);
                                    }

                                    // 2. White Balance Section
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
                                                    }
                                                    ui.checkbox(
                                                        &mut self.edit_stack.white_balance.enabled,
                                                        "",
                                                    );
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
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_temp"))
                                                .clicked()
                                            {
                                                self.edit_stack.white_balance.temperature = 0.0;
                                                edits_changed = true;
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
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_tint"))
                                                .clicked()
                                            {
                                                self.edit_stack.white_balance.tint = 0.0;
                                                edits_changed = true;
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
                                                    }
                                                    ui.checkbox(
                                                        &mut self.edit_stack.exposure.enabled,
                                                        "",
                                                    );
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
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_exposure"))
                                                .clicked()
                                            {
                                                self.edit_stack.exposure.ev = 0.0;
                                                edits_changed = true;
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
                                                    }
                                                    ui.checkbox(
                                                        &mut self
                                                            .edit_stack
                                                            .highlights_shadows
                                                            .enabled,
                                                        "",
                                                    );
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
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_highlights"))
                                                .clicked()
                                            {
                                                self.edit_stack.highlights_shadows.highlights = 0.0;
                                                edits_changed = true;
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
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_shadows"))
                                                .clicked()
                                            {
                                                self.edit_stack.highlights_shadows.shadows = 0.0;
                                                edits_changed = true;
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
                                                    }
                                                    ui.checkbox(
                                                        &mut self.edit_stack.contrast.enabled,
                                                        "",
                                                    );
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
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_contrast_val"))
                                                .clicked()
                                            {
                                                self.edit_stack.contrast.contrast = 0.0;
                                                edits_changed = true;
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
                                                    }
                                                    ui.checkbox(
                                                        &mut self.edit_stack.tone_curve.enabled,
                                                        "",
                                                    );
                                                },
                                            );
                                        });

                                        ui.add_space(4.0);

                                        if show_tone_curve(
                                            ui,
                                            &mut self.edit_stack.tone_curve.points,
                                        ) {
                                            edits_changed = true;
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
                                                    }
                                                    ui.checkbox(
                                                        &mut self.edit_stack.hsl.enabled,
                                                        "",
                                                    );
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
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_hue"))
                                                .clicked()
                                            {
                                                band.hue = 0.0;
                                                edits_changed = true;
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
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_sat"))
                                                .clicked()
                                            {
                                                band.saturation = 0.0;
                                                edits_changed = true;
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
                                            }
                                            if ui
                                                .button("↺")
                                                .on_hover_text(t!("develop_reset_lum"))
                                                .clicked()
                                            {
                                                band.luminance = 0.0;
                                                edits_changed = true;
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

                // Apply edits if sliders or reset buttons were clicked
                if edits_changed {
                    self.re_render_edits(frame);
                    ctx.request_repaint();
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
                            ui.image(egui::load::SizedTexture::new(texture_id, display_size));
                        });
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
        let config = beam_media::ExportConfig {
            output_dir: self.export_output_dir.clone(),
            format: if self.export_format_is_png {
                beam_media::ExportFormat::Png
            } else {
                beam_media::ExportFormat::Jpeg {
                    quality: self.export_jpeg_quality,
                }
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
                ui.set_min_width(400.0);
                ui.vertical(|ui| {
                    ui.heading(t!("export_title"));
                    ui.separator();

                    let count = self.export_targets.len();
                    ui.label(
                        RichText::new(t!("export_photos_to_export", "count" => count)).strong(),
                    );
                    ui.add_space(8.0);

                    // Destination folder
                    ui.label(t!("export_destination_folder"));
                    ui.horizontal(|ui| {
                        let path_str = self.export_output_dir.display().to_string();
                        let mut text = path_str.as_str();
                        ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .desired_width(280.0)
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

                    // Format
                    ui.label(t!("export_format"));
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.export_format_is_png, false, "JPEG");
                        ui.radio_value(&mut self.export_format_is_png, true, "PNG");
                    });

                    if !self.export_format_is_png {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(t!("export_jpeg_quality"));
                            ui.add(Slider::new(&mut self.export_jpeg_quality, 1..=100).text("%"));
                        });
                    }

                    ui.add_space(8.0);

                    // Filename Suffix
                    ui.horizontal(|ui| {
                        ui.label(t!("export_filename_suffix"));
                        ui.text_edit_singleline(&mut self.export_suffix);
                    });
                    ui.label(RichText::new(t!("export_suffix_example")).weak().size(11.0));

                    ui.add_space(16.0);
                    ui.separator();

                    // Dialog buttons
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new(t!("export_start_btn")).strong().size(14.0))
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

    /// Render settings modal with language switcher and catalog storage info
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
                ui.set_min_width(380.0);
                ui.vertical(|ui| {
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
                            .desired_width(360.0)
                            .interactive(false),
                    );

                    ui.add_space(4.0);
                    ui.label(RichText::new(t!("settings_cache_path")).strong());
                    let mut cache_path = self.thumbnail_cache_dir.display().to_string();
                    ui.add(
                        egui::TextEdit::singleline(&mut cache_path)
                            .desired_width(360.0)
                            .interactive(false),
                    );

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
}
