// Manual Lens Correction (Distortion, Chromatic Aberration, Lens Vignette) for beaM.
// Operates on linear Rec.2020 pixel data in full-image normalized coordinates.

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

struct LensUniforms {
    distortion: f32,      // -100.0 to +100.0
    ca_red_cyan: f32,     // -100.0 to +100.0
    ca_blue_yellow: f32,  // -100.0 to +100.0
    vignette: f32,        // -100.0 to +100.0
    enabled: u32,
    perspective_v: f32,   // -100.0 to +100.0
    perspective_h: f32,   // -100.0 to +100.0
    _pad2: u32,
    uv_offset: vec2<f32>,
    uv_scale: vec2<f32>,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_params: LensUniforms;

fn sample_bilinear(local_uv: vec2<f32>, dims: vec2<f32>) -> vec4<f32> {
    let clamped = clamp(local_uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let px = clamped * dims - vec2<f32>(0.5);
    let base = floor(px);
    let f = px - base;
    let i_base = vec2<i32>(base);
    let max_c = vec2<i32>(dims) - vec2<i32>(1);

    let c00 = textureLoad(t_input, clamp(i_base, vec2<i32>(0), max_c), 0);
    let c10 = textureLoad(t_input, clamp(i_base + vec2<i32>(1, 0), vec2<i32>(0), max_c), 0);
    let c01 = textureLoad(t_input, clamp(i_base + vec2<i32>(0, 1), vec2<i32>(0), max_c), 0);
    let c11 = textureLoad(t_input, clamp(i_base + vec2<i32>(1, 1), vec2<i32>(0), max_c), 0);

    return mix(mix(c00, c10, f.x), mix(c01, c11, f.x), f.y);
}

fn perspective_global_uv(global_uv: vec2<f32>, pv: f32, ph: f32) -> vec2<f32> {
    let p = global_uv - vec2<f32>(0.5, 0.5);
    let denom = max(1.0 + pv * p.y + ph * p.x, 0.25);
    return vec2<f32>(0.5, 0.5) + p / denom;
}

fn distort_global_uv(global_uv: vec2<f32>, k1: f32, scale_mul: f32, aspect: f32) -> vec2<f32> {
    let d = global_uv - vec2<f32>(0.5, 0.5);
    let d_iso = vec2<f32>(d.x * aspect, d.y);
    let r2 = dot(d_iso, d_iso);
    let factor = (1.0 + k1 * r2) * scale_mul;
    return vec2<f32>(0.5, 0.5) + d * factor;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    let tex_coords = vec2<i32>(clamp(in.uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    let base_color = textureLoad(t_input, tex_coords, 0);

    if (u_params.enabled == 0u || (
        abs(u_params.distortion) < 1e-4 &&
        abs(u_params.ca_red_cyan) < 1e-4 &&
        abs(u_params.ca_blue_yellow) < 1e-4 &&
        abs(u_params.vignette) < 1e-4 &&
        abs(u_params.perspective_v) < 1e-4 &&
        abs(u_params.perspective_h) < 1e-4
    )) {
        return base_color;
    }

    let uv_scale = max(u_params.uv_scale, vec2<f32>(1e-5));
    let raw_global_uv = u_params.uv_offset + in.uv * uv_scale;
    let pv = (u_params.perspective_v / 100.0) * 0.45;
    let ph = (u_params.perspective_h / 100.0) * 0.45;
    let global_uv = perspective_global_uv(raw_global_uv, pv, ph);

    if (global_uv.x < 0.0 || global_uv.x > 1.0 || global_uv.y < 0.0 || global_uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, base_color.a);
    }

    let full_dims = dims / uv_scale;
    let aspect = full_dims.x / max(full_dims.y, 1.0);

    let k1 = -(u_params.distortion / 100.0) * 0.35;
    let ca_r_scale = 1.0 + (u_params.ca_red_cyan / 100.0) * 0.015;
    let ca_b_scale = 1.0 + (u_params.ca_blue_yellow / 100.0) * 0.015;

    let g_uv_r = distort_global_uv(global_uv, k1, ca_r_scale, aspect);
    let g_uv_g = distort_global_uv(global_uv, k1, 1.0, aspect);
    let g_uv_b = distort_global_uv(global_uv, k1, ca_b_scale, aspect);

    let l_uv_r = (g_uv_r - u_params.uv_offset) / uv_scale;
    let l_uv_g = (g_uv_g - u_params.uv_offset) / uv_scale;
    let l_uv_b = (g_uv_b - u_params.uv_offset) / uv_scale;

    let r = sample_bilinear(l_uv_r, dims).r;
    let g_sample = sample_bilinear(l_uv_g, dims);
    let b = sample_bilinear(l_uv_b, dims).b;

    var rgb = vec3<f32>(r, g_sample.g, b);

    // Optical lens vignette correction (brighten or darken radial falloff)
    if (abs(u_params.vignette) >= 1e-4) {
        let centered = (global_uv - vec2<f32>(0.5, 0.5)) * 2.0;
        let r2_corner = clamp(dot(centered, centered) * 0.5, 0.0, 1.5);
        let gain = max(1.0 + (u_params.vignette / 100.0) * 0.85 * r2_corner, 0.0);
        rgb = rgb * gain;
    }

    return vec4<f32>(max(rgb, vec3<f32>(0.0)), g_sample.a);
}
