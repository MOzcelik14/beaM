//! Interactive viewport overlay for Parametric Masks (Linear & Radial Gradients).

use beam_core::sidecar::{MaskLayer, MaskShape};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

fn norm_to_screen(p: [f32; 2], rect: Rect) -> Pos2 {
    Pos2::new(
        rect.left() + p[0] * rect.width(),
        rect.top() + p[1] * rect.height(),
    )
}

fn drag_handle(ui: &mut egui::Ui, id_salt: &str, pos: Pos2, fill: Color32) -> (Option<Vec2>, bool) {
    let hit_rect = Rect::from_center_size(pos, Vec2::splat(18.0));
    let id = ui.make_persistent_id(id_salt);
    let resp = ui.interact(hit_rect, id, Sense::drag());
    let painter = ui.painter();
    let r = if resp.hovered() || resp.dragged() {
        6.5
    } else {
        5.0
    };
    painter.circle_filled(pos, r, fill);
    painter.circle_stroke(pos, r, Stroke::new(1.5, Color32::WHITE));
    let delta = if resp.dragged() {
        Some(resp.drag_delta())
    } else {
        None
    };
    (delta, resp.drag_stopped())
}

/// Renders and handles interactive dragging for the selected mask on `image_rect`.
/// Returns `(changed, drag_stopped)`.
pub fn show_mask_overlay(
    ui: &mut egui::Ui,
    image_rect: Rect,
    mask: &mut MaskLayer,
) -> (bool, bool) {
    let mut changed = false;
    let mut stopped = false;
    let w = image_rect.width().max(1.0);
    let h = image_rect.height().max(1.0);
    let painter = ui.painter();
    let accent = Color32::from_rgb(59, 130, 246);

    match &mut mask.shape {
        MaskShape::LinearGradient { start, end } => {
            let p_start = norm_to_screen(*start, image_rect);
            let p_end = norm_to_screen(*end, image_rect);
            let p_mid = Pos2::new((p_start.x + p_end.x) * 0.5, (p_start.y + p_end.y) * 0.5);

            painter.line_segment(
                [p_start, p_end],
                Stroke::new(1.5, Color32::from_rgba_unmultiplied(255, 255, 255, 190)),
            );

            // Perpendicular boundary indicators
            let dir = p_end - p_start;
            let len = dir.length();
            if len > 1.0 {
                let perp = Vec2::new(-dir.y / len, dir.x / len) * 28.0;
                painter.line_segment(
                    [p_start - perp, p_start + perp],
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(59, 130, 246, 170)),
                );
                painter.line_segment(
                    [p_end - perp, p_end + perp],
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(220, 225, 235, 140)),
                );
            }

            let (d_start, s_start) = drag_handle(ui, "mask_lin_start", p_start, accent);
            if let Some(d) = d_start {
                start[0] = (start[0] + d.x / w).clamp(-0.5, 1.5);
                start[1] = (start[1] + d.y / h).clamp(-0.5, 1.5);
                changed = true;
            }
            stopped |= s_start;

            let (d_end, s_end) =
                drag_handle(ui, "mask_lin_end", p_end, Color32::from_rgb(80, 86, 102));
            if let Some(d) = d_end {
                end[0] = (end[0] + d.x / w).clamp(-0.5, 1.5);
                end[1] = (end[1] + d.y / h).clamp(-0.5, 1.5);
                changed = true;
            }
            stopped |= s_end;

            let (d_mid, s_mid) =
                drag_handle(ui, "mask_lin_mid", p_mid, Color32::from_rgb(30, 34, 42));
            if let Some(d) = d_mid {
                start[0] = (start[0] + d.x / w).clamp(-0.5, 1.5);
                start[1] = (start[1] + d.y / h).clamp(-0.5, 1.5);
                end[0] = (end[0] + d.x / w).clamp(-0.5, 1.5);
                end[1] = (end[1] + d.y / h).clamp(-0.5, 1.5);
                changed = true;
            }
            stopped |= s_mid;
        }
        MaskShape::RadialGradient {
            center,
            radius,
            feather,
            ..
        } => {
            let p_center = norm_to_screen(*center, image_rect);
            let rx = radius[0] * w;
            let ry = radius[1] * h;

            let draw_ellipse = |scale: f32, color: Color32| {
                let segs = 48;
                let mut pts = Vec::with_capacity(segs + 1);
                for i in 0..=segs {
                    let a = (i as f32 / segs as f32) * std::f32::consts::TAU;
                    pts.push(Pos2::new(
                        p_center.x + a.cos() * rx * scale,
                        p_center.y + a.sin() * ry * scale,
                    ));
                }
                painter.add(egui::Shape::line(pts, Stroke::new(1.5, color)));
            };

            draw_ellipse(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 190));
            let inner = (1.0 - feather.clamp(0.0, 1.0)).max(0.05);
            draw_ellipse(inner, Color32::from_rgba_unmultiplied(59, 130, 246, 150));

            let (d_c, s_c) = drag_handle(ui, "mask_rad_center", p_center, accent);
            if let Some(d) = d_c {
                center[0] = (center[0] + d.x / w).clamp(-0.5, 1.5);
                center[1] = (center[1] + d.y / h).clamp(-0.5, 1.5);
                changed = true;
            }
            stopped |= s_c;

            let p_rx = Pos2::new(p_center.x + rx, p_center.y);
            let (d_rx, s_rx) = drag_handle(ui, "mask_rad_rx", p_rx, Color32::from_rgb(30, 34, 42));
            if let Some(d) = d_rx {
                radius[0] = (radius[0] + d.x / w).clamp(0.03, 1.5);
                changed = true;
            }
            stopped |= s_rx;

            let p_ry = Pos2::new(p_center.x, p_center.y + ry);
            let (d_ry, s_ry) = drag_handle(ui, "mask_rad_ry", p_ry, Color32::from_rgb(30, 34, 42));
            if let Some(d) = d_ry {
                radius[1] = (radius[1] + d.y / h).clamp(0.03, 1.5);
                changed = true;
            }
            stopped |= s_ry;
        }
        MaskShape::Brush {
            strokes, radius, ..
        } => {
            let px_r = (*radius * w.min(h)).max(4.0);
            for pt in strokes.iter() {
                let sp = norm_to_screen(*pt, image_rect);
                painter.circle_stroke(
                    sp,
                    px_r,
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(59, 130, 246, 90)),
                );
            }
            let paint_resp = ui.interact(
                image_rect,
                ui.scope_id().with("mask_brush_canvas"),
                egui::Sense::drag(),
            );
            if paint_resp.dragged() {
                if let Some(pos) = paint_resp.interact_pointer_pos() {
                    let u = ((pos.x - image_rect.left()) / w).clamp(0.0, 1.0);
                    let v = ((pos.y - image_rect.top()) / h).clamp(0.0, 1.0);
                    let min_step = (*radius * 0.25).max(0.005);
                    let far_enough = strokes.last().is_none_or(|last| {
                        let dx = last[0] - u;
                        let dy = last[1] - v;
                        (dx * dx + dy * dy).sqrt() >= min_step
                    });
                    if far_enough && strokes.len() < 128 {
                        strokes.push([u, v]);
                        changed = true;
                    }
                }
            }
            if paint_resp.drag_stopped() {
                stopped = true;
            }
        }
    }

    (changed, stopped)
}
