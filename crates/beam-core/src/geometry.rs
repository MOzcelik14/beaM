//! Coordinate geometry math for crop, rotation, flip, and straighten operations.

use crate::sidecar::{CropAdjustment, CropAspectPreset};

/// Calculate output pixel dimensions taking rotation and crop rectangle into account.
pub fn calculate_cropped_dimensions(orig_w: u32, orig_h: u32, crop: &CropAdjustment) -> (u32, u32) {
    if !crop.is_active() {
        return (orig_w, orig_h);
    }

    // 90 or 270 degree rotation swaps width and height
    let (base_w, base_h) = if crop.rotation == 90 || crop.rotation == 270 {
        (orig_h, orig_w)
    } else {
        (orig_w, orig_h)
    };

    let w = if crop.enabled {
        ((base_w as f64) * (crop.width as f64).clamp(0.01, 1.0)).round() as u32
    } else {
        base_w
    };

    let h = if crop.enabled {
        ((base_h as f64) * (crop.height as f64).clamp(0.01, 1.0)).round() as u32
    } else {
        base_h
    };

    (w.max(1), h.max(1))
}

/// Calculate the effective aspect ratio (width / height) taking rotation and crop into account.
pub fn calculate_effective_aspect_ratio(orig_w: u32, orig_h: u32, crop: &CropAdjustment) -> f32 {
    let (w, h) = calculate_cropped_dimensions(orig_w, orig_h, crop);
    (w as f32) / (h as f32).max(1.0)
}

/// Maps an output normalized UV (0..1) to source image UV (0..1)
/// taking crop rect, straighten, flip, and 90-degree rotations into account.
pub fn map_output_uv_to_source(u_out: f32, v_out: f32, crop: &CropAdjustment) -> (f32, f32) {
    // 1. Map into crop rectangle
    let (u1, v1) = if crop.enabled {
        (crop.x + u_out * crop.width, crop.y + v_out * crop.height)
    } else {
        (u_out, v_out)
    };

    // 2. Straighten rotation around crop center
    let (u2, v2) = if crop.straighten.abs() > 0.0001 {
        let (cu, cv) = if crop.enabled {
            (crop.x + crop.width * 0.5, crop.y + crop.height * 0.5)
        } else {
            (0.5, 0.5)
        };

        let rad = (-crop.straighten).to_radians();
        let cos_a = rad.cos();
        let sin_a = rad.sin();

        let du = u1 - cu;
        let dv = v1 - cv;

        (cu + du * cos_a - dv * sin_a, cv + du * sin_a + dv * cos_a)
    } else {
        (u1, v1)
    };

    // 3. Flips
    let u3 = if crop.flip_h { 1.0 - u2 } else { u2 };
    let v3 = if crop.flip_v { 1.0 - v2 } else { v2 };

    // 4. 90-degree rotations
    let (u4, v4) = match crop.rotation % 360 {
        90 => (v3, 1.0 - u3),
        180 => (1.0 - u3, 1.0 - v3),
        270 => (1.0 - v3, u3),
        _ => (u3, v3),
    };

    (u4.clamp(0.0, 1.0), v4.clamp(0.0, 1.0))
}

/// Helper to fit a crop rectangle to a specific aspect ratio while centering it within the available area.
pub fn create_centered_crop_rect(
    orig_aspect: f32,
    preset: CropAspectPreset,
) -> (f32, f32, f32, f32) {
    if let Some(target_ratio) = preset.ratio(orig_aspect) {
        if target_ratio > orig_aspect {
            // Target is wider than original: fit width, shrink height
            let w = 1.0f32;
            let h = (orig_aspect / target_ratio).clamp(0.05, 1.0);
            let y = (1.0 - h) * 0.5;
            (0.0, y, w, h)
        } else {
            // Target is taller than original: fit height, shrink width
            let h = 1.0f32;
            let w = (target_ratio / orig_aspect).clamp(0.05, 1.0);
            let x = (1.0 - w) * 0.5;
            (x, 0.0, w, h)
        }
    } else {
        (0.0, 0.0, 1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_cropped_dimensions() {
        let mut crop = CropAdjustment::default();
        assert_eq!(
            calculate_cropped_dimensions(6000, 4000, &crop),
            (6000, 4000)
        );

        // 90 deg rotation
        crop.rotation = 90;
        assert_eq!(
            calculate_cropped_dimensions(6000, 4000, &crop),
            (4000, 6000)
        );

        // 50% crop
        crop.enabled = true;
        crop.width = 0.5;
        crop.height = 0.5;
        assert_eq!(
            calculate_cropped_dimensions(6000, 4000, &crop),
            (2000, 3000)
        );
    }

    #[test]
    fn test_map_output_uv_to_source() {
        let mut crop = CropAdjustment::default();
        assert_eq!(map_output_uv_to_source(0.0, 0.0, &crop), (0.0, 0.0));
        assert_eq!(map_output_uv_to_source(1.0, 1.0, &crop), (1.0, 1.0));

        // Flip H
        crop.flip_h = true;
        assert_eq!(map_output_uv_to_source(0.0, 0.0, &crop), (1.0, 0.0));
        assert_eq!(map_output_uv_to_source(1.0, 1.0, &crop), (0.0, 1.0));

        // Rotation 180
        crop.flip_h = false;
        crop.rotation = 180;
        assert_eq!(map_output_uv_to_source(0.0, 0.0, &crop), (1.0, 1.0));
    }

    #[test]
    fn test_create_centered_crop_rect() {
        let (x, y, w, h) = create_centered_crop_rect(1.5, CropAspectPreset::Square);
        assert_eq!(w, 1.0 / 1.5);
        assert_eq!(h, 1.0);
        assert!((x - (1.0 - w) * 0.5).abs() < 1e-5);
        assert_eq!(y, 0.0);
    }
}
