// 3-Way Color Grading Wheels (Shadows / Midtones / Highlights) pass for beaM.
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

struct ColorGradingUniforms {
    // xyz = linear tint color (around 0.0), w = luminance offset (-1..1)
    shadows: vec4<f32>,
    midtones: vec4<f32>,
    highlights: vec4<f32>,
    // x = blending (0..1), y = balance (-1..1), z = enabled (0 or 1), w = pad
    params: vec4<f32>,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_params: ColorGradingUniforms;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    let tex_coords = vec2<i32>(clamp(in.uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    let color = textureLoad(t_input, tex_coords, 0);

    if (u_params.params.z < 0.5) {
        return color;
    }

    let lum = clamp(dot(color.rgb, vec3<f32>(0.2627, 0.6780, 0.0593)), 0.0, 1.0);
    // Perceptual luminance for smooth shadow/mid/highlight separation
    let p_lum = pow(lum, 0.45);

    let blending = clamp(u_params.params.x, 0.0, 1.0);
    let balance = clamp(u_params.params.y, -1.0, 1.0);

    // Balance shifts the crossover point between shadows and highlights
    let pivot = clamp(0.5 - balance * 0.25, 0.15, 0.85);
    let feather = mix(0.08, 0.40, blending);

    let w_shadows = 1.0 - smoothstep(max(0.0, pivot - feather), min(1.0, pivot + feather * 0.5), p_lum);
    let w_highlights = smoothstep(max(0.0, pivot - feather * 0.5), min(1.0, pivot + feather), p_lum);
    let w_midtones = max(0.0, 1.0 - w_shadows - w_highlights);

    // Apply tint offsets (scaled subtly for natural grading) and per-zone luminance
    var rgb = max(color.rgb, vec3<f32>(0.0));

    let s_tint = u_params.shadows.xyz * 0.28;
    let m_tint = u_params.midtones.xyz * 0.28;
    let h_tint = u_params.highlights.xyz * 0.28;

    let tint = s_tint * w_shadows + m_tint * w_midtones + h_tint * w_highlights;
    let lum_shift = (u_params.shadows.w * w_shadows + u_params.midtones.w * w_midtones + u_params.highlights.w * w_highlights) * 0.35;

    // Lift/Gamma/Gain-like soft application so pure black/white aren't harshly clipped
    let soft_mask = clamp(lum * 2.0 + 0.12, 0.0, 1.0);
    rgb = rgb + tint * (0.15 + 0.85 * soft_mask) + vec3<f32>(lum_shift * (0.1 + 0.9 * lum));

    return vec4<f32>(max(rgb, vec3<f32>(0.0)), color.a);
}
