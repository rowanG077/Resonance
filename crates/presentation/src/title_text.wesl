import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var source: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var source_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> opacity_pulse: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(source, source_sampler, in.uv);
    // Combine the normal text and additive pulse in one draw. Premultiplied
    // blending preserves the normal draw's contribution from the background.
    return vec4<f32>(texel.rgb * texel.a * (opacity_pulse.x + opacity_pulse.y),
                     texel.a * opacity_pulse.x);
}
