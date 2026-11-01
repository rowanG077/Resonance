import bevy_pbr::render::forward_io::VertexOutput;
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var reflection: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var reflection_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> tint: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<uniform> dimensions: vec2<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.position.xy / dimensions;
    let reflected = textureSample(reflection, reflection_sampler, vec2<f32>(1. - uv.x, uv.y));
    return vec4<f32>(min(reflected.rgb * tint.rgb, vec3<f32>(1.)), tint.a);
}
