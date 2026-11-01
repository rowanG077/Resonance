import resonance_presentation::effect_depth::depth_coverage;
struct Settings { clip_from_world: mat4x4<f32> };
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(3) var<uniform> settings: Settings;

struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};
@vertex
fn vertex(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> Vertex {
    return Vertex(settings.clip_from_world * vec4<f32>(position, 1.), uv, color);
}
@fragment
fn fragment(in: Vertex) -> @location(0) vec4<f32> {
    // The post-processing color target is single-sampled. Read visibility
    // from every scene depth sample instead of attaching that MSAA texture.
    let coverage = depth_coverage(vec2<i32>(in.position.xy), in.position.z, 0u);
    if coverage == 0. { discard; }
    let color = textureSampleLevel(scene, linear_sampler, in.uv, 0.) * in.color;
    return vec4<f32>(color.rgb, color.a * coverage);
}
