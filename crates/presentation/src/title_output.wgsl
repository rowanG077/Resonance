#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var source: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var source_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> brightness: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    if brightness.z > 0.5 { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }
    // Present without the original vertical deflicker/copy filter. The oracle
    // uses DisableCopyFilter=True; keep fades and color conversion intact.
    let center = textureSample(source, source_sampler, in.uv).rgb;
    let encoded = clamp(center * brightness.x + brightness.y, vec3(0.0), vec3(1.0));
    let linear = select(encoded / 12.92,
        pow((encoded + 0.055) / 1.055, vec3<f32>(2.4)), encoded > vec3<f32>(0.04045));
    return vec4<f32>(linear, 1.0);
}
