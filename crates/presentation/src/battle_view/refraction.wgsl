#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;

@fragment
fn capture(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // C49C(1,6)/4ADF8: linear half-size copy, followed by GX_RGB565
    // quantization. Expand the truncated five/six-bit channels by bit replication.
    let color = textureSampleLevel(scene, linear_sampler, in.uv, 0.).rgb;
    let bits = vec3<u32>(floor(clamp(color, vec3<f32>(0.), vec3<f32>(1.))
        * 255. / vec3<f32>(8., 4., 8.)));
    let rgb = vec3<u32>((bits.r << 3u) | (bits.r >> 2u),
        (bits.g << 2u) | (bits.g >> 4u), (bits.b << 3u) | (bits.b >> 2u));
    return vec4<f32>(vec3<f32>(rgb) / 255., 1.);
}
