//! Color spaces and transformations for beaM.
//!
//! # Architecture & Working Color Space
//! beaM. processes all images internally in a **linear, wide-gamut working space: linear Rec.2020**.
//!
//! ## Rationale:
//! 1. **Linearity**: Photometric operations (exposure adjustments, white balance, contrast)
//!    physically operate on linear light intensity rather than perceptual non-linear encodings.
//!    Performing multiplication and convolution in a gamma-encoded space produces hue shifts
//!    and unnatural luminance distortions.
//! 2. **Wide Gamut (Rec.2020 / BT.2020)**: High-end camera sensors capture spectral responses
//!    that exceed standard sRGB and even AdobeRGB. By retaining linear Rec.2020 throughout
//!    the GPU pipeline, we preserve saturated highlights and deep shadows without clipping colors
//!    until the final display/export color transform.
//! 3. **Conversion**: For screen presentation, linear Rec.2020 is transformed into sRGB
//!    and encoded with the sRGB transfer function (EOTF) inside the display WGSL shader.

/// Standard 3x3 matrix multiplication: `result = M * v`.
#[inline]
pub fn mat3_mul_vec3(m: &[f32; 9], v: [f32; 3]) -> [f32; 3] {
    [
        m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
        m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
        m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
    ]
}

/// Multiply two 3x3 matrices in row-major order: `C = A * B`.
pub fn mat3_mul(a: &[f32; 9], b: &[f32; 9]) -> [f32; 9] {
    let mut out = [0.0f32; 9];
    for row in 0..3 {
        for col in 0..3 {
            out[row * 3 + col] =
                a[row * 3] * b[col] + a[row * 3 + 1] * b[3 + col] + a[row * 3 + 2] * b[6 + col];
        }
    }
    out
}

/// Linear Rec.2020 to CIE XYZ (D65 illuminant).
/// Reference: ITU-R BT.2020-2.
pub const REC2020_TO_XYZ_D65: [f32; 9] = [
    0.636958, 0.144617, 0.168881, 0.262700, 0.677998, 0.059302, 0.000000, 0.028073, 1.060985,
];

/// CIE XYZ to Linear Rec.2020 (D65 illuminant).
pub const XYZ_TO_REC2020_D65: [f32; 9] = [
    1.716651, -0.355671, -0.253366, -0.666684, 1.616481, 0.015769, 0.017640, -0.042771, 0.942103,
];

#[allow(clippy::excessive_precision)]
/// Linear sRGB to CIE XYZ (D65 illuminant).
/// Reference: IEC 61966-2-1:1999.
pub const SRGB_TO_XYZ_D65: [f32; 9] = [
    0.4124564, 0.3575761, 0.1804375, 0.2126729, 0.7151522, 0.0721750, 0.0193339, 0.1191920,
    0.9503041,
];

#[allow(clippy::excessive_precision)]
/// CIE XYZ to Linear sRGB (D65 illuminant).
pub const XYZ_TO_SRGB_D65: [f32; 9] = [
    3.2404542, -1.5371385, -0.4985314, -0.9692660, 1.8760108, 0.0415560, 0.0556434, -0.2040259,
    1.0572252,
];

#[allow(clippy::excessive_precision)]
/// Direct transformation matrix: Linear Rec.2020 -> Linear sRGB (D65).
/// Calculated by XYZ_TO_SRGB_D65 * REC2020_TO_XYZ_D65.
pub const REC2020_TO_SRGB: [f32; 9] = [
    1.6604910, -0.5876411, -0.0728499, -0.1245505, 1.1328999, -0.0083494, -0.0181508, -0.1005789,
    1.1187297,
];

/// Direct transformation matrix: Linear sRGB -> Linear Rec.2020 (D65).
pub const SRGB_TO_REC2020: [f32; 9] = [
    0.6274041, 0.3292821, 0.0433138, 0.0690972, 0.9195411, 0.0113617, 0.0163914, 0.0880133,
    0.8955953,
];

/// Converts a linear scalar channel [0.0, 1.0] to sRGB gamma-encoded scalar.
#[inline]
pub fn linear_to_srgb(x: f32) -> f32 {
    if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

/// Converts an sRGB gamma-encoded scalar [0.0, 1.0] to linear scalar.
#[inline]
pub fn srgb_to_linear(x: f32) -> f32 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// Converts a linear Rec.2020 RGB triplet to gamma-encoded sRGB.
pub fn rec2020_linear_to_srgb_display(rec2020: [f32; 3]) -> [f32; 3] {
    let linear_srgb = mat3_mul_vec3(&REC2020_TO_SRGB, rec2020);
    [
        linear_to_srgb(linear_srgb[0].clamp(0.0, 1.0)),
        linear_to_srgb(linear_srgb[1].clamp(0.0, 1.0)),
        linear_to_srgb(linear_srgb[2].clamp(0.0, 1.0)),
    ]
}

/// Converts temperature and tint slider offsets [-100.0, +100.0] into linear RGB gains.
///
/// - `temp_offset > 0` warms the image (boosts Red, reduces Blue).
/// - `temp_offset < 0` cools the image (boosts Blue, reduces Red).
/// - `tint_offset > 0` shifts towards Green.
/// - `tint_offset < 0` shifts towards Magenta.
pub fn white_balance_gains(temp_offset: f32, tint_offset: f32) -> [f32; 3] {
    let t = (temp_offset.clamp(-100.0, 100.0) / 100.0) * 0.5; // [-0.5, +0.5] EV range
    let tint = (tint_offset.clamp(-100.0, 100.0) / 100.0) * 0.5;

    let r_gain = 2.0f32.powf(t);
    let g_gain = 2.0f32.powf(tint);
    let b_gain = 2.0f32.powf(-t);

    [r_gain, g_gain, b_gain]
}

/// Converts a contrast slider value [-100.0, +100.0] into an exponential slope factor.
///
/// - `contrast = 0.0` -> slope = 1.0 (identity).
/// - `contrast = 100.0` -> slope = 2.0.
/// - `contrast = -100.0` -> slope = 0.5.
pub fn contrast_slope(contrast: f32) -> f32 {
    let c = contrast.clamp(-100.0, 100.0);
    2.0f32.powf(c / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_srgb_gamma_roundtrip() {
        for i in 0..=100 {
            let val = i as f32 / 100.0;
            let encoded = linear_to_srgb(val);
            let decoded = srgb_to_linear(encoded);
            assert!(
                (val - decoded).abs() < 1e-4,
                "Failed roundtrip for {val}: got {decoded}"
            );
        }
    }

    #[test]
    fn test_rec2020_srgb_matrix_inverse() {
        let identity = mat3_mul(&REC2020_TO_SRGB, &SRGB_TO_REC2020);
        for row in 0..3 {
            for col in 0..3 {
                let expected = if row == col { 1.0 } else { 0.0 };
                let val = identity[row * 3 + col];
                assert!(
                    (val - expected).abs() < 2e-3,
                    "Mismatch at ({row}, {col}): expected {expected}, got {val}"
                );
            }
        }
    }

    #[test]
    fn test_white_point_neutrality() {
        // D65 White point: [1.0, 1.0, 1.0] should map to [1.0, 1.0, 1.0]
        let srgb = rec2020_linear_to_srgb_display([1.0, 1.0, 1.0]);
        for c in srgb {
            assert!((c - 1.0).abs() < 1e-3, "White point shifted: got {c}");
        }
    }

    #[test]
    fn test_white_balance_neutral() {
        let gains = white_balance_gains(0.0, 0.0);
        assert!((gains[0] - 1.0).abs() < 1e-5);
        assert!((gains[1] - 1.0).abs() < 1e-5);
        assert!((gains[2] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_white_balance_warming_and_cooling() {
        let warm = white_balance_gains(50.0, 0.0);
        assert!(warm[0] > 1.0, "Red should be boosted when warming");
        assert!(warm[2] < 1.0, "Blue should be reduced when warming");

        let cool = white_balance_gains(-50.0, 0.0);
        assert!(cool[0] < 1.0, "Red should be reduced when cooling");
        assert!(cool[2] > 1.0, "Blue should be boosted when cooling");
    }

    #[test]
    fn test_contrast_slope() {
        assert_eq!(contrast_slope(0.0), 1.0);
        assert_eq!(contrast_slope(100.0), 2.0);
        assert_eq!(contrast_slope(-100.0), 0.5);
    }
}
