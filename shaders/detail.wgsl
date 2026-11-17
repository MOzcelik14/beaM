// Detail pass (Clarity, Dehaze, Sharpening, Luma & Color Noise Reduction) for beaM.
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

    if (u_params.enabled == 0u) {
        return center;
    }

    var rgb = max(center.rgb, vec3<f32>(0.0));

    // 0. 5x5 Edge-Preserving Local AI/Bilateral Denoise
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

    // 3x3 neighborhood for Sharpening and Noise Reduction
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

    // 1. Noise Reduction (Luma + Color)
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

    // 2. Dehaze (local dark-channel veil removal)
    if (abs(u_params.dehaze) > 0.001) {
        var dark_min = min(rgb.r, min(rgb.g, rgb.b));
        // Sample 4 diagonal taps at radius 3 for dark channel estimation
        let d1 = load_clamped(tex_coords + vec2<i32>(-3, -3), max_coords);
        let d2 = load_clamped(tex_coords + vec2<i32>(3, -3), max_coords);
        let d3 = load_clamped(tex_coords + vec2<i32>(-3, 3), max_coords);
        let d4 = load_clamped(tex_coords + vec2<i32>(3, 3), max_coords);
        dark_min = min(dark_min, min(min(d1.r, min(d1.g, d1.b)), min(d2.r, min(d2.g, d2.b))));
        dark_min = min(dark_min, min(min(d3.r, min(d3.g, d3.b)), min(d4.r, min(d4.g, d4.b))));

        let strength = clamp(u_params.dehaze, -1.0, 1.0);
        if (strength > 0.0) {
            let t = clamp(1.0 - strength * 0.75 * dark_min, 0.25, 1.0);
            rgb = max((rgb - vec3<f32>(dark_min * strength * 0.45)) / t, vec3<f32>(0.0));
        } else {
            // Negative dehaze adds atmospheric haze
            let haze = -strength * 0.35;
            rgb = mix(rgb, vec3<f32>(0.65), haze);
        }
    }

    // 3. Clarity (midtone local contrast using wider radius taps)
    if (abs(u_params.clarity) > 0.001) {
        let w1 = load_clamped(tex_coords + vec2<i32>(-4, 0), max_coords);
        let w2 = load_clamped(tex_coords + vec2<i32>(4, 0), max_coords);
        let w3 = load_clamped(tex_coords + vec2<i32>(0, -4), max_coords);
        let w4 = load_clamped(tex_coords + vec2<i32>(0, 4), max_coords);
        let wide_blur = (blur3 * 2.0 + w1 + w2 + w3 + w4) / 6.0;

        let lum = clamp(rec2020_lum(rgb), 0.0, 1.0);
        // Bell curve peaking in midtones so shadows/highlights stay protected
        let mid_mask = 4.0 * lum * (1.0 - lum);
        let detail = rgb - wide_blur;
        rgb = max(rgb + detail * u_params.clarity * (0.35 + 0.65 * mid_mask), vec3<f32>(0.0));
    }

    // 4. Sharpening (luminance unsharp mask)
    if (u_params.sharpening > 0.001) {
        let cur_lum = rec2020_lum(rgb);
        let blur_lum = rec2020_lum(blur3);
        let sharp_delta = (cur_lum - blur_lum) * u_params.sharpening * 1.1;
        rgb = max(rgb + vec3<f32>(sharp_delta), vec3<f32>(0.0));
    }

    return vec4<f32>(rgb, center.a);
}
