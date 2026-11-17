// Vibrance and global saturation adjustment pass for beaM.
// Operates on linear Rec.2020 pixel data.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let x = f32(i32(vertex_index & 1u) * 4 - 1);
    let y = f32(i32(vertex_index & 2u) * 2 - 1);
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

struct VibranceSaturationUniforms {
    vibrance: f32,   // -100.0 to +100.0
    saturation: f32, // -100.0 to +100.0
    enabled: u32,
    profile_id: u32,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_params: VibranceSaturationUniforms;

const REC2020_LUMA: vec3<f32> = vec3<f32>(0.2627, 0.6780, 0.0593);

fn apply_camera_profile(rgb: vec3<f32>, profile_id: u32) -> vec3<f32> {
    var c = max(rgb, vec3<f32>(0.0));
    if (profile_id == 0u) {
        return c;
    }
    let l = dot(c, REC2020_LUMA);
    if (profile_id == 1u) {
        // Neutral: gentler contrast and slightly reduced saturation
        c = 0.18 * pow(max(c, vec3<f32>(1e-5)) / 0.18, vec3<f32>(0.90));
        return max(mix(vec3<f32>(l), c, 0.90), vec3<f32>(0.0));
    } else if (profile_id == 2u) {
        // Vivid: punchier contrast and richer saturation
        c = 0.18 * pow(max(c, vec3<f32>(1e-5)) / 0.18, vec3<f32>(1.15));
        let lv = dot(c, REC2020_LUMA);
        return max(mix(vec3<f32>(lv), c, 1.22), vec3<f32>(0.0));
    } else if (profile_id == 3u) {
        // Portrait: warm skin-friendly rolloff
        c = c * vec3<f32>(1.02, 1.00, 0.97);
        let lp = dot(c, REC2020_LUMA);
        return max(mix(vec3<f32>(lp), c, 0.92), vec3<f32>(0.0));
    } else if (profile_id == 4u) {
        // Landscape: crisp greens/blues and boosted saturation
        c = c * vec3<f32>(0.98, 1.03, 1.04);
        let ll = dot(c, REC2020_LUMA);
        return max(mix(vec3<f32>(ll), c, 1.12), vec3<f32>(0.0));
    } else if (profile_id == 5u) {
        // Monochrome
        return vec3<f32>(l);
    }
    return c;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    let tex_coords = vec2<i32>(clamp(in.uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    let color = textureLoad(t_input, tex_coords, 0);

    let vs_active = u_params.enabled != 0u && (abs(u_params.vibrance) >= 1e-4 || abs(u_params.saturation) >= 1e-4);
    if (!vs_active && u_params.profile_id == 0u) {
        return color;
    }

    var rgb = apply_camera_profile(color.rgb, u_params.profile_id);
    if (!vs_active) {
        return vec4<f32>(rgb, color.a);
    }

    let luma = dot(rgb, REC2020_LUMA);
    let max_c = max(rgb.r, max(rgb.g, rgb.b));
    let min_c = min(rgb.r, min(rgb.g, rgb.b));
    let chroma_sat = clamp((max_c - min_c) / max(max_c, 1e-4), 0.0, 1.0);

    // Gentle skin-tone attenuation for vibrance (attenuate warm R > G > B hues)
    var skin_protect = 1.0;
    if (rgb.r > rgb.g && rgb.g > rgb.b && (rgb.r - rgb.b) > 1e-4) {
        let warmth = clamp((rgb.r - rgb.b) / max(max_c, 1e-4), 0.0, 1.0);
        skin_protect = 1.0 - 0.5 * warmth;
    }

    let vib_delta = (u_params.vibrance / 100.0) * (1.0 - chroma_sat) * skin_protect;
    let sat_delta = u_params.saturation / 100.0;
    let factor = max(1.0 + sat_delta + vib_delta, 0.0);

    let adjusted = max(vec3<f32>(luma) + (rgb - vec3<f32>(luma)) * factor, vec3<f32>(0.0));
    return vec4<f32>(adjusted, color.a);
}
