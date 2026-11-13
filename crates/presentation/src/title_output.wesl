import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var source: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var source_sampler: sampler;
struct OutputUniform {
    brightness: vec4<f32>,
    overlay_enabled: u32,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> settings: OutputUniform;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var overlay: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var overlay_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let brightness = settings.brightness;
    // Present without the original vertical deflicker/copy filter. The oracle
    // uses DisableCopyFilter=True; keep fades and color conversion intact.
    var encoded = vec3<f32>(0.0);
    if brightness.z <= 0.5 {
        let center = textureSample(source, source_sampler, in.uv).rgb;
        encoded = clamp(center * brightness.x + brightness.y, vec3(0.0), vec3(1.0));
    }
    if settings.overlay_enabled != 0u {
        let ui = textureSample(overlay, overlay_sampler, in.uv);
        encoded = ui.rgb + encoded * (1.0 - ui.a);
    }
    let linear = select(encoded / 12.92,
        pow((encoded + 0.055) / 1.055, vec3<f32>(2.4)), encoded > vec3<f32>(0.04045));
    return vec4<f32>(linear, 1.0);
}
