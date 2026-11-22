// Denoise pass (5x5 Edge-Preserving Smart Denoise, Luma & Color Noise Reduction) for beaM.
// Operates on linear Rec.2020 pixel data prior to tonal/detail amplification.

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

struct DetailUniforms {
    clarity: f32,
    dehaze: f32,
    sharpening: f32,
    noise_reduction_luma: f32,
    noise_reduction_color: f32,
    enabled: u32,
    ai_denoise: f32,
    _pad1: u32,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_params: DetailUniforms;

fn rec2020_lum(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2627, 0.6780, 0.0593));
}

fn load_clamped(coords: vec2<i32>, max_coords: vec2<i32>) -> vec3<f32> {
    let c = clamp(coords, vec2<i32>(0), max_coords);
    return textureLoad(t_input, c, 0).rgb;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = textureDimensions(t_input);
    let max_coords = vec2<i32>(dims) - vec2<i32>(1);
    let tex_coords = vec2<i32>(clamp(in.uv * vec2<f32>(dims), vec2<f32>(0.0), vec2<f32>(max_coords)));
    let center = textureLoad(t_input, tex_coords, 0);

    if (u_params.enabled == 0u || (
        u_params.ai_denoise <= 0.001 &&
        u_params.noise_reduction_luma <= 0.001 &&
        u_params.noise_reduction_color <= 0.001
    )) {
        return center;
    }

    var rgb = max(center.rgb, vec3<f32>(0.0));

    // 0. 5x5 Edge-Preserving Local Smart Denoise
    if (u_params.ai_denoise > 0.001) {
        let strength = clamp(u_params.ai_denoise, 0.0, 1.0);
        let sigma_r = mix(0.02, 0.16, strength);
        let c_lum = rec2020_lum(rgb);
        var acc_rgb = vec3<f32>(0.0);
        var acc_w = 0.0;
        for (var dy: i32 = -2; dy <= 2; dy = dy + 1) {
            for (var dx: i32 = -2; dx <= 2; dx = dx + 1) {
                let s = load_clamped(tex_coords + vec2<i32>(dx, dy), max_coords);
                let s_lum = rec2020_lum(s);
                let d_lum = abs(s_lum - c_lum);
                let dist2 = f32(dx * dx + dy * dy);
                let w_spatial = exp(-dist2 / 6.0);
                let w_range = exp(-(d_lum * d_lum) / (2.0 * sigma_r * sigma_r + 1e-6));
                let w = w_spatial * w_range;
                acc_rgb = acc_rgb + s * w;
                acc_w = acc_w + w;
            }
        }
        let denoised = acc_rgb / max(acc_w, 1e-5);
        rgb = mix(rgb, denoised, strength);
    }

    // 1. 3x3 Luma & Color Noise Reduction
    if (u_params.noise_reduction_luma > 0.001 || u_params.noise_reduction_color > 0.001) {
        var blur3 = vec3<f32>(0.0);
        var luma_bilateral_sum = 0.0;
        var luma_bilateral_w = 0.0;
        let center_lum = rec2020_lum(rgb);
        let luma_sigma = mix(0.01, 0.12, clamp(u_params.noise_reduction_luma, 0.0, 1.0));

        for (var dy: i32 = -1; dy <= 1; dy = dy + 1) {
            for (var dx: i32 = -1; dx <= 1; dx = dx + 1) {
                let s = load_clamped(tex_coords + vec2<i32>(dx, dy), max_coords);
                let k = select(select(1.0, 2.0, dx == 0 || dy == 0), 4.0, dx == 0 && dy == 0);
                blur3 = blur3 + s * k;

                let s_lum = rec2020_lum(s);
                let diff = abs(s_lum - center_lum);
                let bw = exp(-(diff * diff) / (2.0 * luma_sigma * luma_sigma + 1e-6)) * k;
                luma_bilateral_sum = luma_bilateral_sum + s_lum * bw;
                luma_bilateral_w = luma_bilateral_w + bw;
            }
        }
        blur3 = blur3 / 16.0;

        if (u_params.noise_reduction_luma > 0.001) {
            let nr_lum = luma_bilateral_sum / max(luma_bilateral_w, 1e-5);
            let new_lum = mix(center_lum, nr_lum, clamp(u_params.noise_reduction_luma, 0.0, 1.0));
            let scale = new_lum / max(center_lum, 1e-5);
            rgb = select(rgb, rgb * scale, center_lum > 1e-5);
        }

        if (u_params.noise_reduction_color > 0.001) {
            let cur_lum = rec2020_lum(rgb);
            let blur_lum = rec2020_lum(blur3);
            let chroma = rgb - vec3<f32>(cur_lum);
            let blur_chroma = blur3 - vec3<f32>(blur_lum);
            let smooth_chroma = mix(chroma, blur_chroma, clamp(u_params.noise_reduction_color * 0.9, 0.0, 0.95));
            rgb = max(vec3<f32>(cur_lum) + smooth_chroma, vec3<f32>(0.0));
        }
    }

    return vec4<f32>(rgb, center.a);
}
