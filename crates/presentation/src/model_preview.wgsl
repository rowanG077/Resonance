#import bevy_sprite::mesh2d_vertex_output::VertexOutput
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var source: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var source_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> opacity: f32;
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(source, source_sampler, in.uv) * opacity;
}
