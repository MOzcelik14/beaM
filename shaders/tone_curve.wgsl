// Tone curve evaluation pass for beaM.
// Operates on linear Rec.2020 pixel data using a 256-entry spline LUT.

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

// 64 * 4 = 256 LUT points per channel representing the interpolated tone curves
struct ToneCurveUniforms {
    master_lut: array<vec4<f32>, 64>,
    red_lut: array<vec4<f32>, 64>,
    green_lut: array<vec4<f32>, 64>,
    blue_lut: array<vec4<f32>, 64>,
    enabled: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_params: ToneCurveUniforms;

fn unpack_lut(v: vec4<f32>, comp_idx: u32) -> f32 {
    if (comp_idx == 0u) { return v.x; }
    if (comp_idx == 1u) { return v.y; }
    if (comp_idx == 2u) { return v.z; }
    return v.w;
}

fn evaluate_channel_curve(val: f32, channel: u32) -> f32 {
    let clamped = clamp(val, 0.0, 1.0);
    let pos = pow(clamped, 1.0 / 2.2) * 255.0;
    let i0 = min(u32(floor(pos)), 254u);
    let i1 = i0 + 1u;
    let frac = pos - f32(i0);

    let v0_idx = i0 / 4u;
    let c0_idx = i0 % 4u;
    let v1_idx = i1 / 4u;
    let c1_idx = i1 % 4u;

    var y0: f32;
    var y1: f32;
    if (channel == 0u) {
        y0 = unpack_lut(u_params.master_lut[v0_idx], c0_idx);
        y1 = unpack_lut(u_params.master_lut[v1_idx], c1_idx);
    } else if (channel == 1u) {
        y0 = unpack_lut(u_params.red_lut[v0_idx], c0_idx);
        y1 = unpack_lut(u_params.red_lut[v1_idx], c1_idx);
    } else if (channel == 2u) {
        y0 = unpack_lut(u_params.green_lut[v0_idx], c0_idx);
        y1 = unpack_lut(u_params.green_lut[v1_idx], c1_idx);
    } else {
        y0 = unpack_lut(u_params.blue_lut[v0_idx], c0_idx);
        y1 = unpack_lut(u_params.blue_lut[v1_idx], c1_idx);
    }
    let mapped = mix(y0, y1, frac);
    if (val > 1.0) {
        return mapped + (val - 1.0);
    }
    return mapped;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    let tex_coords = vec2<i32>(clamp(in.uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    let color = textureLoad(t_input, tex_coords, 0);

    if (u_params.enabled == 0u) {
        return color;
    }

    let m_rgb = vec3<f32>(
        evaluate_channel_curve(color.r, 0u),
        evaluate_channel_curve(color.g, 0u),
        evaluate_channel_curve(color.b, 0u)
    );

    let curved_rgb = vec3<f32>(
        evaluate_channel_curve(m_rgb.r, 1u),
        evaluate_channel_curve(m_rgb.g, 2u),
        evaluate_channel_curve(m_rgb.b, 3u)
    );

    return vec4<f32>(curved_rgb, color.a);
}
