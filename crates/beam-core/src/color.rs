use crate::sidecar::ToneCurvePoint;
use serde::{Deserialize, Serialize};

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
pub const REC2020_TO_XYZ_D65: [f32; 9] = [
    0.636958, 0.144617, 0.168881, 0.262700, 0.677998, 0.059302, 0.000000, 0.028073, 1.060985,
];

/// CIE XYZ to Linear Rec.2020 (D65 illuminant).
pub const XYZ_TO_REC2020_D65: [f32; 9] = [
    1.716651, -0.355671, -0.253366, -0.666684, 1.616481, 0.015769, 0.017640, -0.042771, 0.942103,
];

#[allow(clippy::excessive_precision)]
/// Linear sRGB to CIE XYZ (D65 illuminant).
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
pub const REC2020_TO_SRGB: [f32; 9] = [
    1.6604910, -0.5876411, -0.0728499, -0.1245505, 1.1328999, -0.0083494, -0.0181508, -0.1005789,
    1.1187297,
];

/// Direct transformation matrix: Linear sRGB -> Linear Rec.2020 (D65).
pub const SRGB_TO_REC2020: [f32; 9] = [
    0.6274041, 0.3292821, 0.0433138, 0.0690972, 0.9195411, 0.0113617, 0.0163914, 0.0880133,
    0.8955953,
];

#[allow(clippy::excessive_precision)]
/// Direct transformation matrix: Linear Rec.2020 -> Linear Display P3 (D65).
pub const REC2020_TO_DISPLAY_P3: [f32; 9] = [
    1.3435774, -0.2821798, -0.0613987, -0.0652972, 1.0757884, -0.0104912, 0.0028224, -0.0195982,
    1.0167768,
];

#[allow(clippy::excessive_precision)]
/// Direct transformation matrix: Linear Rec.2020 -> Linear Adobe RGB (1998) (D65).
pub const REC2020_TO_ADOBE_RGB: [f32; 9] = [
    1.4883403, -0.4042402, -0.0841001, -0.0827202, 1.0924901, -0.0097701, 0.0068800, -0.0381300,
    1.0312500,
];

/// Supported export color spaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ExportColorSpace {
    #[default]
    Srgb,
    DisplayP3,
    AdobeRgb,
}

impl ExportColorSpace {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Srgb => "sRGB",
            Self::DisplayP3 => "Display P3",
            Self::AdobeRgb => "Adobe RGB",
        }
    }

    pub fn translation_key(&self) -> &'static str {
        match self {
            Self::Srgb => "export_colorspace_srgb",
            Self::DisplayP3 => "export_colorspace_display_p3",
            Self::AdobeRgb => "export_colorspace_adobe_rgb",
        }
    }
}

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

/// Converts a linear scalar channel [0.0, 1.0] to Adobe RGB (1998) pure-gamma 2.19921875 scalar.
#[inline]
pub fn linear_to_adobe_rgb(x: f32) -> f32 {
    x.clamp(0.0, 1.0).powf(256.0 / 563.0)
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

/// Converts a linear Rec.2020 RGB triplet to gamma-encoded Display P3.
pub fn rec2020_linear_to_display_p3(rec2020: [f32; 3]) -> [f32; 3] {
    let lin_p3 = mat3_mul_vec3(&REC2020_TO_DISPLAY_P3, rec2020);
    [
        linear_to_srgb(lin_p3[0].clamp(0.0, 1.0)),
        linear_to_srgb(lin_p3[1].clamp(0.0, 1.0)),
        linear_to_srgb(lin_p3[2].clamp(0.0, 1.0)),
    ]
}

/// Converts a linear Rec.2020 RGB triplet to gamma-encoded Adobe RGB (1998).
pub fn rec2020_linear_to_adobe_rgb(rec2020: [f32; 3]) -> [f32; 3] {
    let lin_adobe = mat3_mul_vec3(&REC2020_TO_ADOBE_RGB, rec2020);
    [
        linear_to_adobe_rgb(lin_adobe[0]),
        linear_to_adobe_rgb(lin_adobe[1]),
        linear_to_adobe_rgb(lin_adobe[2]),
    ]
}

/// Converts a linear Rec.2020 RGB triplet to the requested export color space.
pub fn rec2020_linear_to_color_space(rec2020: [f32; 3], cs: ExportColorSpace) -> [f32; 3] {
    match cs {
        ExportColorSpace::Srgb => rec2020_linear_to_srgb_display(rec2020),
        ExportColorSpace::DisplayP3 => rec2020_linear_to_display_p3(rec2020),
        ExportColorSpace::AdobeRgb => rec2020_linear_to_adobe_rgb(rec2020),
    }
}

/// Converts temperature and tint slider offsets [-100.0, +100.0] into linear RGB gains.
pub fn white_balance_gains(temp_offset: f32, tint_offset: f32) -> [f32; 3] {
    let t = (temp_offset.clamp(-100.0, 100.0) / 100.0) * 0.5;
    let tint = (tint_offset.clamp(-100.0, 100.0) / 100.0) * 0.5;

    let r_gain = 2.0f32.powf(t);
    let g_gain = 2.0f32.powf(tint);
    let b_gain = 2.0f32.powf(-t);

    [r_gain, g_gain, b_gain]
}

/// Converts a contrast slider value [-100.0, +100.0] into an exponential slope factor.
pub fn contrast_slope(contrast: f32) -> f32 {
    let c = contrast.clamp(-100.0, 100.0);
    2.0f32.powf(c / 100.0)
}

// ----------------------------------------------------------------------------
// Tone Curve Spline Interpolation (Monotone Cubic / Fritsch-Carlson)
// ----------------------------------------------------------------------------

/// Evaluates a 256-sample lookup table from a list of ToneCurvePoints using monotone cubic spline interpolation.
pub fn generate_tone_curve_lut(points: &[ToneCurvePoint]) -> [f32; 256] {
    if points.is_empty() {
        let mut lut = [0.0f32; 256];
        for (i, item) in lut.iter_mut().enumerate() {
            *item = i as f32 / 255.0;
        }
        return lut;
    }

    // Sort and deduplicate control points by x coordinate
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    pts.dedup_by(|a, b| (a.x - b.x).abs() < 1e-4);

    // Ensure boundary points at x=0 and x=1 exist
    if pts[0].x > 0.0 {
        let first_y = pts[0].y;
        pts.insert(0, ToneCurvePoint { x: 0.0, y: first_y });
    }
    if pts.last().unwrap().x < 1.0 {
        let last_y = pts.last().unwrap().y;
        pts.push(ToneCurvePoint { x: 1.0, y: last_y });
    }

    let n = pts.len();
    if n == 2 {
        let mut lut = [0.0f32; 256];
        let p0 = pts[0];
        let p1 = pts[1];
        let dx = (p1.x - p0.x).max(1e-5);
        for (i, item) in lut.iter_mut().enumerate() {
            let x = i as f32 / 255.0;
            let t = ((x - p0.x) / dx).clamp(0.0, 1.0);
            *item = (p0.y + t * (p1.y - p0.y)).clamp(0.0, 1.0);
        }
        return lut;
    }

    // Compute segment slopes delta_k
    let mut deltas = Vec::with_capacity(n - 1);
    let mut h = Vec::with_capacity(n - 1);
    for i in 0..(n - 1) {
        let dx = (pts[i + 1].x - pts[i].x).max(1e-5);
        let dy = pts[i + 1].y - pts[i].y;
        deltas.push(dy / dx);
        h.push(dx);
    }

    // Tangents m_k (Fritsch-Carlson initialization)
    let mut m = vec![0.0f32; n];
    m[0] = deltas[0];
    m[n - 1] = deltas[n - 2];
    for i in 1..(n - 1) {
        m[i] = (deltas[i - 1] + deltas[i]) * 0.5;
    }

    // Monotonicity enforcement
    for i in 0..(n - 1) {
        if deltas[i].abs() < 1e-6 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
        } else {
            let alpha = m[i] / deltas[i];
            let beta = m[i + 1] / deltas[i];
            let dist = alpha * alpha + beta * beta;
            if dist > 9.0 {
                let tau = 3.0 / dist.sqrt();
                m[i] = tau * alpha * deltas[i];
                m[i + 1] = tau * beta * deltas[i];
            }
        }
    }

    // Sample into 256 LUT entries using cubic Hermite basis
    let mut lut = [0.0f32; 256];
    let mut seg = 0;
    for (i, item) in lut.iter_mut().enumerate() {
        let x = i as f32 / 255.0;

        while seg < n - 2 && x > pts[seg + 1].x {
            seg += 1;
        }

        let x0 = pts[seg].x;
        let x1 = pts[seg + 1].x;
        let y0 = pts[seg].y;
        let y1 = pts[seg + 1].y;
        let dx = (x1 - x0).max(1e-5);
        let t = ((x - x0) / dx).clamp(0.0, 1.0);
        let t2 = t * t;
        let t3 = t2 * t;

        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;

        let y = h00 * y0 + h10 * dx * m[seg] + h01 * y1 + h11 * dx * m[seg + 1];
        *item = y.clamp(0.0, 1.0);
    }

    lut
}

// ----------------------------------------------------------------------------
// RGB ↔ HSL Conversion Math
// ----------------------------------------------------------------------------

/// Convert RGB [0.0, 1.0] to HSL (Hue in [0.0, 360.0], Saturation in [0.0, 1.0], Luminance in [0.0, 1.0]).
pub fn rgb_to_hsl(rgb: [f32; 3]) -> [f32; 3] {
    let r = rgb[0].clamp(0.0, 1.0);
    let g = rgb[1].clamp(0.0, 1.0);
    let b = rgb[2].clamp(0.0, 1.0);

    let max = r.max(g.max(b));
    let min = r.min(g.min(b));
    let delta = max - min;
    let l = (max + min) * 0.5;

    if delta < 1e-5 {
        return [0.0, 0.0, l];
    }

    let s = if l < 0.5 {
        delta / (max + min)
    } else {
        delta / (2.0 - max - min)
    };

    let mut h = if (max - r).abs() < 1e-5 {
        ((g - b) / delta).rem_euclid(6.0)
    } else if (max - g).abs() < 1e-5 {
        ((b - r) / delta) + 2.0
    } else {
        ((r - g) / delta) + 4.0
    };
    h *= 60.0;
    if h < 0.0 {
        h += 360.0;
    }

    [h, s, l]
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 1.0 / 2.0 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

/// Convert HSL (Hue in [0.0, 360.0], Saturation in [0.0, 1.0], Luminance in [0.0, 1.0]) to RGB [0.0, 1.0].
pub fn hsl_to_rgb(hsl: [f32; 3]) -> [f32; 3] {
    let h = (hsl[0].rem_euclid(360.0)) / 360.0;
    let s = hsl[1].clamp(0.0, 1.0);
    let l = hsl[2].clamp(0.0, 1.0);

    if s < 1e-5 {
        return [l, l, l];
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;

    [
        hue_to_rgb(p, q, h + 1.0 / 3.0),
        hue_to_rgb(p, q, h),
        hue_to_rgb(p, q, h - 1.0 / 3.0),
    ]
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
        let srgb = rec2020_linear_to_srgb_display([1.0, 1.0, 1.0]);
        for c in srgb {
            assert!((c - 1.0).abs() < 1e-3, "White point shifted: got {c}");
        }
    }

    #[test]
    fn test_tone_curve_identity() {
        let points = vec![
            ToneCurvePoint { x: 0.0, y: 0.0 },
            ToneCurvePoint { x: 0.5, y: 0.5 },
            ToneCurvePoint { x: 1.0, y: 1.0 },
        ];
        let lut = generate_tone_curve_lut(&points);
        assert!((lut[0] - 0.0).abs() < 1e-4);
        assert!((lut[128] - (128.0 / 255.0)).abs() < 1e-3);
        assert!((lut[255] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_tone_curve_monotonicity() {
        let points = vec![
            ToneCurvePoint { x: 0.0, y: 0.0 },
            ToneCurvePoint { x: 0.25, y: 0.15 },
            ToneCurvePoint { x: 0.75, y: 0.85 },
            ToneCurvePoint { x: 1.0, y: 1.0 },
        ];
        let lut = generate_tone_curve_lut(&points);
        for i in 1..256 {
            assert!(
                lut[i] >= lut[i - 1],
                "Monotonicity violated at {i}: {} < {}",
                lut[i],
                lut[i - 1]
            );
        }
    }

    #[test]
    fn test_rgb_hsl_roundtrip() {
        let colors = [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.8, 0.4, 0.2],
            [0.2, 0.7, 0.9],
        ];

        for rgb in colors {
            let hsl = rgb_to_hsl(rgb);
            let roundtrip = hsl_to_rgb(hsl);
            for ch in 0..3 {
                assert!(
                    (rgb[ch] - roundtrip[ch]).abs() < 1e-3,
                    "Failed HSL roundtrip for {:?}: got {:?}",
                    rgb,
                    roundtrip
                );
            }
        }
    }
}
