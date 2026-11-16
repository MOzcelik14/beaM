// Post-crop vignette and organic film grain shader for beaM.
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

struct VignetteGrainUniforms {
    vignette_amount: f32,   // -100.0 to +100.0
    vignette_midpoint: f32, // 0.0 to 100.0
    vignette_feather: f32,  // 0.0 to 100.0
    grain_amount: f32,      // 0.0 to 100.0
    grain_size: f32,        // 1.0 to 5.0
    enabled: u32,
    _pad0: u32,
    _pad1: u32,
    uv_offset: vec2<f32>,
    uv_scale: vec2<f32>,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_params: VignetteGrainUniforms;

const REC2020_LUMA: vec3<f32> = vec3<f32>(0.2627, 0.6780, 0.0593);

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + vec3<f32>(33.33));
    return fract((p3.x + p3.y) * p3.z);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    let tex_coords = vec2<i32>(clamp(in.uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    let color = textureLoad(t_input, tex_coords, 0);

    if (u_params.enabled == 0u || (abs(u_params.vignette_amount) < 1e-4 && u_params.grain_amount < 1e-4)) {
        return color;
    }

    let global_uv = u_params.uv_offset + in.uv * u_params.uv_scale;
    var rgb = color.rgb;

    // 1. Vignette
    if (abs(u_params.vignette_amount) >= 1e-4) {
        let centered = (global_uv - vec2<f32>(0.5, 0.5)) * 2.0;
        let dist = length(centered) / 1.41421356; // 0 at center, 1 at corners
        let mid = clamp(u_params.vignette_midpoint / 100.0, 0.05, 0.95);
        let feather = max(u_params.vignette_feather / 100.0, 0.05) * 0.65;
        let edge0 = max(mid - feather * 0.5, 0.0);
        let edge1 = min(mid + feather * 0.5, 1.25);
        let falloff = smoothstep(edge0, edge1, dist);
        let amount = u_params.vignette_amount / 100.0;
        if (amount < 0.0) {
            // Darken corners
            let factor = 1.0 + amount * 0.85 * falloff;
            rgb = max(rgb * factor, vec3<f32>(0.0));
        } else {
            // Lift corners subtly
            rgb = rgb + vec3<f32>(amount * 0.35 * falloff);
        }
    }

    // 2. Film Grain
    if (u_params.grain_amount >= 1e-4) {
        let full_dims = dims / max(u_params.uv_scale, vec2<f32>(1e-5));
        let pixel_pos = floor((global_uv * full_dims) / max(u_params.grain_size, 1.0));
        let n1 = hash12(pixel_pos);
        let n2 = hash12(pixel_pos + vec2<f32>(17.3, 41.9));
        // Triangular distribution centered at 0
        let noise = (n1 + n2 - 1.0);
        let luma = dot(rgb, REC2020_LUMA);
        let midtone_mask = clamp(1.0 - abs(luma - 0.35) * 1.4, 0.25, 1.0);
        let strength = (u_params.grain_amount / 100.0) * 0.08 * midtone_mask;
        rgb = max(rgb + vec3<f32>(noise * strength), vec3<f32>(0.0));
    }

    return vec4<f32>(rgb, color.a);
}
