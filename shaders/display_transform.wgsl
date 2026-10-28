// Fullscreen display transform shader for beaM.
// Transforms linear working space (linear Rec.2020) to sRGB with display gamma EOTF.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    // Generate a fullscreen triangle covering [-1, 1] in clip space
    let x = f32(i32(vertex_index & 1u) * 4 - 1);
    let y = f32(i32(vertex_index & 2u) * 2 - 1);
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    // UV coordinates: (0, 0) at top-left to (1, 1) at bottom-right
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

@group(0) @binding(0) var t_input: texture_2d<f32>;

// Linear Rec.2020 to Linear sRGB matrix
const M_REC2020_TO_SRGB: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.6604910, -0.1245505, -0.0181508),
    vec3<f32>(-0.5876411, 1.1328999, -0.1005789),
    vec3<f32>(-0.0728499, -0.0083494, 1.1187297)
);

fn linear_to_srgb(x: f32) -> f32 {
    if (x <= 0.0031308) {
        return x * 12.92;
    } else {
        return 1.055 * pow(x, 1.0 / 2.4) - 0.055;
    }
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    let tex_coords = vec2<i32>(clamp(in.uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    let color = textureLoad(t_input, tex_coords, 0);
    
    // Convert wide-gamut linear Rec.2020 to linear sRGB
    let linear_srgb = M_REC2020_TO_SRGB * color.rgb;
    
    // Clamp to valid gamut range [0.0, 1.0] for SDR display
    let clamped = clamp(linear_srgb, vec3<f32>(0.0), vec3<f32>(1.0));
    
    // Apply sRGB transfer function (gamma curve)
    let srgb = vec3<f32>(
        linear_to_srgb(clamped.r),
        linear_to_srgb(clamped.g),
        linear_to_srgb(clamped.b)
    );
    
    return vec4<f32>(srgb, color.a);
}
