//! Main egui application view and state for beaM.

use beam_core::sidecar::{EditStack, Sidecar};
use beam_core::APP_NAME;
use beam_gpu::{EditPipeline, GpuImage};
use beam_media::{decode_image, DecodedImage, ImageMetadata};
use eframe::egui::{self, Color32, MenuBar, Panel, RichText, Slider, Vec2};
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

    // UI state
    error_message: Option<String>,
    show_about: bool,
    show_develop_panel: bool,
}

impl BeamApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel();

        let edit_pipeline = cc
            .wgpu_render_state
            .as_ref()
            .map(|rs| EditPipeline::new(&rs.device, wgpu::TextureFormat::Rgba8Unorm));

        Self {
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
            error_message: None,
            show_about: false,
            show_develop_panel: true,
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
                    self.error_message = Some(err_msg);
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

        self.texture_aspect_ratio = decoded.width as f32 / decoded.height as f32;
        self.gpu_display_texture = Some(display_texture);
        self.gpu_proxy_image = Some(gpu_proxy);
        self.current_image = Some(Box::new(decoded));
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
            if let Err(e) = sidecar.save_to_file(sidecar_path) {
                log::error!("Failed to save sidecar to {}: {e}", sidecar_path.display());
                self.sidecar_status = format!("Save error: {e}");
            } else {
                self.sidecar_status = "Saved".to_string();
            }
        }
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

                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_develop_panel, "Develop Panel");
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

                    ui.separator();
                    ui.label(format!("Sidecar: {}", self.sidecar_status));
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

        // Right-side Develop Adjustment Panel
        let mut edits_changed = false;
        if self.show_develop_panel && self.current_image.is_some() {
            Panel::right("develop_panel")
                .resizable(true)
                .default_size(300.0)
                .min_size(260.0)
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.heading("Develop");
                        ui.separator();

                        // 1. White Balance Section
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.strong("White Balance");
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .button("↺")
                                            .on_hover_text("Reset White Balance")
                                            .clicked()
                                        {
                                            self.edit_stack.reset_white_balance();
                                            edits_changed = true;
                                        }
                                        ui.checkbox(&mut self.edit_stack.white_balance.enabled, "");
                                    },
                                );
                            });

                            ui.add_space(4.0);

                            // Temperature slider
                            ui.horizontal(|ui| {
                                ui.label("Temp");
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
                                    .on_hover_text("Reset Temperature (0)")
                                    .clicked()
                                {
                                    self.edit_stack.white_balance.temperature = 0.0;
                                    edits_changed = true;
                                }
                            });

                            // Tint slider
                            ui.horizontal(|ui| {
                                ui.label("Tint  ");
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
                                if ui.button("↺").on_hover_text("Reset Tint (0)").clicked() {
                                    self.edit_stack.white_balance.tint = 0.0;
                                    edits_changed = true;
                                }
                            });
                        });

                        ui.add_space(8.0);

                        // 2. Exposure Section
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.strong("Exposure");
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("↺").on_hover_text("Reset Exposure").clicked()
                                        {
                                            self.edit_stack.reset_exposure();
                                            edits_changed = true;
                                        }
                                        ui.checkbox(&mut self.edit_stack.exposure.enabled, "");
                                    },
                                );
                            });

                            ui.add_space(4.0);

                            // Exposure EV slider
                            ui.horizontal(|ui| {
                                ui.label("EV    ");
                                let resp = ui.add(
                                    Slider::new(&mut self.edit_stack.exposure.ev, -5.00..=5.00)
                                        .step_by(0.05)
                                        .custom_formatter(|v, _| format!("{:+.2} EV", v)),
                                );
                                if resp.changed() {
                                    edits_changed = true;
                                }
                                if ui.button("↺").on_hover_text("Reset Exposure (0)").clicked() {
                                    self.edit_stack.exposure.ev = 0.0;
                                    edits_changed = true;
                                }
                            });
                        });

                        ui.add_space(8.0);

                        // 3. Contrast Section
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.strong("Contrast");
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("↺").on_hover_text("Reset Contrast").clicked()
                                        {
                                            self.edit_stack.reset_contrast();
                                            edits_changed = true;
                                        }
                                        ui.checkbox(&mut self.edit_stack.contrast.enabled, "");
                                    },
                                );
                            });

                            ui.add_space(4.0);

                            // Contrast factor slider
                            ui.horizontal(|ui| {
                                ui.label("Level ");
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
                                if ui.button("↺").on_hover_text("Reset Contrast (0)").clicked() {
                                    self.edit_stack.contrast.contrast = 0.0;
                                    edits_changed = true;
                                }
                            });
                        });

                        ui.add_space(16.0);

                        // 4. Action Buttons
                        ui.horizontal(|ui| {
                            if ui
                                .button(RichText::new("↺ Reset All").color(Color32::LIGHT_RED))
                                .on_hover_text("Reset all adjustments to defaults")
                                .clicked()
                            {
                                self.edit_stack.reset_all();
                                edits_changed = true;
                            }

                            if ui.button("💾 Save Sidecar").clicked() {
                                self.save_sidecar();
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
