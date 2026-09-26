#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
@group(0) @binding(0) var field: texture_2d<f32>;
@group(0) @binding(1) var field_sampler: sampler;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // The field owner copies its completed 640x448 EFB to texture5 in RGB565.
    let color = textureSampleLevel(field, field_sampler, in.uv, 0.).rgb;
    let bits = vec3<u32>(floor(clamp(color, vec3<f32>(0.), vec3<f32>(1.))
        * 255. / vec3<f32>(8., 4., 8.)));
    let rgb = vec3<u32>((bits.r << 3u) | (bits.r >> 2u),
        (bits.g << 2u) | (bits.g >> 4u), (bits.b << 3u) | (bits.b >> 2u));
    return vec4<f32>(vec3<f32>(rgb) / 255., 1.);
}
