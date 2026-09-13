#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
@group(0) @binding(0) var scene: texture_2d<f32>;
@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let linear = max(textureLoad(scene, vec2<i32>(in.position.xy), 0).rgb, vec3(0.0));
    let encoded = select(linear * 12.92,
        1.055 * pow(linear, vec3(1.0 / 2.4)) - 0.055, linear > vec3(0.0031308));
    return vec4(encoded, 1.0);
}
