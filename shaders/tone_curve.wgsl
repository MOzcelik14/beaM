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

// 64 * 4 = 256 LUT points representing the interpolated tone curve
struct ToneCurveUniforms {
    lut: array<vec4<f32>, 64>,
    enabled: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_params: ToneCurveUniforms;

fn sample_lut_element(index: u32) -> f32 {
    let vec_idx = index / 4u;
    let comp_idx = index % 4u;
    let v = u_params.lut[vec_idx];
    if (comp_idx == 0u) { return v.x; }
    if (comp_idx == 1u) { return v.y; }
    if (comp_idx == 2u) { return v.z; }
    return v.w;
}

fn evaluate_curve(val: f32) -> f32 {
    let clamped = clamp(val, 0.0, 1.0);
    let pos = clamped * 255.0;
    let i0 = min(u32(floor(pos)), 254u);
    let i1 = i0 + 1u;
    let frac = pos - f32(i0);

    let y0 = sample_lut_element(i0);
    let y1 = sample_lut_element(i1);
    return mix(y0, y1, frac);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    let tex_coords = vec2<i32>(clamp(in.uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    let color = textureLoad(t_input, tex_coords, 0);

    if (u_params.enabled == 0u) {
        return color;
    }

    let curved_rgb = vec3<f32>(
        evaluate_curve(color.r),
        evaluate_curve(color.g),
        evaluate_curve(color.b)
    );

    return vec4<f32>(curved_rgb, color.a);
}
