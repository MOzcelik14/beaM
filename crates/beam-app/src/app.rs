//! Main egui application view and state for beaM.

use beam_core::APP_NAME;
use beam_gpu::{DisplayPipeline, GpuImage};
use beam_media::{decode_image, DecodedImage, ImageMetadata};
use eframe::egui::{self, Color32, MenuBar, Panel, RichText, Vec2};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

enum BackgroundMsg {
    Started(PathBuf),
    Success(Box<DecodedImage>),
    Error(String),
}

pub struct BeamApp {
    // Current decoded image and metadata
    current_image: Option<Box<DecodedImage>>,
    current_path: Option<PathBuf>,
    current_metadata: Option<ImageMetadata>,

    // GPU rendering state
    display_pipeline: Option<DisplayPipeline>,
    egui_texture_id: Option<egui::TextureId>,
    gpu_display_texture: Option<wgpu::Texture>,
    texture_aspect_ratio: f32,

    // Background decoding
    tx: Sender<BackgroundMsg>,
    rx: Receiver<BackgroundMsg>,
    is_loading: bool,
    loading_filename: Option<String>,

    // UI state
    error_message: Option<String>,
    show_about: bool,
}

impl BeamApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel();

        // Initialize display pipeline using the wgpu device if available
        let display_pipeline = cc
            .wgpu_render_state
            .as_ref()
            .map(|rs| DisplayPipeline::new(&rs.device, wgpu::TextureFormat::Rgba8Unorm));

        Self {
            current_image: None,
            current_path: None,
            current_metadata: None,
            display_pipeline,
            egui_texture_id: None,
            gpu_display_texture: None,
            texture_aspect_ratio: 1.0,
            tx,
            rx,
            is_loading: false,
            loading_filename: None,
            error_message: None,
            show_about: false,
        }
    }

    /// Open file dialog and spawn background thread to decode the RAW image
    pub fn open_file_dialog(&mut self) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .set_title("Open RAW Photo")
                .add_filter(
                    "RAW Images & Photos",
                    &[
                        "arw", "cr2", "cr3", "nef", "dng", "raf", "rw2", "orf", "pef", "jpg",
                        "jpeg", "png", "tif", "tiff",
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

    /// Load a specific file path asynchronously
    pub fn load_file_path(&mut self, path: PathBuf) {
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

    /// Process messages from the background decoding thread
    fn check_background_messages(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                BackgroundMsg::Started(path) => {
                    self.is_loading = true;
                    self.loading_filename = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string());
                    self.current_path = Some(path);
                    self.error_message = None;
                    ctx.request_repaint();
                }
                BackgroundMsg::Success(decoded) => {
                    self.is_loading = false;
                    self.loading_filename = None;
                    self.current_metadata = Some(decoded.metadata.clone());
                    self.upload_and_render_image(frame, *decoded);
                    ctx.request_repaint();
                }
                BackgroundMsg::Error(err_msg) => {
                    self.is_loading = false;
                    self.loading_filename = None;
                    self.error_message = Some(err_msg);
                    ctx.request_repaint();
                }
            }
        }
    }

    /// Upload decoded image to GPU and render display transform
    fn upload_and_render_image(&mut self, frame: &mut eframe::Frame, decoded: DecodedImage) {
        let Some(render_state) = frame.wgpu_render_state() else {
            self.error_message = Some("GPU RenderState not available".to_string());
            return;
        };

        let device = &render_state.device;
        let queue = &render_state.queue;

        // Ensure display pipeline is initialized
        if self.display_pipeline.is_none() {
            self.display_pipeline = Some(DisplayPipeline::new(
                device,
                wgpu::TextureFormat::Rgba8Unorm,
            ));
        }

        let pipeline = self.display_pipeline.as_ref().unwrap();

        // 1. Upload linear float pixels into GPU image texture
        let gpu_image = match GpuImage::from_rgba32f(
            device,
            queue,
            decoded.width,
            decoded.height,
            &decoded.data,
        ) {
            Ok(img) => img,
            Err(e) => {
                self.error_message = Some(format!("Failed to upload image to GPU: {e}"));
                return;
            }
        };

        // 2. Create the display target texture (Rgba8Unorm)
        let display_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("beam_display_target_texture"),
            size: wgpu::Extent3d {
                width: decoded.width,
                height: decoded.height,
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

        // 3. Execute display transform render pass (Linear Rec.2020 -> sRGB)
        pipeline.render_to_target(device, queue, &gpu_image, &display_view);

        // 4. Register or update egui native texture
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

        self.texture_aspect_ratio = decoded.width as f32 / decoded.height as f32;
        self.gpu_display_texture = Some(display_texture);
        self.current_image = Some(Box::new(decoded));
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

                ui.menu_button("File", |ui| {
                    if ui.button("Open RAW Photo... (Ctrl+O)").clicked() {
                        self.open_file_dialog();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button(format!("About {APP_NAME}")).clicked() {
                        self.show_about = true;
                    }
                });
            });
        });

        // Keyboard shortcuts
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
            self.open_file_dialog();
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
                } else {
                    ui.label(
                        "No image loaded. Use File -> Open RAW Photo... (Ctrl+O) to open an image.",
                    );
                }

                if self.is_loading {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spinner();
                        if let Some(fname) = &self.loading_filename {
                            ui.label(format!("Decoding {fname} with rawler..."));
                        } else {
                            ui.label("Decoding...");
                        }
                    });
                }
            });
        });

        // Central viewport panel
        egui::CentralPanel::default().show(ui, |ui| {
            // Error banner if any
            let mut clear_error = false;
            if let Some(err) = &self.error_message {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(Color32::RED, "⚠ Error:");
                        ui.label(RichText::new(err).color(Color32::LIGHT_RED));
                        if ui.button("Dismiss").clicked() {
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
                // Aspect-fit image display
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
                        ui.label("Open-source non-destructive RAW photo editor & library manager");
                        ui.add_space(20.0);
                        if ui
                            .button(RichText::new("📂  Open RAW Photo (Ctrl+O)").size(16.0))
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
                        ui.label("Decoding RAW file with pure-Rust rawler...");
                    });
                });
            }
        });

        // About dialog
        if self.show_about {
            egui::Window::new(format!("About {APP_NAME}"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(&ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading(RichText::new(APP_NAME).size(26.0).strong());
                        ui.label("Version 0.1.0");
                        ui.label("License: GNU General Public License v3.0");
                        ui.add_space(10.0);
                        ui.label("A non-destructive RAW photo editor and library manager.");
                        ui.label("Built with Rust, pure-Rust rawler, wgpu, and egui.");
                        ui.add_space(15.0);
                        if ui.button("Close").clicked() {
                            self.show_about = false;
                        }
                    });
                });
        }
    }
}
