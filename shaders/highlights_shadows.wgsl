// Highlights and shadows adjustment pass for beaM.
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

struct HighlightsShadowsUniforms {
    highlights: f32, // -100.0 to +100.0
    shadows: f32,    // -100.0 to +100.0
    enabled: u32,
    _pad0: u32,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_params: HighlightsShadowsUniforms;

const REC2020_LUMA: vec3<f32> = vec3<f32>(0.2627, 0.6780, 0.0593);

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    let tex_coords = vec2<i32>(clamp(in.uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    let color = textureLoad(t_input, tex_coords, 0);

    if (u_params.enabled == 0u || (abs(u_params.highlights) < 1e-4 && abs(u_params.shadows) < 1e-4)) {
        return color;
    }

    let luma = dot(color.rgb, REC2020_LUMA);
    if (luma <= 1e-5) {
        return color;
    }

    // Mid-gray pivot = 0.18 in linear light
    let mid = 0.18;

    // Highlights weight: transitions from 0 at mid to 1 at bright highlights
    let h_weight = pow(clamp((luma - mid) / max(1.0 - mid, 0.01), 0.0, 1.0), 1.5);
    let h_factor = 1.0 + (u_params.highlights / 100.0) * 0.5 * h_weight;

    // Shadows weight: transitions from 0 at mid to 1 at deepest shadows
    let s_weight = pow(clamp((mid - luma) / mid, 0.0, 1.0), 1.5);
    let s_factor = 1.0 + (u_params.shadows / 100.0) * 0.8 * s_weight;

    let target_luma = max(luma * h_factor * s_factor, 0.0);
    let ratio = target_luma / luma;
    let adjusted_rgb = max(color.rgb * ratio, vec3<f32>(0.0));

    return vec4<f32>(adjusted_rgb, color.a);
}
