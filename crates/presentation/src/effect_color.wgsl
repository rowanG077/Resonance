#define_import_path resonance::effect_color

fn quantize(color: vec3<f32>, pixel: vec2<f32>) -> vec3<f32> {
    // Particle tints use 64 as unity. Dither to six bits before blending;
    // pixel coordinates use the framebuffer's bottom-left origin.
    let rgb = vec3<u32>(round(clamp(color * (255. / 256.), vec3<f32>(0.), vec3<f32>(1.)) * 255.));
    let parity = vec2<u32>(pixel) & vec2<u32>(1u);
    let dither = (parity.x ^ parity.y) * 2u + parity.y;
    let quantized = min((rgb - (rgb >> vec3<u32>(6u)) + vec3<u32>(dither)) >> vec3<u32>(2u), vec3<u32>(63u));
    return vec3<f32>(quantized) / 63.;
}
