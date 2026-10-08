#define_import_path resonance::effect_color

fn sample_bytes(map: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
    // Filter on the atlas's 1/128-texel grid, retaining whole color bytes.
    let size = vec2<i32>(textureDimensions(map));
    let pixel = trunc(uv * vec2<f32>(size) * 128.) / 128. - vec2<f32>(0.5);
    let base = vec2<i32>(floor(pixel));
    let weight = fract(pixel);
    let a = round(textureLoad(map, clamp(base, vec2<i32>(0), size - 1), 0) * 255.);
    let b = round(textureLoad(map, clamp(base + vec2<i32>(1, 0), vec2<i32>(0), size - 1), 0) * 255.);
    let c = round(textureLoad(map, clamp(base + vec2<i32>(0, 1), vec2<i32>(0), size - 1), 0) * 255.);
    let d = round(textureLoad(map, clamp(base + vec2<i32>(1, 1), vec2<i32>(0), size - 1), 0) * 255.);
    return floor(mix(mix(a, b, weight.x), mix(c, d, weight.x), weight.y));
}

fn dithered_bytes(color: vec3<f32>, pixel: vec2<f32>) -> vec3<f32> {
    // Particle tints use 64 as unity. Keep byte precision through fog;
    // pixel coordinates use the render target's top-left origin.
    let rgb = vec3<u32>(round(clamp(color * (255. / 256.), vec3<f32>(0.), vec3<f32>(1.)) * 255.));
    let parity = vec2<u32>(pixel) & vec2<u32>(1u);
    let dither = (parity.x ^ parity.y) * 2u + parity.y;
    return vec3<f32>(rgb - (rgb >> vec3<u32>(6u)) + vec3<u32>(dither));
}

fn quantize(color: vec3<f32>, pixel: vec2<f32>) -> vec3<f32> {
    return floor(dithered_bytes(color, pixel) / 4.) / 63.;
}
