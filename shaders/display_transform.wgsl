// Fullscreen display transform shader for beaM.
// Transforms linear working space (linear Rec.2020) to sRGB with display gamma EOTF.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    // Generate a fullscreen triangle covering [-1, 1] in clip space
    let x = f32(i32(vertex_index & 1u) * 4 - 1);
    let y = f32(i32(vertex_index & 2u) * 2 - 1);
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    // UV coordinates: (0, 0) at top-left to (1, 1) at bottom-right
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

struct DisplayGeometryUniforms {
    crop_x: f32,
    crop_y: f32,
    crop_w: f32,
    crop_h: f32,
    rotation: u32,
    flip_h: u32,
    flip_v: u32,
    straighten_rad: f32,
    crop_enabled: u32,
    show_shadow_clipping: u32,
    show_highlight_clipping: u32,
    proof_flags: u32,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_geo: DisplayGeometryUniforms;

// Linear Rec.2020 to Linear sRGB matrix
const M_REC2020_TO_SRGB: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.6604910, -0.1245505, -0.0181508),
    vec3<f32>(-0.5876411, 1.1328999, -0.1005789),
    vec3<f32>(-0.0728499, -0.0083494, 1.1187297)
);

const M_REC2020_TO_DISPLAY_P3: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.3435783, -0.0652975, 0.0028218),
    vec3<f32>(-0.2821797, 1.0757879, -0.0195985),
    vec3<f32>(-0.0613986, -0.0104905, 1.0167767)
);

const M_DISPLAY_P3_TO_SRGB: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.2249402, -0.0420570, -0.0196376),
    vec3<f32>(-0.2249402, 1.0420570, -0.0786360),
    vec3<f32>(0.0, 0.0, 1.0982736)
);

const M_REC2020_TO_ADOBE_RGB: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.1519784, -0.1245505, -0.0225304),
    vec3<f32>(-0.0975031, 1.1328999, -0.0498065),
    vec3<f32>(-0.0544753, -0.0083494, 1.0723369)
);

const M_ADOBE_RGB_TO_SRGB: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.3983557, 0.0, 0.0),
    vec3<f32>(-0.3983557, 1.0, -0.0429290),
    vec3<f32>(0.0, 0.0, 1.0429290)
);

fn linear_to_srgb(x: f32) -> f32 {
    if (x <= 0.0031308) {
        return x * 12.92;
    } else {
        return 1.055 * pow(max(x, 0.0), 1.0 / 2.4) - 0.055;
    }
}

fn sample_input_bilinear(uv: vec2<f32>, dims: vec2<f32>) -> vec4<f32> {
    let px = uv * dims - vec2<f32>(0.5);
    let base = floor(px);
    let f = px - base;
    let i_base = vec2<i32>(base);

    let max_coords = vec2<i32>(dims) - vec2<i32>(1);
    let c00 = textureLoad(t_input, clamp(i_base, vec2<i32>(0), max_coords), 0);
    let c10 = textureLoad(t_input, clamp(i_base + vec2<i32>(1, 0), vec2<i32>(0), max_coords), 0);
    let c01 = textureLoad(t_input, clamp(i_base + vec2<i32>(0, 1), vec2<i32>(0), max_coords), 0);
    let c11 = textureLoad(t_input, clamp(i_base + vec2<i32>(1, 1), vec2<i32>(0), max_coords), 0);

    let top = mix(c00, c10, f.x);
    let bottom = mix(c01, c11, f.x);
    return mix(top, bottom, f.y);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    var uv = in.uv;

    // 1. If crop enabled, map into crop rect
    if (u_geo.crop_enabled != 0u) {
        uv = vec2<f32>(
            u_geo.crop_x + uv.x * u_geo.crop_w,
            u_geo.crop_y + uv.y * u_geo.crop_h
        );
    }

    // 2. Straighten rotation around crop center in aspect-corrected isotropic space
    if (abs(u_geo.straighten_rad) > 0.00001) {
        var center = vec2<f32>(0.5, 0.5);
        if (u_geo.crop_enabled != 0u) {
            center = vec2<f32>(
                u_geo.crop_x + u_geo.crop_w * 0.5,
                u_geo.crop_y + u_geo.crop_h * 0.5
            );
        }
        let aspect = dims.x / max(dims.y, 1.0);
        let rad = -u_geo.straighten_rad;
        let cos_a = cos(rad);
        let sin_a = sin(rad);
        let d = uv - center;
        let dx = d.x * aspect;
        let dy = d.y;
        uv = center + vec2<f32>(
            (dx * cos_a - dy * sin_a) / aspect,
            dx * sin_a + dy * cos_a
        );
    }

    // 3. Flips
    if (u_geo.flip_h != 0u) {
        uv.x = 1.0 - uv.x;
    }
    if (u_geo.flip_v != 0u) {
        uv.y = 1.0 - uv.y;
    }

    // 4. 90-degree rotations
    let rot = u_geo.rotation % 360u;
    if (rot == 90u) {
        uv = vec2<f32>(uv.y, 1.0 - uv.x);
    } else if (rot == 180u) {
        uv = vec2<f32>(1.0 - uv.x, 1.0 - uv.y);
    } else if (rot == 270u) {
        uv = vec2<f32>(1.0 - uv.y, uv.x);
    }

    let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let color = sample_input_bilinear(clamped_uv, dims);

    let proof_space = u_geo.proof_flags & 3u;
    let show_gamut = (u_geo.proof_flags & 4u) != 0u;
    let show_focus_peaking = (u_geo.proof_flags & 8u) != 0u;

    if (show_focus_peaking) {
        let step = vec2<f32>(1.0) / max(dims, vec2<f32>(1.0));
        let luma_w = vec3<f32>(0.2627, 0.6780, 0.0593);
        let lp = sqrt(clamp(dot(sample_input_bilinear(clamp(clamped_uv + vec2<f32>(step.x, 0.0), vec2<f32>(0.0), vec2<f32>(1.0)), dims).rgb, luma_w), 0.0, 1.0));
        let lm = sqrt(clamp(dot(sample_input_bilinear(clamp(clamped_uv - vec2<f32>(step.x, 0.0), vec2<f32>(0.0), vec2<f32>(1.0)), dims).rgb, luma_w), 0.0, 1.0));
        let up = sqrt(clamp(dot(sample_input_bilinear(clamp(clamped_uv + vec2<f32>(0.0, step.y), vec2<f32>(0.0), vec2<f32>(1.0)), dims).rgb, luma_w), 0.0, 1.0));
        let um = sqrt(clamp(dot(sample_input_bilinear(clamp(clamped_uv - vec2<f32>(0.0, step.y), vec2<f32>(0.0), vec2<f32>(1.0)), dims).rgb, luma_w), 0.0, 1.0));
        let grad = length(vec2<f32>(lp - lm, up - um));
        if (grad > 0.14) {
            return vec4<f32>(0.15, 0.95, 0.45, 1.0);
        }
    }

    var linear_srgb = M_REC2020_TO_SRGB * color.rgb;
    if (proof_space != 0u) {
        var proof_lin = linear_srgb;
        if (proof_space == 2u) {
            proof_lin = M_REC2020_TO_DISPLAY_P3 * color.rgb;
        } else if (proof_space == 3u) {
            proof_lin = M_REC2020_TO_ADOBE_RGB * color.rgb;
        }

        if (show_gamut && (
            proof_lin.r < -0.002 || proof_lin.g < -0.002 || proof_lin.b < -0.002 ||
            proof_lin.r > 1.002 || proof_lin.g > 1.002 || proof_lin.b > 1.002
        )) {
            return vec4<f32>(0.95, 0.20, 0.85, 1.0);
        }

        let proof_clamped = clamp(proof_lin, vec3<f32>(0.0), vec3<f32>(1.0));
        if (proof_space == 2u) {
            linear_srgb = M_DISPLAY_P3_TO_SRGB * proof_clamped;
        } else if (proof_space == 3u) {
            linear_srgb = M_ADOBE_RGB_TO_SRGB * proof_clamped;
        } else {
            linear_srgb = proof_clamped;
        }
    }

    // Highlight clipping overlay (red) for blown out highlights
    if (u_geo.show_highlight_clipping != 0u) {
        if (linear_srgb.r >= 1.0 || linear_srgb.g >= 1.0 || linear_srgb.b >= 1.0) {
            return vec4<f32>(1.0, 0.0, 0.0, 1.0);
        }
    }

    // Shadow clipping overlay (blue) for crushed blacks
    if (u_geo.show_shadow_clipping != 0u) {
        if (linear_srgb.r <= 0.0005 && linear_srgb.g <= 0.0005 && linear_srgb.b <= 0.0005) {
            return vec4<f32>(0.0, 0.25, 1.0, 1.0);
        }
    }
    
    // Soft hue-preserving highlight rolloff above 0.85 to avoid harsh flat clipping
    let pos_rgb = max(linear_srgb, vec3<f32>(0.0));
    let peak = max(pos_rgb.r, max(pos_rgb.g, pos_rgb.b));
    var rolled = pos_rgb;
    if (peak > 0.85) {
        let excess = peak - 0.85;
        let compressed = 0.85 + (0.15 * excess) / (excess + 0.15);
        rolled = pos_rgb * (compressed / peak);
    }

    // Clamp to valid gamut range [0.0, 1.0] for SDR display
    let clamped = clamp(rolled, vec3<f32>(0.0), vec3<f32>(1.0));
    
    // Apply sRGB transfer function (gamma curve)
    let srgb = vec3<f32>(
        linear_to_srgb(clamped.r),
        linear_to_srgb(clamped.g),
        linear_to_srgb(clamped.b)
    );
    
    return vec4<f32>(srgb, color.a);
}
