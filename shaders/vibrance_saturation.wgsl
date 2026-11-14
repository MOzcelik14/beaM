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
    _pad0: u32,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_params: VibranceSaturationUniforms;

const REC2020_LUMA: vec3<f32> = vec3<f32>(0.2627, 0.6780, 0.0593);

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    let tex_coords = vec2<i32>(clamp(in.uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    let color = textureLoad(t_input, tex_coords, 0);

    if (u_params.enabled == 0u || (abs(u_params.vibrance) < 1e-4 && abs(u_params.saturation) < 1e-4)) {
        return color;
    }

    let luma = dot(color.rgb, REC2020_LUMA);
    let max_c = max(color.r, max(color.g, color.b));
    let min_c = min(color.r, min(color.g, color.b));
    let chroma_sat = clamp((max_c - min_c) / max(max_c, 1e-4), 0.0, 1.0);

    // Gentle skin-tone attenuation for vibrance (attenuate warm R > G > B hues)
    var skin_protect = 1.0;
    if (color.r > color.g && color.g > color.b && (color.r - color.b) > 1e-4) {
        let warmth = clamp((color.r - color.b) / max(max_c, 1e-4), 0.0, 1.0);
        skin_protect = 1.0 - 0.5 * warmth;
    }

    let vib_delta = (u_params.vibrance / 100.0) * (1.0 - chroma_sat) * skin_protect;
    let sat_delta = u_params.saturation / 100.0;
    let factor = max(1.0 + sat_delta + vib_delta, 0.0);

    let adjusted = max(vec3<f32>(luma) + (color.rgb - vec3<f32>(luma)) * factor, vec3<f32>(0.0));
    return vec4<f32>(adjusted, color.a);
}
