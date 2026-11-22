// Export transform shader for beaM.
// Transforms linear working space (linear Rec.2020) to sRGB, Display P3, or Adobe RGB (1998).

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

struct ExportParams {
    color_space: u32, // 0 = sRGB, 1 = Display P3, 2 = Adobe RGB
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_params: ExportParams;

// Linear Rec.2020 to Linear sRGB matrix (D65)
const M_REC2020_TO_SRGB: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.6604910, -0.1245505, -0.0181508),
    vec3<f32>(-0.5876411, 1.1328999, -0.1005789),
    vec3<f32>(-0.0728499, -0.0083494, 1.1187297)
);

// Linear Rec.2020 to Linear Display P3 matrix (D65)
const M_REC2020_TO_DISPLAY_P3: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.3435783, -0.0652975, 0.0028218),
    vec3<f32>(-0.2821797, 1.0757879, -0.0195985),
    vec3<f32>(-0.0613986, -0.0104905, 1.0167767)
);

// Linear Rec.2020 to Linear Adobe RGB (1998) matrix (D65)
const M_REC2020_TO_ADOBE_RGB: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.1519784, -0.1245505, -0.0225304),
    vec3<f32>(-0.0975031, 1.1328999, -0.0498065),
    vec3<f32>(-0.0544753, -0.0083494, 1.0723369)
);

fn linear_to_srgb(x: f32) -> f32 {
    if (x <= 0.0031308) {
        return x * 12.92;
    } else {
        return 1.055 * pow(max(x, 0.0), 1.0 / 2.4) - 0.055;
    }
}

fn linear_to_adobe(x: f32) -> f32 {
    return pow(clamp(x, 0.0, 1.0), 1.0 / 2.19921875);
}

fn soft_highlight_rolloff(rgb: vec3<f32>) -> vec3<f32> {
    let c = max(rgb, vec3<f32>(0.0));
    let peak = max(c.r, max(c.g, c.b));
    if (peak <= 0.85) {
        return c;
    }
    let excess = peak - 0.85;
    let compressed = 0.85 + (0.15 * excess) / (excess + 0.15);
    return c * (compressed / peak);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    let tex_coords = vec2<i32>(clamp(in.uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    let color = textureLoad(t_input, tex_coords, 0);

    var target_rgb: vec3<f32>;
    if (u_params.color_space == 1u) {
        // Display P3: P3 primaries + sRGB transfer curve
        let lin_p3 = clamp(soft_highlight_rolloff(M_REC2020_TO_DISPLAY_P3 * color.rgb), vec3<f32>(0.0), vec3<f32>(1.0));
        target_rgb = vec3<f32>(
            linear_to_srgb(lin_p3.r),
            linear_to_srgb(lin_p3.g),
            linear_to_srgb(lin_p3.b)
        );
    } else if (u_params.color_space == 2u) {
        // Adobe RGB (1998): Adobe RGB primaries + pure gamma 2.2 curve
        let lin_adobe = clamp(soft_highlight_rolloff(M_REC2020_TO_ADOBE_RGB * color.rgb), vec3<f32>(0.0), vec3<f32>(1.0));
        target_rgb = vec3<f32>(
            linear_to_adobe(lin_adobe.r),
            linear_to_adobe(lin_adobe.g),
            linear_to_adobe(lin_adobe.b)
        );
    } else {
        // sRGB: Rec.709 primaries + sRGB transfer curve
        let lin_srgb = clamp(soft_highlight_rolloff(M_REC2020_TO_SRGB * color.rgb), vec3<f32>(0.0), vec3<f32>(1.0));
        target_rgb = vec3<f32>(
            linear_to_srgb(lin_srgb.r),
            linear_to_srgb(lin_srgb.g),
            linear_to_srgb(lin_srgb.b)
        );
    }

    return vec4<f32>(target_rgb, color.a);
}
