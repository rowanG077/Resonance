#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import resonance::effect_color::{sample_bytes, quantize}

struct Settings { clip_from_world: mat4x4<f32>, dimensions: vec4<f32> };
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var mask: texture_2d<f32>;
@group(0) @binding(3) var<uniform> settings: Settings;

@fragment
fn copy(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return textureSampleLevel(scene, linear_sampler, in.uv, 0.);
}

struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) screen_uv: vec2<f32>,
};
@vertex
fn vertex(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>, @location(3) offset: vec2<f32>) -> Vertex {
    let clip = settings.clip_from_world * vec4<f32>(position, 1.);
    let screen = clip.xy / clip.w * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    return Vertex(clip, uv, color, screen + offset / settings.dimensions.xy);
}

fn scene_texel(pixel: vec2<f32>) -> vec3<f32> {
    let size = settings.dimensions.zw;
    let uv = (clamp(pixel, vec2<f32>(0.), size - vec2<f32>(1.)) + vec2<f32>(0.5)) / size;
    let rgba = textureSampleLevel(scene, linear_sampler, uv, 0.);
    // The scene copy is half resolution, with RGB565 precision before filtering.
    let packed = vec3<u32>(round(rgba.rgb * 255.)) >> vec3<u32>(3u, 2u, 3u);
    return vec3<f32>((packed << vec3<u32>(3u, 2u, 3u)) | (packed >> vec3<u32>(2u, 4u, 2u)));
}
fn captured_scene(uv: vec2<f32>) -> vec3<f32> {
    let pixel = trunc(uv * settings.dimensions.zw * 128.) / 128. - vec2<f32>(0.5);
    let base = floor(pixel);
    let weight = fract(pixel);
    return floor(mix(mix(scene_texel(base), scene_texel(base + vec2<f32>(1., 0.)), weight.x),
                     mix(scene_texel(base + vec2<f32>(0., 1.)), scene_texel(base + vec2<f32>(1.)), weight.x), weight.y)) / 255.;
}
@fragment
fn fragment(in: Vertex) -> @location(0) vec4<f32> {
    let opacity = round(in.color.a * 255.);
    let multiplier = (opacity + floor(opacity / 128.)) / 256.;
    let first = floor(sample_bytes(mask, in.uv).a * multiplier + 0.5);
    let alpha = floor(first * multiplier + 0.5) / 255.;
    if alpha <= 1. / 255. { discard; }
    let color = captured_scene(in.screen_uv) * in.color.rgb * 2.;
    return vec4<f32>(quantize(color, in.position.xy), alpha);
}
