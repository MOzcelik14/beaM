// 8-Band HSL adjustment pass for beaM.
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

// 8 bands: [Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta]
// Each vec4 is (hue_shift, sat_factor, lum_factor, _unused)
struct HslUniforms {
    bands: array<vec4<f32>, 8>,
    enabled: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_params: HslUniforms;

fn rgb_to_hsl(c: vec3<f32>) -> vec3<f32> {
    let c_max = max(c.r, max(c.g, c.b));
    let c_min = min(c.r, min(c.g, c.b));
    let delta = c_max - c_min;

    let l = (c_max + c_min) * 0.5;

    if (delta <= 1e-5) {
        return vec3<f32>(0.0, 0.0, l);
    }

    var s: f32 = 0.0;
    if (l < 0.5) {
        s = delta / (c_max + c_min);
    } else {
        s = delta / (2.0 - c_max - c_min);
    }

    var h: f32 = 0.0;
    if (c_max == c.r) {
        h = ((c.g - c.b) / delta) % 6.0;
    } else if (c_max == c.g) {
        h = ((c.b - c.r) / delta) + 2.0;
    } else {
        h = ((c.r - c.g) / delta) + 4.0;
    }
    h = h * 60.0;
    if (h < 0.0) {
        h = h + 360.0;
    }

    return vec3<f32>(h, s, l);
}

fn hue_to_rgb(p: f32, q: f32, t_in: f32) -> f32 {
    var t = t_in;
    if (t < 0.0) { t = t + 1.0; }
    if (t > 1.0) { t = t - 1.0; }
    if (t < 1.0 / 6.0) { return p + (q - p) * 6.0 * t; }
    if (t < 1.0 / 2.0) { return q; }
    if (t < 2.0 / 3.0) { return p + (q - p) * (2.0 / 3.0 - t) * 6.0; }
    return p;
}

fn hsl_to_rgb(hsl: vec3<f32>) -> vec3<f32> {
    let h = (hsl.x % 360.0) / 360.0;
    let s = clamp(hsl.y, 0.0, 1.0);
    let l = clamp(hsl.z, 0.0, 1.0);

    if (s <= 1e-5) {
        return vec3<f32>(l);
    }

    var q: f32 = 0.0;
    if (l < 0.5) {
        q = l * (1.0 + s);
    } else {
        q = l + s - l * s;
    }
    let p = 2.0 * l - q;

    return vec3<f32>(
        hue_to_rgb(p, q, h + 1.0 / 3.0),
        hue_to_rgb(p, q, h),
        hue_to_rgb(p, q, h - 1.0 / 3.0)
    );
}

fn angular_distance(a: f32, b: f32) -> f32 {
    let diff = abs(a - b) % 360.0;
    if (diff > 180.0) {
        return 360.0 - diff;
    }
    return diff;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_input));
    let tex_coords = vec2<i32>(clamp(in.uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    let color = textureLoad(t_input, tex_coords, 0);

    if (u_params.enabled == 0u) {
        return color;
    }

    let hsl = rgb_to_hsl(color.rgb);
    if (hsl.y <= 1e-4) {
        return color;
    }

    // 8 band center angles in degrees
    var centers = array<f32, 8>(0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 280.0, 320.0);
    var bandwidths = array<f32, 8>(30.0, 25.0, 35.0, 45.0, 35.0, 35.0, 30.0, 30.0);

    var total_weight: f32 = 0.0;
    var d_hue: f32 = 0.0;
    var d_sat: f32 = 0.0;
    var d_lum: f32 = 0.0;

    for (var i = 0u; i < 8u; i = i + 1u) {
        let dist = angular_distance(hsl.x, centers[i]);
        let bw = bandwidths[i];
        if (dist < bw) {
            let w = (1.0 - (dist / bw)) * (1.0 - (dist / bw));
            total_weight = total_weight + w;
            d_hue = d_hue + w * (u_params.bands[i].x * 0.3); // max +-30 degrees
            d_sat = d_sat + w * (u_params.bands[i].y / 100.0);
            d_lum = d_lum + w * (u_params.bands[i].z / 100.0);
        }
    }

    if (total_weight <= 1e-4) {
        return color;
    }

    d_hue = d_hue / total_weight;
    d_sat = d_sat / total_weight;
    d_lum = d_lum / total_weight;

    var new_h = (hsl.x + d_hue + 360.0) % 360.0;
    var new_s = clamp(hsl.y * (1.0 + d_sat), 0.0, 1.0);
    var new_l = clamp(hsl.z * (1.0 + d_lum), 0.0, 1.0);

    let new_rgb = hsl_to_rgb(vec3<f32>(new_h, new_s, new_l));
    return vec4<f32>(new_rgb, color.a);
}
