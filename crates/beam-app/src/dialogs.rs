//! Modal dialog windows extracted from `app.rs` for modularity.

use super::*;

impl BeamApp {
    /// Render export settings configuration modal
    pub(super) fn show_export_dialog_window(&mut self, ctx: &egui::Context) {
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
                                ui.radio_value(&mut self.export_format_index, 0, "JPEG");
                                ui.radio_value(&mut self.export_format_index, 1, "PNG");
                                ui.radio_value(
                                    &mut self.export_format_index,
                                    2,
                                    t!("export_format_tiff16"),
                                );
                                ui.radio_value(
                                    &mut self.export_format_index,
                                    3,
                                    t!("export_format_webp"),
                                );
                            });

                            if self.export_format_index == 0 {
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

                            // Watermark
                            ui.collapsing(t!("export_watermark"), |ui| {
                                ui.checkbox(
                                    &mut self.export_watermark_enabled,
                                    t!("export_watermark_enable"),
                                );
                                if self.export_watermark_enabled {
                                    ui.horizontal(|ui| {
                                        ui.label(t!("export_watermark_text"));
                                        ui.add(
                                            egui::TextEdit::singleline(
                                                &mut self.export_watermark_text,
                                            )
                                            .desired_width(180.0),
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(t!("export_watermark_logo"));
                                        if ui.button(t!("export_watermark_pick_logo")).clicked() {
                                            if let Some(path) = rfd::FileDialog::new()
                                                .add_filter("PNG", &["png"])
                                                .pick_file()
                                            {
                                                self.export_watermark_logo_path = Some(path);
                                            }
                                        }
                                        if let Some(ref p) = self.export_watermark_logo_path {
                                            let fname = p
                                                .file_name()
                                                .and_then(|n| n.to_str())
                                                .unwrap_or("logo.png");
                                            ui.label(fname);
                                            if ui
                                                .button(t!("export_watermark_clear_logo"))
                                                .clicked()
                                            {
                                                self.export_watermark_logo_path = None;
                                            }
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(t!("export_watermark_opacity"));
                                        ui.add(Slider::new(
                                            &mut self.export_watermark_opacity,
                                            0.05..=1.0,
                                        ));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(t!("export_watermark_scale"));
                                        ui.add(Slider::new(
                                            &mut self.export_watermark_scale,
                                            0.03..=0.5,
                                        ));
                                    });
                                    ui.label(t!("export_watermark_position"));
                                    let rows = [
                                        [
                                            (beam_media::WatermarkAnchor::TopLeft, "TL"),
                                            (beam_media::WatermarkAnchor::TopCenter, "TC"),
                                            (beam_media::WatermarkAnchor::TopRight, "TR"),
                                        ],
                                        [
                                            (beam_media::WatermarkAnchor::MiddleLeft, "ML"),
                                            (beam_media::WatermarkAnchor::Center, "C"),
                                            (beam_media::WatermarkAnchor::MiddleRight, "MR"),
                                        ],
                                        [
                                            (beam_media::WatermarkAnchor::BottomLeft, "BL"),
                                            (beam_media::WatermarkAnchor::BottomCenter, "BC"),
                                            (beam_media::WatermarkAnchor::BottomRight, "BR"),
                                        ],
                                    ];
                                    for row in rows {
                                        ui.horizontal(|ui| {
                                            for (anchor, label) in row {
                                                ui.selectable_value(
                                                    &mut self.export_watermark_anchor,
                                                    anchor,
                                                    label,
                                                );
                                            }
                                        });
                                    }
                                }
                            });
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
                            let ext = match self.export_format_index {
                                1 => "png",
                                2 => "tiff",
                                3 => "webp",
                                _ => "jpg",
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
    pub(super) fn show_export_progress_window(&mut self, ctx: &egui::Context) {
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
    pub(super) fn show_settings_dialog_window(&mut self, ctx: &egui::Context) {
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
                        ui.selectable_value(
                            &mut self.settings_tab,
                            SettingsTab::Plugins,
                            t!("settings_plugins"),
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
                        SettingsTab::Plugins => {
                            ui.heading(t!("settings_plugins"));
                            ui.separator();
                            ui.add_space(4.0);

                            ui.horizontal(|ui| {
                                if ui.button(t!("plugins_refresh")).clicked() {
                                    self.plugins = beam_core::discover_plugins(
                                        &beam_core::default_plugins_dir(),
                                    );
                                }
                                if ui.button(t!("plugins_install_sample")).clicked() {
                                    match beam_core::install_sample_plugin(
                                        &beam_core::default_plugins_dir(),
                                    ) {
                                        Ok(p) => {
                                            self.plugins = beam_core::discover_plugins(
                                                &beam_core::default_plugins_dir(),
                                            );
                                            self.plugin_status =
                                                Some(format!("Created {}", p.display()));
                                        }
                                        Err(e) => {
                                            self.plugin_status = Some(e.to_string());
                                        }
                                    }
                                }
                            });
                            ui.add_space(6.0);

                            if self.plugins.is_empty() {
                                ui.label(RichText::new(t!("plugins_none")).weak());
                            } else {
                                let plugins_snapshot = self.plugins.clone();
                                for p in plugins_snapshot {
                                    ui.group(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.strong(&p.name);
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    let can_run = self.current_path.is_some();
                                                    if ui
                                                        .add_enabled(
                                                            can_run,
                                                            egui::Button::new(t!("plugins_run")),
                                                        )
                                                        .clicked()
                                                    {
                                                        if let Some(img_path) = &self.current_path {
                                                            let sc = beam_core::Sidecar::path_for(
                                                                img_path,
                                                            );
                                                            let sc_opt = sc.exists().then_some(sc);
                                                            if beam_core::is_manifest_trusted(
                                                                &p,
                                                                &self
                                                                    .app_config
                                                                    .approved_plugin_hashes,
                                                            ) {
                                                                self.launch_trusted_plugin(
                                                                    ctx,
                                                                    p.clone(),
                                                                    img_path.clone(),
                                                                    sc_opt,
                                                                );
                                                            } else {
                                                                self.pending_plugin_approval =
                                                                    Some((
                                                                        p.clone(),
                                                                        img_path.clone(),
                                                                        sc_opt,
                                                                    ));
                                                            }
                                                        }
                                                    }
                                                },
                                            );
                                        });
                                        if !p.description.is_empty() {
                                            ui.label(RichText::new(&p.description).small().weak());
                                        }
                                    });
                                }
                            }

                            if let Some(status) = &self.plugin_status {
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(status)
                                        .small()
                                        .color(Color32::from_rgb(130, 210, 170)),
                                );
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
    pub(super) fn show_new_preset_dialog_window(&mut self, ctx: &egui::Context) {
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
    pub(super) fn show_copy_dialog_window(&mut self, ctx: &egui::Context) {
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
    pub(super) fn show_sync_dialog_window(&mut self, ctx: &egui::Context) {
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

    /// Render unsaved changes confirmation modal when attempting to exit or switch with modified edits
    pub(super) fn show_unsaved_changes_dialog_window(&mut self, ctx: &egui::Context) {
        if !self.show_unsaved_changes_dialog {
            return;
        }

        // Dim background to make the modal truly prominent and focused
        let screen_rect = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::unique("unsaved_dialog_dim"),
        ));
        painter.rect_filled(
            screen_rect,
            0.0,
            Color32::from_rgba_unmultiplied(0, 0, 0, 160),
        );

        let mut action_save = false;
        let mut action_discard = false;
        let mut action_cancel = false;

        egui::Window::new(t!("unsaved_dialog_title"))
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    ui.heading(
                        RichText::new(t!("unsaved_dialog_title"))
                            .strong()
                            .color(Color32::from_rgb(240, 200, 100)),
                    );
                    ui.separator();
                    ui.add_space(8.0);

                    ui.label(RichText::new(t!("unsaved_dialog_message")).size(14.0));

                    if let Some(path) = &self.current_path {
                        if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                            ui.add_space(6.0);
                            ui.colored_label(Color32::from_rgb(180, 200, 240), fname);
                        }
                    }

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        // 1. Kaydet (Save)
                        let save_btn = egui::Button::new(
                            RichText::new(t!("unsaved_save"))
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(45, 120, 210));
                        if ui.add(save_btn).clicked() {
                            action_save = true;
                        }

                        ui.add_space(8.0);

                        // 2. Kaydetmeden Çık (Exit without saving / Discard)
                        let discard_btn = egui::Button::new(
                            RichText::new(t!("unsaved_discard_and_exit"))
                                .color(Color32::from_rgb(255, 120, 120)),
                        );
                        if ui.add(discard_btn).clicked() {
                            action_discard = true;
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // 3. İptal (Cancel)
                            if ui.button(t!("unsaved_cancel")).clicked() {
                                action_cancel = true;
                            }
                        });
                    });

                    // Keyboard shortcuts in dialog: Escape cancels, Enter saves
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        action_cancel = true;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        action_save = true;
                    }
                });
            });

        if action_save {
            self.save_project();
            self.show_unsaved_changes_dialog = false;
            if let Some(action) = self.pending_action.take() {
                self.execute_pending_action(action, ctx);
            }
        } else if action_discard {
            self.show_unsaved_changes_dialog = false;
            if let Some(action) = self.pending_action.take() {
                self.execute_pending_action(action, ctx);
            }
        } else if action_cancel {
            self.show_unsaved_changes_dialog = false;
            self.pending_action = None;
        }
    }

    fn launch_trusted_plugin(
        &mut self,
        ctx: &egui::Context,
        plugin: beam_core::PluginManifest,
        img_path: PathBuf,
        sc_opt: Option<PathBuf>,
    ) {
        let approved_snapshot = self.app_config.approved_plugin_hashes.clone();
        let tx = self.tx.clone();
        let ctx_clone = ctx.clone();
        self.plugin_status = Some(format!("[{}] running...", plugin.id));
        std::thread::spawn(move || {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                beam_core::run_plugin_trusted(
                    &plugin,
                    &approved_snapshot,
                    &img_path,
                    sc_opt.as_deref(),
                )
            }));
            let msg = match res {
                Ok(Ok(out)) => {
                    if out.stderr.is_empty() {
                        format!("[{}] exit {}: {}", plugin.id, out.exit_code, out.stdout)
                    } else {
                        format!(
                            "[{}] exit {}: {} ({})",
                            plugin.id, out.exit_code, out.stdout, out.stderr
                        )
                    }
                }
                Ok(Err(e)) => format!("[{}] error: {e}", plugin.id),
                Err(_) => format!("[{}] error: plugin runner panicked", plugin.id),
            };
            let _ = tx.send(BackgroundMsg::PluginFinished(msg));
            ctx_clone.request_repaint();
        });
    }

    pub(super) fn show_plugin_approval_dialog_window(&mut self, ctx: &egui::Context) {
        let Some((plugin, img_path, sc_opt)) = self.pending_plugin_approval.clone() else {
            return;
        };

        let mut approve = false;
        let mut cancel = false;
        let resolved_path = beam_core::resolve_command_path(&plugin.command)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<not found in PATH>".to_string());
        let work_dir = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string());
        let digest = beam_core::manifest_sha256(&plugin);

        egui::Window::new(t!("plugin_approval_title"))
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(440.0);
                ui.label(t!("plugin_approval_warning"));
                ui.add_space(8.0);
                egui::Grid::new("plugin_approval_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong(&plugin.name);
                        ui.monospace(&plugin.id);
                        ui.end_row();

                        ui.strong(t!("plugin_approval_command"));
                        ui.monospace(&plugin.command);
                        ui.end_row();

                        ui.strong(t!("plugin_approval_resolved"));
                        ui.monospace(&resolved_path);
                        ui.end_row();

                        ui.strong(t!("plugin_approval_args"));
                        ui.monospace(if plugin.args.is_empty() {
                            "<none>".to_string()
                        } else {
                            plugin.args.join(" ")
                        });
                        ui.end_row();

                        ui.strong(t!("plugin_approval_workdir"));
                        ui.monospace(&work_dir);
                        ui.end_row();

                        ui.strong(t!("plugin_approval_sha256"));
                        ui.monospace(&digest);
                        ui.end_row();
                    });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("plugin_approval_approve")).clicked() {
                        approve = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t!("plugin_approval_cancel")).clicked() {
                            cancel = true;
                        }
                    });
                });
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
            });

        if approve {
            self.app_config
                .approved_plugin_hashes
                .insert(plugin.id.trim().to_string(), digest);
            let _ = self.app_config.save();
            self.pending_plugin_approval = None;
            self.launch_trusted_plugin(ctx, plugin, img_path, sc_opt);
        } else if cancel {
            self.pending_plugin_approval = None;
        }
    }
}
