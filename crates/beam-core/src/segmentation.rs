//! Local subject and sky auto-selection heuristics operating on RGBA32F image buffers.

use crate::sidecar::{MaskLayer, MaskShape};

/// Analyze an RGBA32F image buffer and generate a non-destructive Sky `MaskLayer`.
///
/// Inspects the upper 70% of the image for sky-like regions (blue/cyan chromaticity or
/// bright low-saturation overcast) and fits a vertical linear gradient + luminance/color mask.
pub fn detect_sky_mask(
    rgba: &[f32],
    width: u32,
    height: u32,
    mask_id: u64,
    name: String,
) -> MaskLayer {
    let mut layer = MaskLayer::new_linear(mask_id, name);
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return layer;
    }

    let step_x = (w / 64).max(1);
    let step_y = (h / 64).max(1);
    let max_y = (h * 70) / 100;

    let mut sky_rows = 0usize;
    let mut last_sky_y = (h / 3) as f32;
    let mut blue_sky_hits = 0usize;

    for y in (0..max_y).step_by(step_y) {
        let mut row_hits = 0usize;
        let mut row_samples = 0usize;
        for x in (0..w).step_by(step_x) {
            let idx = (y * w + x) * 4;
            let r = rgba[idx].max(0.0);
            let g = rgba[idx + 1].max(0.0);
            let b = rgba[idx + 2].max(0.0);
            let lum = 0.2627 * r + 0.6780 * g + 0.0593 * b;
            let is_blue_sky = b > r * 1.08 && b >= g * 0.92 && lum > 0.08;
            let is_bright_overcast = lum > 0.35 && (r - b).abs() < 0.08;
            if is_blue_sky {
                blue_sky_hits += 1;
            }
            if is_blue_sky || is_bright_overcast {
                row_hits += 1;
            }
            row_samples += 1;
        }
        if row_samples > 0 && (row_hits * 100 / row_samples) >= 35 {
            sky_rows += 1;
            last_sky_y = y as f32;
        }
    }

    let end_y = if sky_rows > 0 {
        ((last_sky_y / h as f32) + 0.10).clamp(0.25, 0.80)
    } else {
        0.55
    };

    layer.shape = MaskShape::LinearGradient {
        start: [0.5, 0.05],
        end: [0.5, end_y],
    };
    if blue_sky_hits > sky_rows * 4 {
        layer.color_range_enabled = true;
        layer.color_range_hue = 210.0;
        layer.color_range_tolerance = 55.0;
    } else {
        layer.luma_min = 0.35;
        layer.luma_max = 1.0;
        layer.luma_smoothness = 0.20;
    }
    layer
}

/// Analyze an RGBA32F image buffer and generate a non-destructive Subject `MaskLayer`.
///
/// Computes center-weighted local edge salience to locate the primary subject centroid
/// and extent, returning a fitted radial gradient mask.
pub fn detect_subject_mask(
    rgba: &[f32],
    width: u32,
    height: u32,
    mask_id: u64,
    name: String,
) -> MaskLayer {
    let mut layer = MaskLayer::new_radial(mask_id, name);
    let w = width as usize;
    let h = height as usize;
    if w < 4 || h < 4 || rgba.len() < w * h * 4 {
        return layer;
    }

    let step_x = (w / 64).max(1);
    let step_y = (h / 64).max(1);

    let luma_at = |x: usize, y: usize| -> f32 {
        let idx = (y * w + x) * 4;
        0.2627 * rgba[idx] + 0.6780 * rgba[idx + 1] + 0.0593 * rgba[idx + 2]
    };

    let mut sum_w = 0.0f64;
    let mut sum_x = 0.0f64;
    let mut sum_y = 0.0f64;
    let mut sum_x2 = 0.0f64;
    let mut sum_y2 = 0.0f64;

    for y in (1..(h - 1)).step_by(step_y) {
        let ny = y as f64 / h as f64;
        for x in (1..(w - 1)).step_by(step_x) {
            let nx = x as f64 / w as f64;
            let gx = (luma_at(x + 1, y) - luma_at(x - 1, y)).abs();
            let gy = (luma_at(x, y + 1) - luma_at(x, y - 1)).abs();
            let edge = (gx + gy) as f64;
            let dx_c = nx - 0.5;
            let dy_c = ny - 0.5;
            let center_prior = (-2.0 * (dx_c * dx_c + dy_c * dy_c)).exp();
            let weight = edge * center_prior;
            if weight > 1e-4 {
                sum_w += weight;
                sum_x += nx * weight;
                sum_y += ny * weight;
                sum_x2 += nx * nx * weight;
                sum_y2 += ny * ny * weight;
            }
        }
    }

    if sum_w > 1e-6 {
        let cx = (sum_x / sum_w).clamp(0.15, 0.85) as f32;
        let cy = (sum_y / sum_w).clamp(0.15, 0.85) as f32;
        let var_x = (sum_x2 / sum_w - (cx as f64).powi(2)).max(0.01);
        let var_y = (sum_y2 / sum_w - (cy as f64).powi(2)).max(0.01);
        let rx = (var_x.sqrt() as f32 * 1.6).clamp(0.15, 0.45);
        let ry = (var_y.sqrt() as f32 * 1.6).clamp(0.15, 0.45);
        layer.shape = MaskShape::RadialGradient {
            center: [cx, cy],
            radius: [rx, ry],
            feather: 0.55,
            invert: false,
        };
    }

    layer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_sky_and_subject_masks() {
        let w = 32u32;
        let h = 32u32;
        let mut rgba = vec![0.0f32; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 4) as usize;
                if y < 12 {
                    // Blue sky in top third
                    rgba[idx] = 0.15;
                    rgba[idx + 1] = 0.35;
                    rgba[idx + 2] = 0.75;
                } else if (x as i32 - 20).abs() <= 4 && (y as i32 - 20).abs() <= 4 {
                    // High-contrast subject block around (20/32, 20/32)
                    let checker = if (x + y) % 2 == 0 { 0.9 } else { 0.1 };
                    rgba[idx] = checker;
                    rgba[idx + 1] = checker;
                    rgba[idx + 2] = checker;
                } else {
                    rgba[idx] = 0.15;
                    rgba[idx + 1] = 0.18;
                    rgba[idx + 2] = 0.12;
                }
                rgba[idx + 3] = 1.0;
            }
        }

        let sky = detect_sky_mask(&rgba, w, h, 1, "Sky".into());
        assert!(sky.color_range_enabled);
        if let MaskShape::LinearGradient { end, .. } = sky.shape {
            assert!(
                end[1] > 0.25 && end[1] < 0.75,
                "Unexpected sky end_y={}",
                end[1]
            );
        } else {
            panic!("Expected LinearGradient for sky mask");
        }

        let subj = detect_subject_mask(&rgba, w, h, 2, "Subject".into());
        if let MaskShape::RadialGradient { center, .. } = subj.shape {
            assert!(
                center[0] > 0.45 && center[1] > 0.45,
                "Subject centroid should shift toward bottom-right subject (got {:?})",
                center
            );
        } else {
            panic!("Expected RadialGradient for subject mask");
        }
    }
}
