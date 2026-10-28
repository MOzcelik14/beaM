//! Interactive spline tone curve editor widget for beaM.

use beam_core::color::generate_tone_curve_lut;
use beam_core::sidecar::ToneCurvePoint;
use eframe::egui::{self, Color32, Pos2, Sense, Stroke, Vec2};

/// Renders the tone curve editor. Returns `true` if any point was modified, added, or deleted.
pub fn show_tone_curve(ui: &mut egui::Ui, points: &mut Vec<ToneCurvePoint>) -> bool {
    let mut changed = false;

    // Fixed or responsive square size
    let width = ui.available_width().clamp(180.0, 240.0);
    let size = Vec2::new(width, width);

    let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let rect = response.rect;

    // Background
    painter.rect_filled(rect, 4.0, Color32::from_rgb(22, 22, 25));
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, Color32::from_rgb(45, 45, 50)),
        egui::StrokeKind::Inside,
    );

    // Diagonal reference line (0,0) -> (1,1)
    painter.line_segment(
        [
            Pos2::new(rect.left(), rect.bottom()),
            Pos2::new(rect.right(), rect.top()),
        ],
        Stroke::new(1.0, Color32::from_rgb(38, 38, 44)),
    );

    // Grid lines (25%, 50%, 75%)
    let grid_stroke = Stroke::new(1.0, Color32::from_rgb(32, 32, 36));
    for i in 1..4 {
        let frac = i as f32 / 4.0;
        let x = rect.left() + rect.width() * frac;
        let y = rect.bottom() - rect.height() * frac;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            grid_stroke,
        );
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            grid_stroke,
        );
    }

    // Helper functions to map normalized coords [0, 1] to screen coords
    let to_screen = |norm: Pos2| -> Pos2 {
        Pos2::new(
            rect.left() + norm.x * rect.width(),
            rect.bottom() - norm.y * rect.height(),
        )
    };

    let to_norm = |screen: Pos2| -> Pos2 {
        Pos2::new(
            ((screen.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
            ((rect.bottom() - screen.y) / rect.height()).clamp(0.0, 1.0),
        )
    };

    // Ensure we have at least 2 endpoints
    if points.is_empty() {
        points.push(ToneCurvePoint { x: 0.0, y: 0.0 });
        points.push(ToneCurvePoint { x: 1.0, y: 1.0 });
        changed = true;
    }

    // Render smooth curve using 256-entry monotonic cubic spline LUT
    let lut = generate_tone_curve_lut(points);
    let mut curve_pts = Vec::with_capacity(256);
    for (i, &y_val) in lut.iter().enumerate() {
        let x_norm = i as f32 / 255.0;
        let y_norm = y_val.clamp(0.0, 1.0);
        curve_pts.push(to_screen(Pos2::new(x_norm, y_norm)));
    }
    painter.add(egui::Shape::line(
        curve_pts,
        Stroke::new(2.0, Color32::from_rgb(220, 220, 225)),
    ));

    // Drag tracking ID
    let drag_id = ui.scope_id().with("tone_curve_dragging_index");
    let mut dragging_index: Option<usize> = ui.data_mut(|d| d.get_temp(drag_id));

    // Handle mouse interactions
    let pointer_pos = response.interact_pointer_pos();

    if let Some(pos) = pointer_pos {
        let norm_pos = to_norm(pos);

        // Find nearest point
        let mut nearest_idx = None;
        let mut min_dist = f32::MAX;
        for (i, p) in points.iter().enumerate() {
            let screen_pt = to_screen(Pos2::new(p.x, p.y));
            let dist = screen_pt.distance(pos);
            if dist < min_dist {
                min_dist = dist;
                nearest_idx = Some(i);
            }
        }

        // Secondary click (right click) on intermediate point: delete point
        if response.secondary_clicked() {
            if let Some(idx) = nearest_idx {
                if min_dist < 14.0 && idx > 0 && idx < points.len() - 1 {
                    points.remove(idx);
                    changed = true;
                    dragging_index = None;
                }
            }
        } else if response.drag_started() {
            if min_dist < 14.0 {
                dragging_index = nearest_idx;
            } else if response.clicked() || response.drag_started() {
                // Add new point at clicked position
                let new_pt = ToneCurvePoint {
                    x: norm_pos.x.clamp(0.02, 0.98),
                    y: norm_pos.y,
                };
                points.push(new_pt);
                points.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
                // Find index of newly added point
                let new_idx = points
                    .iter()
                    .position(|p| (p.x - new_pt.x).abs() < 1e-4)
                    .unwrap_or(points.len() - 1);
                dragging_index = Some(new_idx);
                changed = true;
            }
        } else if response.dragged() {
            if let Some(idx) = dragging_index {
                if idx < points.len() {
                    let is_first = idx == 0;
                    let is_last = idx == points.len() - 1;

                    if is_first {
                        points[idx].x = 0.0;
                        points[idx].y = norm_pos.y;
                    } else if is_last {
                        points[idx].x = 1.0;
                        points[idx].y = norm_pos.y;
                    } else {
                        // Intermediate point: maintain bounds between neighbors
                        let min_x = points[idx - 1].x + 0.01;
                        let max_x = points[idx + 1].x - 0.01;
                        points[idx].x = norm_pos.x.clamp(min_x, max_x);
                        points[idx].y = norm_pos.y;
                    }
                    changed = true;
                }
            }
        }
    }

    if response.drag_stopped() {
        dragging_index = None;
    }

    ui.data_mut(|d| d.insert_temp(drag_id, dragging_index));

    // Draw control points
    for (i, p) in points.iter().enumerate() {
        let screen_pt = to_screen(Pos2::new(p.x, p.y));
        let is_being_dragged = dragging_index == Some(i);

        let radius = if is_being_dragged { 6.0 } else { 4.5 };
        let fill_col = if is_being_dragged {
            Color32::from_rgb(80, 160, 255)
        } else {
            Color32::from_rgb(20, 20, 22)
        };
        let stroke_col = if is_being_dragged {
            Color32::WHITE
        } else {
            Color32::from_rgb(210, 210, 215)
        };

        painter.circle_filled(screen_pt, radius, fill_col);
        painter.circle_stroke(screen_pt, radius, Stroke::new(1.5, stroke_col));
    }

    changed
}
