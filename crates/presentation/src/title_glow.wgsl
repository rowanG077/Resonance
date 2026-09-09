#import bevy_pbr::forward_io::VertexOutput
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var atlas: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var atlas_sampler: sampler;
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(atlas, atlas_sampler, in.uv) * in.color;
    // Glow artwork uses fourfold RGB gain before additive blending.
    return vec4<f32>(min(color.rgb * 4.0, vec3<f32>(1.0)), color.a);
}
