// Contrast adjustment shader pass for beaM.
// Operates in linear working space (linear Rec.2020) pivoting around middle gray.

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

struct ContrastUniforms {
    slope: f32,    // 2.0 ^ (contrast / 100.0)
    midpoint: f32, // standard linear middle gray = 0.18
    enabled: u32,
    _pad0: u32,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_params: ContrastUniforms;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    let tex_coords = vec2<i32>(clamp(in.uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    let color = textureLoad(t_input, tex_coords, 0);

    if (u_params.enabled == 0u || abs(u_params.slope - 1.0) < 1e-5) {
        return color;
    }

    let mid = max(u_params.midpoint, 0.001);
    let slope = u_params.slope;

    // Contrast pivot formula: mid * (x / mid) ^ slope
    let norm = max(color.rgb, vec3<f32>(0.0)) / mid;
    let contrasted_rgb = mid * pow(norm, vec3<f32>(slope));

    return vec4<f32>(contrasted_rgb, color.a);
}
