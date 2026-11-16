// Parametric local GPU masking pass (Linear & Radial Gradients) for beaM.
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

struct MaskGpuEntry {
    // x = type (0 = linear, 1 = radial), y = invert (0 or 1), z = feather (0..1), w = enabled (0 or 1)
    geom0: vec4<f32>,
    // xy = start/center, zw = end/radius
    geom1: vec4<f32>,
    // x = exposure_mult, y = contrast_slope, z = highlights (-1..1), w = shadows (-1..1)
    adj0: vec4<f32>,
    // xyz = wb gains, w = saturation (-1..1)
    adj1: vec4<f32>,
};

struct MaskUniforms {
    masks: array<MaskGpuEntry, 8>,
    count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    uv_rect: vec4<f32>, // xy = offset, zw = scale
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_params: MaskUniforms;

fn compute_mask_weight(uv: vec2<f32>, m: MaskGpuEntry) -> f32 {
    if (m.geom0.w < 0.5) {
        return 0.0;
    }
    if (m.geom0.x < 0.5) {
        // Linear gradient: start -> end
        let start = m.geom1.xy;
        let end = m.geom1.zw;
        let dir = end - start;
        let len_sq = max(dot(dir, dir), 1e-6);
        let proj = clamp(dot(uv - start, dir) / len_sq, 0.0, 1.0);
        return 1.0 - smoothstep(0.0, 1.0, proj);
    } else {
        // Radial gradient: center, radius
        let center = m.geom1.xy;
        let radius = max(m.geom1.zw, vec2<f32>(1e-4));
        let d = length((uv - center) / radius);
        let feather = clamp(m.geom0.z, 0.01, 1.0);
        let inner = max(0.0, 1.0 - feather);
        var w = 1.0 - smoothstep(inner, 1.0, d);
        if (m.geom0.y > 0.5) {
            w = 1.0 - w;
        }
        return clamp(w, 0.0, 1.0);
    }
}

fn apply_local_adjustments(rgb: vec3<f32>, m: MaskGpuEntry) -> vec3<f32> {
    var c = max(rgb, vec3<f32>(0.0));

    // Local White Balance
    c = c * m.adj1.xyz;

    // Local Exposure
    c = c * m.adj0.x;

    // Local Highlights & Shadows
    let lum = dot(c, vec3<f32>(0.2627, 0.6780, 0.0593));
    let shadow_mask = 1.0 - smoothstep(0.0, 0.35, lum);
    let highlight_mask = smoothstep(0.30, 0.90, lum);
    let s_gain = pow(2.0, m.adj0.w * 1.4);
    let h_gain = pow(2.0, m.adj0.z * 1.4);
    let hs_scale = mix(1.0, s_gain, shadow_mask) * mix(1.0, h_gain, highlight_mask);
    c = c * hs_scale;

    // Local Contrast around 0.18 mid-grey
    let slope = m.adj0.y;
    if (abs(slope - 1.0) > 0.001) {
        let ratio = max(c, vec3<f32>(1e-5)) / 0.18;
        c = 0.18 * pow(ratio, vec3<f32>(slope));
    }

    // Local Saturation
    let sat = m.adj1.w;
    if (abs(sat) > 0.001) {
        let l = dot(c, vec3<f32>(0.2627, 0.6780, 0.0593));
        c = max(mix(vec3<f32>(l), c, 1.0 + sat), vec3<f32>(0.0));
    }

    return c;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    let tex_coords = vec2<i32>(clamp(in.uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    let color = textureLoad(t_input, tex_coords, 0);

    if (u_params.count == 0u) {
        return color;
    }

    var rgb = color.rgb;
    let global_uv = u_params.uv_rect.xy + in.uv * u_params.uv_rect.zw;
    let n = min(u_params.count, 8u);
    for (var i: u32 = 0u; i < n; i = i + 1u) {
        let m = u_params.masks[i];
        let w = compute_mask_weight(global_uv, m);
        if (w > 0.0005) {
            let adjusted = apply_local_adjustments(rgb, m);
            rgb = mix(rgb, adjusted, w);
        }
    }

    return vec4<f32>(max(rgb, vec3<f32>(0.0)), color.a);
}
