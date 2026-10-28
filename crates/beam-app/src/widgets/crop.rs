//! Interactive Crop & Straighten overlay widget with handles and rule-of-thirds grid.

use beam_core::sidecar::CropAdjustment;
use eframe::egui::{self, Color32, CursorIcon, Pos2, Rect, Stroke, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragHandle {
    None,
    Move,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
    Left,
    Right,
}

pub fn show_crop_overlay(
    ui: &mut egui::Ui,
    image_rect: Rect,
    crop: &mut CropAdjustment,
    orig_aspect: f32,
) -> bool {
    let mut changed = false;

    // Normalized to screen coordinates
    let crop_min_x = image_rect.min.x + crop.x.clamp(0.0, 1.0) * image_rect.width();
    let crop_min_y = image_rect.min.y + crop.y.clamp(0.0, 1.0) * image_rect.height();
    let crop_max_x = crop_min_x + crop.width.clamp(0.05, 1.0) * image_rect.width();
    let crop_max_y = crop_min_y + crop.height.clamp(0.05, 1.0) * image_rect.height();

    let crop_screen_rect = Rect::from_min_max(
        Pos2::new(crop_min_x, crop_min_y),
        Pos2::new(crop_max_x, crop_max_y),
    );

    // 1. Paint Dimmed Exterior (outside crop rectangle)
    let painter = ui.painter();
    let dim_color = Color32::from_black_alpha(150);

    // Top
    if crop_screen_rect.min.y > image_rect.min.y {
        painter.rect_filled(
            Rect::from_min_max(
                image_rect.min,
                Pos2::new(image_rect.max.x, crop_screen_rect.min.y),
            ),
            0.0,
            dim_color,
        );
    }
    // Bottom
    if crop_screen_rect.max.y < image_rect.max.y {
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(image_rect.min.x, crop_screen_rect.max.y),
                image_rect.max,
            ),
            0.0,
            dim_color,
        );
    }
    // Left
    if crop_screen_rect.min.x > image_rect.min.x {
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(image_rect.min.x, crop_screen_rect.min.y),
                Pos2::new(crop_screen_rect.min.x, crop_screen_rect.max.y),
            ),
            0.0,
            dim_color,
        );
    }
    // Right
    if crop_screen_rect.max.x < image_rect.max.x {
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(crop_screen_rect.max.x, crop_screen_rect.min.y),
                Pos2::new(image_rect.max.x, crop_screen_rect.max.y),
            ),
            0.0,
            dim_color,
        );
    }

    // 2. Paint Crop Border & Rule of Thirds Grid
    let border_stroke = Stroke::new(1.5, Color32::WHITE);
    painter.rect_stroke(
        crop_screen_rect,
        0.0,
        border_stroke,
        egui::StrokeKind::Inside,
    );

    let grid_stroke = Stroke::new(1.0, Color32::from_white_alpha(90));
    let w_third = crop_screen_rect.width() / 3.0;
    let h_third = crop_screen_rect.height() / 3.0;

    // Vertical third lines
    painter.line_segment(
        [
            Pos2::new(crop_screen_rect.min.x + w_third, crop_screen_rect.min.y),
            Pos2::new(crop_screen_rect.min.x + w_third, crop_screen_rect.max.y),
        ],
        grid_stroke,
    );
    painter.line_segment(
        [
            Pos2::new(
                crop_screen_rect.min.x + 2.0 * w_third,
                crop_screen_rect.min.y,
            ),
            Pos2::new(
                crop_screen_rect.min.x + 2.0 * w_third,
                crop_screen_rect.max.y,
            ),
        ],
        grid_stroke,
    );

    // Horizontal third lines
    painter.line_segment(
        [
            Pos2::new(crop_screen_rect.min.x, crop_screen_rect.min.y + h_third),
            Pos2::new(crop_screen_rect.max.x, crop_screen_rect.min.y + h_third),
        ],
        grid_stroke,
    );
    painter.line_segment(
        [
            Pos2::new(
                crop_screen_rect.min.x,
                crop_screen_rect.min.y + 2.0 * h_third,
            ),
            Pos2::new(
                crop_screen_rect.max.x,
                crop_screen_rect.min.y + 2.0 * h_third,
            ),
        ],
        grid_stroke,
    );

    // 3. Paint Corner and Edge Handles (L-brackets & edge bars)
    let handle_stroke = Stroke::new(3.0, Color32::WHITE);
    let corner_len = 16.0f32
        .min(crop_screen_rect.width() * 0.3)
        .min(crop_screen_rect.height() * 0.3);

    // Top-Left
    painter.line_segment(
        [
            crop_screen_rect.min,
            Pos2::new(crop_screen_rect.min.x + corner_len, crop_screen_rect.min.y),
        ],
        handle_stroke,
    );
    painter.line_segment(
        [
            crop_screen_rect.min,
            Pos2::new(crop_screen_rect.min.x, crop_screen_rect.min.y + corner_len),
        ],
        handle_stroke,
    );

    // Top-Right
    let tr = Pos2::new(crop_screen_rect.max.x, crop_screen_rect.min.y);
    painter.line_segment([tr, Pos2::new(tr.x - corner_len, tr.y)], handle_stroke);
    painter.line_segment([tr, Pos2::new(tr.x, tr.y + corner_len)], handle_stroke);

    // Bottom-Left
    let bl = Pos2::new(crop_screen_rect.min.x, crop_screen_rect.max.y);
    painter.line_segment([bl, Pos2::new(bl.x + corner_len, bl.y)], handle_stroke);
    painter.line_segment([bl, Pos2::new(bl.x, bl.y - corner_len)], handle_stroke);

    // Bottom-Right
    let br = crop_screen_rect.max;
    painter.line_segment([br, Pos2::new(br.x - corner_len, br.y)], handle_stroke);
    painter.line_segment([br, Pos2::new(br.x, br.y - corner_len)], handle_stroke);

    // Edge Center Bars
    let center_x = crop_screen_rect.center().x;
    let center_y = crop_screen_rect.center().y;
    let bar_len = 14.0f32
        .min(crop_screen_rect.width() * 0.2)
        .min(crop_screen_rect.height() * 0.2);

    // Top edge
    painter.line_segment(
        [
            Pos2::new(center_x - bar_len, crop_screen_rect.min.y),
            Pos2::new(center_x + bar_len, crop_screen_rect.min.y),
        ],
        handle_stroke,
    );
    // Bottom edge
    painter.line_segment(
        [
            Pos2::new(center_x - bar_len, crop_screen_rect.max.y),
            Pos2::new(center_x + bar_len, crop_screen_rect.max.y),
        ],
        handle_stroke,
    );
    // Left edge
    painter.line_segment(
        [
            Pos2::new(crop_screen_rect.min.x, center_y - bar_len),
            Pos2::new(crop_screen_rect.min.x, center_y + bar_len),
        ],
        handle_stroke,
    );
    // Right edge
    painter.line_segment(
        [
            Pos2::new(crop_screen_rect.max.x, center_y - bar_len),
            Pos2::new(crop_screen_rect.max.x, center_y + bar_len),
        ],
        handle_stroke,
    );

    // 4. Handle Drag Interaction
    let id = ui.scope_id().with("beam_crop_overlay_interaction");
    let mut active_handle: DragHandle = ui
        .ctx()
        .data(|d| d.get_temp(id).unwrap_or(DragHandle::None));

    let mouse_pos = ui.input(|i| i.pointer.hover_pos());
    let mouse_down = ui.input(|i| i.pointer.primary_down());
    let mouse_released = ui.input(|i| i.pointer.any_released());

    let hit_dist = 14.0;

    if active_handle == DragHandle::None {
        if let Some(p) = mouse_pos {
            // Check corners
            let is_tl = (p - crop_screen_rect.min).length() < hit_dist;
            let is_tr = (p - tr).length() < hit_dist;
            let is_bl = (p - bl).length() < hit_dist;
            let is_br = (p - br).length() < hit_dist;

            // Check edges
            let is_top = !is_tl
                && !is_tr
                && (p.y - crop_screen_rect.min.y).abs() < hit_dist
                && p.x >= crop_screen_rect.min.x
                && p.x <= crop_screen_rect.max.x;
            let is_bottom = !is_bl
                && !is_br
                && (p.y - crop_screen_rect.max.y).abs() < hit_dist
                && p.x >= crop_screen_rect.min.x
                && p.x <= crop_screen_rect.max.x;
            let is_left = !is_tl
                && !is_bl
                && (p.x - crop_screen_rect.min.x).abs() < hit_dist
                && p.y >= crop_screen_rect.min.y
                && p.y <= crop_screen_rect.max.y;
            let is_right = !is_tr
                && !is_br
                && (p.x - crop_screen_rect.max.x).abs() < hit_dist
                && p.y >= crop_screen_rect.min.y
                && p.y <= crop_screen_rect.max.y;

            let is_inside = crop_screen_rect.contains(p);

            let hovered = if is_tl {
                ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeNorthWest);
                DragHandle::TopLeft
            } else if is_tr {
                ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeNorthEast);
                DragHandle::TopRight
            } else if is_bl {
                ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeSouthWest);
                DragHandle::BottomLeft
            } else if is_br {
                ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeSouthEast);
                DragHandle::BottomRight
            } else if is_top {
                ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeNorth);
                DragHandle::Top
            } else if is_bottom {
                ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeSouth);
                DragHandle::Bottom
            } else if is_left {
                ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeWest);
                DragHandle::Left
            } else if is_right {
                ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeEast);
                DragHandle::Right
            } else if is_inside {
                ui.output_mut(|o| o.cursor_icon = CursorIcon::Grab);
                DragHandle::Move
            } else {
                DragHandle::None
            };

            if mouse_down && hovered != DragHandle::None {
                active_handle = hovered;
                ui.ctx().data_mut(|d| d.insert_temp(id, active_handle));
            }
        }
    }

    if active_handle != DragHandle::None {
        if mouse_released || !mouse_down {
            ui.ctx().data_mut(|d| d.insert_temp(id, DragHandle::None));
        } else if let Some(delta) = ui.input(|i| Some(i.pointer.delta())) {
            if delta != Vec2::ZERO {
                let dx = delta.x / image_rect.width().max(1.0);
                let dy = delta.y / image_rect.height().max(1.0);

                let ratio_opt = crop.preset.ratio(orig_aspect);

                match active_handle {
                    DragHandle::Move => {
                        let new_x = (crop.x + dx).clamp(0.0, 1.0 - crop.width);
                        let new_y = (crop.y + dy).clamp(0.0, 1.0 - crop.height);
                        if new_x != crop.x || new_y != crop.y {
                            crop.x = new_x;
                            crop.y = new_y;
                            crop.enabled = true;
                            changed = true;
                        }
                    }
                    DragHandle::TopLeft => {
                        let mut new_x = (crop.x + dx).clamp(0.0, crop.x + crop.width - 0.05);
                        let mut new_y = (crop.y + dy).clamp(0.0, crop.y + crop.height - 0.05);
                        let mut new_w = crop.width + (crop.x - new_x);
                        let mut new_h = crop.height + (crop.y - new_y);

                        if let Some(r) = ratio_opt {
                            let desired_h = new_w * orig_aspect / r;
                            new_y = (crop.y + crop.height - desired_h).max(0.0);
                            new_h = crop.height + (crop.y - new_y);
                            new_w = new_h * r / orig_aspect;
                            new_x = crop.x + crop.width - new_w;
                        }

                        if new_x >= 0.0 && new_y >= 0.0 && new_w >= 0.05 && new_h >= 0.05 {
                            crop.x = new_x;
                            crop.y = new_y;
                            crop.width = new_w;
                            crop.height = new_h;
                            crop.enabled = true;
                            changed = true;
                        }
                    }
                    DragHandle::BottomRight => {
                        let mut new_w = (crop.width + dx).clamp(0.05, 1.0 - crop.x);
                        let mut new_h = (crop.height + dy).clamp(0.05, 1.0 - crop.y);

                        if let Some(r) = ratio_opt {
                            new_h = (new_w * orig_aspect / r).min(1.0 - crop.y);
                            new_w = new_h * r / orig_aspect;
                        }

                        if new_w >= 0.05 && new_h >= 0.05 {
                            crop.width = new_w;
                            crop.height = new_h;
                            crop.enabled = true;
                            changed = true;
                        }
                    }
                    DragHandle::TopRight => {
                        let mut new_w = (crop.width + dx).clamp(0.05, 1.0 - crop.x);
                        let mut new_y = (crop.y + dy).clamp(0.0, crop.y + crop.height - 0.05);
                        let mut new_h = crop.height + (crop.y - new_y);

                        if let Some(r) = ratio_opt {
                            let desired_h = new_w * orig_aspect / r;
                            new_y = (crop.y + crop.height - desired_h).max(0.0);
                            new_h = crop.height + (crop.y - new_y);
                            new_w = new_h * r / orig_aspect;
                        }

                        if new_y >= 0.0 && new_w >= 0.05 && new_h >= 0.05 {
                            crop.y = new_y;
                            crop.width = new_w;
                            crop.height = new_h;
                            crop.enabled = true;
                            changed = true;
                        }
                    }
                    DragHandle::BottomLeft => {
                        let mut new_x = (crop.x + dx).clamp(0.0, crop.x + crop.width - 0.05);
                        let mut new_w = crop.width + (crop.x - new_x);
                        let mut new_h = (crop.height + dy).clamp(0.05, 1.0 - crop.y);

                        if let Some(r) = ratio_opt {
                            new_h = (new_w * orig_aspect / r).min(1.0 - crop.y);
                            new_w = new_h * r / orig_aspect;
                            new_x = crop.x + crop.width - new_w;
                        }

                        if new_x >= 0.0 && new_w >= 0.05 && new_h >= 0.05 {
                            crop.x = new_x;
                            crop.width = new_w;
                            crop.height = new_h;
                            crop.enabled = true;
                            changed = true;
                        }
                    }
                    DragHandle::Top => {
                        let new_y = (crop.y + dy).clamp(0.0, crop.y + crop.height - 0.05);
                        let new_h = crop.height + (crop.y - new_y);
                        if ratio_opt.is_none() && new_h >= 0.05 {
                            crop.y = new_y;
                            crop.height = new_h;
                            crop.enabled = true;
                            changed = true;
                        }
                    }
                    DragHandle::Bottom => {
                        let new_h = (crop.height + dy).clamp(0.05, 1.0 - crop.y);
                        if ratio_opt.is_none() && new_h >= 0.05 {
                            crop.height = new_h;
                            crop.enabled = true;
                            changed = true;
                        }
                    }
                    DragHandle::Left => {
                        let new_x = (crop.x + dx).clamp(0.0, crop.x + crop.width - 0.05);
                        let new_w = crop.width + (crop.x - new_x);
                        if ratio_opt.is_none() && new_w >= 0.05 {
                            crop.x = new_x;
                            crop.width = new_w;
                            crop.enabled = true;
                            changed = true;
                        }
                    }
                    DragHandle::Right => {
                        let new_w = (crop.width + dx).clamp(0.05, 1.0 - crop.x);
                        if ratio_opt.is_none() && new_w >= 0.05 {
                            crop.width = new_w;
                            crop.enabled = true;
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    changed
}
