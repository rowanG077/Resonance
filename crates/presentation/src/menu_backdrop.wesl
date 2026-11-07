import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var source: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var source_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> enabled: u32;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<uniform> cover: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = mix(textureSample(source, source_sampler, in.uv).rgb, cover.rgb, cover.a);
    var alpha = f32(enabled);
    @if(VERTEX_COLORS)
    alpha *= in.color.a;
    return vec4(color, alpha);
}
