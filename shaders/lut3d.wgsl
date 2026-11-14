// 3D .cube LUT evaluation pass for beaM.
// Converts linear Rec.2020 -> sRGB [0,1], samples 3D LUT texture, and converts back to linear Rec.2020.

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

struct Lut3DUniforms {
    domain_min: vec4<f32>,
    domain_max: vec4<f32>,
    // x = intensity (0..1), y = enabled (0 or 1), z = lut_size, w = pad
    params: vec4<f32>,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_params: Lut3DUniforms;
@group(0) @binding(2) var t_lut3d: texture_3d<f32>;
@group(0) @binding(3) var s_lut3d: sampler;

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let clamped = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let low = clamped * 12.92;
    let high = 1.055 * pow(clamped, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, clamped <= vec3<f32>(0.0031308));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let clamped = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let low = clamped / 12.92;
    let high = pow((clamped + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, clamped <= vec3<f32>(0.04045));
}

fn rec2020_to_srgb_linear(c: vec3<f32>) -> vec3<f32> {
    // Standard linear Rec.2020 -> linear sRGB/Rec.709 matrix
    let r =  1.6604910 * c.r - 0.5876411 * c.g - 0.0728499 * c.b;
    let g = -0.1245505 * c.r + 1.1328999 * c.g - 0.0083494 * c.b;
    let b = -0.0181508 * c.r - 0.1005789 * c.g + 1.1187297 * c.b;
    return vec3<f32>(r, g, b);
}

fn srgb_linear_to_rec2020(c: vec3<f32>) -> vec3<f32> {
    // Standard linear sRGB/Rec.709 -> linear Rec.2020 matrix
    let r = 0.6274039 * c.r + 0.3292830 * c.g + 0.0433131 * c.b;
    let g = 0.0690973 * c.r + 0.9195404 * c.g + 0.0113623 * c.b;
    let b = 0.0163914 * c.r + 0.0880133 * c.g + 0.8955953 * c.b;
    return vec3<f32>(r, g, b);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    let tex_coords = vec2<i32>(clamp(in.uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    let color = textureLoad(t_input, tex_coords, 0);

    if (u_params.params.y < 0.5 || u_params.params.x <= 0.0001) {
        return color;
    }

    let srgb = linear_to_srgb(rec2020_to_srgb_linear(color.rgb));
    let span = max(u_params.domain_max.xyz - u_params.domain_min.xyz, vec3<f32>(1e-5));
    let norm_coord = clamp((srgb - u_params.domain_min.xyz) / span, vec3<f32>(0.0), vec3<f32>(1.0));

    // Align to texel centers of the 3D LUT texture: (0.5 + coord * (N - 1)) / N
    let n = max(u_params.params.z, 2.0);
    let lut_uvw = (0.5 + norm_coord * (n - 1.0)) / n;

    let sampled_srgb = textureSampleLevel(t_lut3d, s_lut3d, lut_uvw, 0.0).rgb;
    let lut_rec2020 = srgb_linear_to_rec2020(srgb_to_linear(sampled_srgb));

    let intensity = clamp(u_params.params.x, 0.0, 1.0);
    let out_rgb = mix(color.rgb, lut_rec2020, intensity);

    return vec4<f32>(max(out_rgb, vec3<f32>(0.0)), color.a);
}
