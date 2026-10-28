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

struct DisplayGeometryUniforms {
    crop_x: f32,
    crop_y: f32,
    crop_w: f32,
    crop_h: f32,
    rotation: u32,
    flip_h: u32,
    flip_v: u32,
    straighten_rad: f32,
    crop_enabled: u32,
    show_shadow_clipping: u32,
    show_highlight_clipping: u32,
    _pad2: u32,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_geo: DisplayGeometryUniforms;

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
        return 1.055 * pow(max(x, 0.0), 1.0 / 2.4) - 0.055;
    }
}

fn sample_input_bilinear(uv: vec2<f32>, dims: vec2<f32>) -> vec4<f32> {
    let px = uv * dims - vec2<f32>(0.5);
    let base = floor(px);
    let f = px - base;
    let i_base = vec2<i32>(base);

    let max_coords = vec2<i32>(dims) - vec2<i32>(1);
    let c00 = textureLoad(t_input, clamp(i_base, vec2<i32>(0), max_coords), 0);
    let c10 = textureLoad(t_input, clamp(i_base + vec2<i32>(1, 0), vec2<i32>(0), max_coords), 0);
    let c01 = textureLoad(t_input, clamp(i_base + vec2<i32>(0, 1), vec2<i32>(0), max_coords), 0);
    let c11 = textureLoad(t_input, clamp(i_base + vec2<i32>(1, 1), vec2<i32>(0), max_coords), 0);

    let top = mix(c00, c10, f.x);
    let bottom = mix(c01, c11, f.x);
    return mix(top, bottom, f.y);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    var uv = in.uv;

    // 1. If crop enabled, map into crop rect
    if (u_geo.crop_enabled != 0u) {
        uv = vec2<f32>(
            u_geo.crop_x + uv.x * u_geo.crop_w,
            u_geo.crop_y + uv.y * u_geo.crop_h
        );
    }

    // 2. Straighten rotation around crop center
    if (abs(u_geo.straighten_rad) > 0.00001) {
        var center = vec2<f32>(0.5, 0.5);
        if (u_geo.crop_enabled != 0u) {
            center = vec2<f32>(
                u_geo.crop_x + u_geo.crop_w * 0.5,
                u_geo.crop_y + u_geo.crop_h * 0.5
            );
        }
        let rad = -u_geo.straighten_rad;
        let cos_a = cos(rad);
        let sin_a = sin(rad);
        let d = uv - center;
        uv = center + vec2<f32>(
            d.x * cos_a - d.y * sin_a,
            d.x * sin_a + d.y * cos_a
        );
    }

    // 3. Flips
    if (u_geo.flip_h != 0u) {
        uv.x = 1.0 - uv.x;
    }
    if (u_geo.flip_v != 0u) {
        uv.y = 1.0 - uv.y;
    }

    // 4. 90-degree rotations
    let rot = u_geo.rotation % 360u;
    if (rot == 90u) {
        uv = vec2<f32>(uv.y, 1.0 - uv.x);
    } else if (rot == 180u) {
        uv = vec2<f32>(1.0 - uv.x, 1.0 - uv.y);
    } else if (rot == 270u) {
        uv = vec2<f32>(1.0 - uv.y, uv.x);
    }

    let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let color = sample_input_bilinear(clamped_uv, dims);
    
    // Convert wide-gamut linear Rec.2020 to linear sRGB
    let linear_srgb = M_REC2020_TO_SRGB * color.rgb;

    // Highlight clipping overlay (red) for blown out highlights
    if (u_geo.show_highlight_clipping != 0u) {
        if (linear_srgb.r >= 1.0 || linear_srgb.g >= 1.0 || linear_srgb.b >= 1.0) {
            return vec4<f32>(1.0, 0.0, 0.0, 1.0);
        }
    }

    // Shadow clipping overlay (blue) for crushed blacks
    if (u_geo.show_shadow_clipping != 0u) {
        if (linear_srgb.r <= 0.0005 && linear_srgb.g <= 0.0005 && linear_srgb.b <= 0.0005) {
            return vec4<f32>(0.0, 0.25, 1.0, 1.0);
        }
    }
    
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
