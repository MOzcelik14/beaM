use beam_core::t;
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke, Vec2};
use eframe::epaint::PathShape;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistogramMode {
    Rgb,
    Luminance,
    Red,
    Green,
    Blue,
}

#[derive(Debug, Clone)]
pub struct HistogramData {
    pub r: [u32; 256],
    pub g: [u32; 256],
    pub b: [u32; 256],
    pub lum: [u32; 256],
    pub max_count: u32,
}

impl Default for HistogramData {
    fn default() -> Self {
        Self {
            r: [0; 256],
            g: [0; 256],
            b: [0; 256],
            lum: [0; 256],
            max_count: 1,
        }
    }
}

impl HistogramData {
    /// Compute 256-bin histogram from RGBA32F linear or sRGB pixel data.
    /// Uses stride subsampling on large images to compute in <1ms.
    pub fn compute_from_rgba32f(data: &[f32]) -> Self {
        let mut r = [0u32; 256];
        let mut g = [0u32; 256];
        let mut b = [0u32; 256];
        let mut lum = [0u32; 256];

        let num_pixels = data.len() / 4;
        if num_pixels == 0 {
            return Self::default();
        }

        // Subsample if image is large (> 250,000 pixels) for instantaneous response
        let stride = (num_pixels / 250_000).max(1);

        for chunk in data.chunks_exact(4 * stride) {
            let pr = chunk[0].clamp(0.0, 1.0);
            let pg = chunk[1].clamp(0.0, 1.0);
            let pb = chunk[2].clamp(0.0, 1.0);

            let bin_r = (pr * 255.0).round() as usize;
            let bin_g = (pg * 255.0).round() as usize;
            let bin_b = (pb * 255.0).round() as usize;

            // Rec.2020 luminance
            let plum = (0.2627 * pr + 0.6780 * pg + 0.0593 * pb).clamp(0.0, 1.0);
            let bin_lum = (plum * 255.0).round() as usize;

            r[bin_r] += 1;
            g[bin_g] += 1;
            b[bin_b] += 1;
            lum[bin_lum] += 1;
        }

        // Avoid letting pure 0 (crushed black) or pure 255 (clipped white)
        // dominate the graph scale.
        let mut max_count = 1u32;
        for i in 1..255 {
            max_count = max_count.max(r[i]).max(g[i]).max(b[i]).max(lum[i]);
        }
        // Account for endpoints gracefully
        max_count = max_count
            .max(r[0] / 4)
            .max(r[255] / 4)
            .max(lum[0] / 4)
            .max(lum[255] / 4)
            .max(1);

        Self {
            r,
            g,
            b,
            lum,
            max_count,
        }
    }

    /// Analyze luminance percentiles and return balanced Auto Tone parameters:
    /// `(exposure_ev, contrast, highlights, shadows, whites)`.
    pub fn compute_auto_tone(&self) -> (f32, f32, f32, f32, f32) {
        let total: u64 = self.lum.iter().map(|&c| c as u64).sum();
        if total == 0 {
            return (0.0, 0.0, 0.0, 0.0, 0.0);
        }

        let percentile_bin = |frac: f64| -> usize {
            let target = ((total as f64) * frac).round() as u64;
            let mut acc = 0u64;
            for (i, &c) in self.lum.iter().enumerate() {
                acc += c as u64;
                if acc >= target {
                    return i;
                }
            }
            255
        };

        let p01 = percentile_bin(0.01);
        let p50 = percentile_bin(0.50);
        let p99 = percentile_bin(0.99);

        // Nudge median towards perceptual mid-grey (~0.42)
        let med_norm = (p50.max(4) as f32) / 255.0;
        let ev = ((0.42f32.log2() - med_norm.log2()) * 0.55).clamp(-2.5, 2.5);

        // Recover clipped highlights or lift flat whites
        let highlights = if p99 >= 248 {
            -35.0
        } else if p99 >= 235 {
            -15.0
        } else {
            0.0
        };

        let whites = if p99 < 200 {
            ((220.0 - p99 as f32) * 0.45).clamp(10.0, 45.0)
        } else {
            0.0
        };

        // Lift crushed shadows
        let shadows = if p01 <= 6 {
            32.0
        } else if p01 <= 16 {
            16.0
        } else {
            0.0
        };

        // Contrast boost for narrow dynamic range
        let spread = p99.saturating_sub(p01);
        let contrast = if spread < 130 {
            20.0
        } else if spread < 180 {
            10.0
        } else {
            5.0
        };

        (ev, contrast, highlights, shadows, whites)
    }
}

/// Renders the histogram into the given egui UI.
#[allow(dead_code)]
pub fn show_histogram(ui: &mut egui::Ui, data: &HistogramData, mode: &mut HistogramMode) {
    let mut shadow_dummy = false;
    let mut highlight_dummy = false;
    show_histogram_with_clipping(ui, data, mode, &mut shadow_dummy, &mut highlight_dummy);
}

/// Renders the histogram with interactive highlight/shadow clipping warning toggles.
/// Returns true if either clipping toggle was switched.
pub fn show_histogram_with_clipping(
    ui: &mut egui::Ui,
    data: &HistogramData,
    mode: &mut HistogramMode,
    show_shadow_clipping: &mut bool,
    show_highlight_clipping: &mut bool,
) -> bool {
    let mut changed = false;

    ui.vertical(|ui| {
        // Channel selector and clipping toggles
        ui.horizontal(|ui| {
            // Shadow clipping toggle on left
            let is_shadow = *show_shadow_clipping;
            let shadow_btn = egui::Button::new(
                RichText::new("◺")
                    .color(if is_shadow {
                        Color32::from_rgb(80, 165, 255)
                    } else {
                        Color32::from_rgb(120, 125, 140)
                    })
                    .size(11.0)
                    .strong(),
            )
            .fill(if is_shadow {
                Color32::from_rgb(25, 45, 75)
            } else {
                Color32::from_rgb(20, 22, 28)
            })
            .stroke(Stroke::new(
                1.0,
                if is_shadow {
                    Color32::from_rgb(50, 100, 180)
                } else {
                    Color32::from_rgb(36, 40, 50)
                },
            ))
            .corner_radius(4.0)
            .min_size(Vec2::new(18.0, 18.0));

            if ui
                .add(shadow_btn)
                .on_hover_text(t!("histogram_shadow_clipping"))
                .clicked()
            {
                *show_shadow_clipping = !*show_shadow_clipping;
                changed = true;
            }

            ui.add_space(2.0);

            let channels = [
                (HistogramMode::Rgb, "RGB"),
                (HistogramMode::Luminance, "Lum"),
                (HistogramMode::Red, "R"),
                (HistogramMode::Green, "G"),
                (HistogramMode::Blue, "B"),
            ];

            for (m, lbl) in channels {
                let is_sel = *mode == m;
                let (bg, stroke, fg) = if is_sel {
                    (
                        Color32::from_rgb(46, 56, 78),
                        Stroke::new(1.0, Color32::from_rgb(70, 92, 138)),
                        Color32::from_rgb(240, 245, 255),
                    )
                } else {
                    (
                        Color32::from_rgb(20, 22, 28),
                        Stroke::new(1.0, Color32::from_rgb(34, 38, 48)),
                        Color32::from_rgb(150, 155, 168),
                    )
                };
                if ui
                    .add(
                        egui::Button::new(RichText::new(lbl).size(10.5).color(fg))
                            .fill(bg)
                            .stroke(stroke)
                            .corner_radius(8.0)
                            .min_size(Vec2::new(0.0, 18.0)),
                    )
                    .clicked()
                {
                    *mode = m;
                }
            }

            ui.add_space(2.0);

            // Highlight clipping toggle on right
            let is_hl = *show_highlight_clipping;
            let hl_btn = egui::Button::new(
                RichText::new("◸")
                    .color(if is_hl {
                        Color32::from_rgb(255, 80, 80)
                    } else {
                        Color32::from_rgb(120, 125, 140)
                    })
                    .size(11.0)
                    .strong(),
            )
            .fill(if is_hl {
                Color32::from_rgb(60, 25, 28)
            } else {
                Color32::from_rgb(20, 22, 28)
            })
            .stroke(Stroke::new(
                1.0,
                if is_hl {
                    Color32::from_rgb(140, 45, 52)
                } else {
                    Color32::from_rgb(36, 40, 50)
                },
            ))
            .corner_radius(4.0)
            .min_size(Vec2::new(18.0, 18.0));

            if ui
                .add(hl_btn)
                .on_hover_text(t!("histogram_highlight_clipping"))
                .clicked()
            {
                *show_highlight_clipping = !*show_highlight_clipping;
                changed = true;
            }
        });

        ui.add_space(3.0);

        let width = ui.available_width().max(100.0);
        let height = 90.0f32;
        let (response, painter) = ui.allocate_painter(Vec2::new(width, height), Sense::hover());
        let rect = response.rect;

        // Background box
        painter.rect_filled(rect, 6.0, Color32::from_rgb(14, 15, 19));
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0, Color32::from_rgb(34, 38, 48)),
            egui::StrokeKind::Inside,
        );

        // Grid lines (vertical at 25%, 50%, 75%)
        let grid_stroke = Stroke::new(1.0, Color32::from_rgb(26, 28, 36));
        for i in 1..4 {
            let x = rect.left() + (rect.width() * i as f32 / 4.0);
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                grid_stroke,
            );
        }

        // Draw curves according to mode
        let max_val = data.max_count as f32;

        let draw_channel = |bins: &[u32; 256], fill_col: Color32, stroke_col: Color32| {
            let mut pts = Vec::with_capacity(258);
            pts.push(Pos2::new(rect.left(), rect.bottom()));

            for (i, &count) in bins.iter().enumerate() {
                let x = rect.left() + (i as f32 / 255.0) * rect.width();
                let norm = (count as f32 / max_val).min(1.0);
                let y = rect.bottom() - (norm * (rect.height() - 2.0));
                pts.push(Pos2::new(x, y));
            }

            pts.push(Pos2::new(rect.right(), rect.bottom()));

            painter.add(egui::Shape::Path(PathShape {
                points: pts.clone(),
                closed: true,
                fill: fill_col,
                stroke: Stroke::NONE.into(),
            }));

            // Stroke line along the curve (excluding bottom boundary)
            let curve_pts = &pts[1..pts.len() - 1];
            painter.add(egui::Shape::line(
                curve_pts.to_vec(),
                Stroke::new(1.2, stroke_col),
            ));
        };

        match mode {
            HistogramMode::Rgb => {
                // Overlaid R, G, B with translucent alpha
                draw_channel(
                    &data.r,
                    Color32::from_rgba_unmultiplied(230, 60, 60, 75),
                    Color32::from_rgb(240, 70, 70),
                );
                draw_channel(
                    &data.g,
                    Color32::from_rgba_unmultiplied(60, 200, 70, 75),
                    Color32::from_rgb(70, 215, 80),
                );
                draw_channel(
                    &data.b,
                    Color32::from_rgba_unmultiplied(60, 120, 240, 75),
                    Color32::from_rgb(70, 135, 255),
                );
            }
            HistogramMode::Luminance => {
                draw_channel(
                    &data.lum,
                    Color32::from_rgba_unmultiplied(200, 200, 200, 90),
                    Color32::from_rgb(230, 230, 230),
                );
            }
            HistogramMode::Red => {
                draw_channel(
                    &data.r,
                    Color32::from_rgba_unmultiplied(230, 60, 60, 110),
                    Color32::from_rgb(240, 70, 70),
                );
            }
            HistogramMode::Green => {
                draw_channel(
                    &data.g,
                    Color32::from_rgba_unmultiplied(60, 200, 70, 110),
                    Color32::from_rgb(70, 215, 80),
                );
            }
            HistogramMode::Blue => {
                draw_channel(
                    &data.b,
                    Color32::from_rgba_unmultiplied(60, 120, 240, 110),
                    Color32::from_rgb(70, 135, 255),
                );
            }
        }
    });

    changed
}
