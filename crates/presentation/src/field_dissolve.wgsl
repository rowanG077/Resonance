#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
struct Settings { opacity: vec4<f32> };
@group(0) @binding(0) var captured: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var<uniform> settings: Settings;
@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let uv = (in.uv - vec2<f32>(0.5)) * settings.opacity.zw + vec2<f32>(0.5);
    return vec4<f32>(textureSampleLevel(captured, linear_sampler, uv, 0.).rgb, settings.opacity.x);
}
