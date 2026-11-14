//! Interactive 3-way Color Grading Wheel widget for beaM.

use beam_core::sidecar::ColorWheelGrade;
use eframe::egui::{self, Color32, Pos2, Sense, Stroke, Vec2};

fn hsv_to_color32(hue_deg: f32, sat: f32, val: f32) -> Color32 {
    let h = (hue_deg.rem_euclid(360.0)) / 60.0;
    let sector = h.floor() as i32;
    let f = h - sector as f32;
    let p = val * (1.0 - sat);
    let q = val * (1.0 - sat * f);
    let t = val * (1.0 - sat * (1.0 - f));
    let (r, g, b) = match sector {
        0 => (val, t, p),
        1 => (q, val, p),
        2 => (p, val, t),
        3 => (p, q, val),
        4 => (t, p, val),
        _ => (val, p, q),
    };
    Color32::from_rgb(
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    )
}

/// Renders an interactive circular Hue/Saturation color wheel and luminance slider.
/// Returns `(changed, interaction_stopped)`.
pub fn show_color_wheel(
    ui: &mut egui::Ui,
    label: &str,
    grade: &mut ColorWheelGrade,
    lum_label: &str,
) -> (bool, bool) {
    let mut changed = false;
    let mut stopped = false;

    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(11.0)
                .color(Color32::from_rgb(175, 182, 198)),
        );

        let wheel_size = 64.0;
        let (response, painter) =
            ui.allocate_painter(Vec2::splat(wheel_size), Sense::click_and_drag());
        let center = response.rect.center();
        let radius = wheel_size * 0.45;

        // Background disc
        painter.circle_filled(center, radius, Color32::from_rgb(20, 22, 28));

        // Hue ring segments
        let segments = 36;
        for i in 0..segments {
            let a0 = (i as f32 / segments as f32) * std::f32::consts::TAU;
            let a1 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
            let hue_deg = (i as f32 / segments as f32) * 360.0;
            let col = hsv_to_color32(hue_deg, 0.85, 0.85);
            let p0 = Pos2::new(center.x + a0.cos() * radius, center.y + a0.sin() * radius);
            let p1 = Pos2::new(center.x + a1.cos() * radius, center.y + a1.sin() * radius);
            painter.line_segment([p0, p1], Stroke::new(3.5, col));
        }

        // Crosshair
        painter.line_segment(
            [
                Pos2::new(center.x - radius + 4.0, center.y),
                Pos2::new(center.x + radius - 4.0, center.y),
            ],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 25)),
        );
        painter.line_segment(
            [
                Pos2::new(center.x, center.y - radius + 4.0),
                Pos2::new(center.x, center.y + radius - 4.0),
            ],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 25)),
        );

        // Double click resets wheel
        if response.double_clicked() {
            grade.hue = 0.0;
            grade.saturation = 0.0;
            changed = true;
            stopped = true;
        } else if response.dragged() || response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let dx = pos.x - center.x;
                let dy = pos.y - center.y;
                let dist = (dx * dx + dy * dy).sqrt();
                let sat = (dist / radius).clamp(0.0, 1.0);
                let angle_rad = dy.atan2(dx);
                let hue = angle_rad.to_degrees().rem_euclid(360.0);
                grade.hue = hue;
                grade.saturation = sat;
                changed = true;
            }
        }
        if response.drag_stopped() {
            stopped = true;
        }

        // Draw handle
        let angle = grade.hue.to_radians();
        let handle_r = grade.saturation.clamp(0.0, 1.0) * radius;
        let handle_pos = Pos2::new(
            center.x + angle.cos() * handle_r,
            center.y + angle.sin() * handle_r,
        );
        let handle_col = if grade.saturation > 0.01 {
            hsv_to_color32(grade.hue, grade.saturation.clamp(0.0, 1.0), 1.0)
        } else {
            Color32::from_rgb(200, 205, 215)
        };
        painter.circle_filled(handle_pos, 5.0, handle_col);
        painter.circle_stroke(handle_pos, 5.0, Stroke::new(1.5, Color32::WHITE));

        // Luminance slider underneath (compact so 3 wheels fit in 285px panel)
        ui.add_space(2.0);
        let prev_slider_w = ui.spacing().slider_width;
        ui.spacing_mut().slider_width = wheel_size;
        let lum_res = ui
            .add(egui::Slider::new(&mut grade.luminance, -1.0..=1.0).show_value(false))
            .on_hover_text(lum_label);
        ui.spacing_mut().slider_width = prev_slider_w;
        if lum_res.changed() {
            changed = true;
        }
        if lum_res.drag_stopped() {
            stopped = true;
        }
        let val_res = ui.add(
            egui::DragValue::new(&mut grade.luminance)
                .speed(0.01)
                .range(-1.0..=1.0)
                .fixed_decimals(2)
                .prefix(format!("{lum_label} ")),
        );
        if val_res.changed() {
            changed = true;
            stopped = true;
        }
    });

    (changed, stopped)
}
