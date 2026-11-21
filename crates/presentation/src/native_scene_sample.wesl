// Mesh draws that sample the completed scene, using the native frame's geometry.
import resonance_presentation::native_frame::{draws, get_world_from_local, position_world_to_clip};
import resonance_presentation::effect_color::{sample_bytes, quantize};
import resonance_presentation::effect_depth::depth_coverage;
@group(1) @binding(0) var mask: texture_2d<f32>;
@group(2) @binding(0) var scene: texture_2d<f32>;
@group(2) @binding(1) var linear_sampler: sampler;
struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) screen_uv: vec2<f32>,
};
@vertex
fn vertex(@builtin(instance_index) index: u32, @location(0) position: vec3<f32>,
    @if(MASKED_SCENE) @location(1) offset: vec3<f32>,
    @location(2) uv: vec2<f32>, @if(VERTEX_COLORS) @location(5) color: vec4<f32>) -> Vertex {
    let draw = draws[index];
    let world = get_world_from_local(index) * vec4<f32>(position, 1.);
    let clip = position_world_to_clip(world.xyz);
    var tint = draw.params;
    @if(VERTEX_COLORS)
    if draw.ids.x != 0u { tint *= color; }
    let screen = clip.xy / clip.w * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    @if(MASKED_SCENE)
    return Vertex(clip, uv, tint, screen + offset.xy / vec2<f32>(640., 448.));
    @else
    return Vertex(clip, uv, tint, screen);
}
fn scene_texel(pixel: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(320., 224.);
    let uv = (clamp(pixel, vec2<f32>(0.), size - vec2<f32>(1.)) + vec2<f32>(0.5)) / size;
    let rgba = textureSampleLevel(scene, linear_sampler, uv, 0.);
    // The scene copy is half resolution, with RGB565 precision before filtering.
    let packed = vec3<u32>(round(rgba.rgb * 255.)) >> vec3<u32>(3u, 2u, 3u);
    return vec3<f32>((packed << vec3<u32>(3u, 2u, 3u)) | (packed >> vec3<u32>(2u, 4u, 2u)));
}
fn captured_scene(uv: vec2<f32>) -> vec3<f32> {
    let pixel = trunc(uv * vec2<f32>(320., 224.) * 128.) / 128. - vec2<f32>(0.5);
    let base = floor(pixel);
    let weight = fract(pixel);
    return floor(mix(mix(scene_texel(base), scene_texel(base + vec2<f32>(1., 0.)), weight.x),
                     mix(scene_texel(base + vec2<f32>(0., 1.)), scene_texel(base + vec2<f32>(1.)), weight.x), weight.y)) / 255.;
}
@fragment
fn fragment(in: Vertex) -> @location(0) vec4<f32> {
    var coverage = 1.;
    @if(DEPTH_TEST) {
        @if(MASKED_SCENE)
        coverage = depth_coverage(vec2<i32>(in.position.xy), in.position.z, 1u);
        @else
        coverage = depth_coverage(vec2<i32>(in.position.xy), in.position.z, 0u);
        if coverage == 0. { discard; }
    }
    @if(MASKED_SCENE) {
        let opacity = round(in.color.a * 255.);
        let multiplier = (opacity + floor(opacity / 128.)) / 256.;
        let first = floor(sample_bytes(mask, in.uv).a * multiplier + 0.5);
        let alpha = floor(first * multiplier + 0.5) / 255.;
        if alpha <= 1. / 255. { discard; }
        let color = captured_scene(in.screen_uv) * in.color.rgb * 2.;
        return vec4<f32>(quantize(color, in.position.xy), alpha * coverage);
    }
    @else {
        let color = textureSampleLevel(scene, linear_sampler, in.uv, 0.) * in.color;
        return vec4<f32>(color.rgb, color.a * coverage);
    }
}
