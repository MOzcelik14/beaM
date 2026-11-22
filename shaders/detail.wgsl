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

    if (u_params.enabled == 0u || (
        abs(u_params.dehaze) <= 0.001 &&
        abs(u_params.clarity) <= 0.001 &&
        u_params.sharpening <= 0.001
    )) {
        return center;
    }

    var rgb = max(center.rgb, vec3<f32>(0.0));

    // 1. Dehaze (local dark-channel veil removal)
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

    // 3x3 neighborhood only when Clarity or Sharpening is active
    var blur3 = rgb;
    if (abs(u_params.clarity) > 0.001 || u_params.sharpening > 0.001) {
        var acc = vec3<f32>(0.0);
        for (var dy: i32 = -1; dy <= 1; dy = dy + 1) {
            for (var dx: i32 = -1; dx <= 1; dx = dx + 1) {
                let s = load_clamped(tex_coords + vec2<i32>(dx, dy), max_coords);
                let k = select(select(1.0, 2.0, dx == 0 || dy == 0), 4.0, dx == 0 && dy == 0);
                acc = acc + s * k;
            }
        }
        blur3 = acc / 16.0;
    }

    // 2. Clarity (midtone local contrast using wider radius taps)
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

    // 3. Sharpening (luminance unsharp mask)
    if (u_params.sharpening > 0.001) {
        let cur_lum = rec2020_lum(rgb);
        let blur_lum = rec2020_lum(blur3);
        let sharp_delta = (cur_lum - blur_lum) * u_params.sharpening * 1.1;
        rgb = max(rgb + vec3<f32>(sharp_delta), vec3<f32>(0.0));
    }

    return vec4<f32>(rgb, center.a);
}
