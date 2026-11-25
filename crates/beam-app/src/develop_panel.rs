//! Right-side Develop adjustment panel extracted from `app.rs` for modularity.

use super::*;

impl BeamApp {
    pub(super) fn show_develop_right_panel(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        frame: &mut eframe::Frame,
    ) {
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
    }
}
