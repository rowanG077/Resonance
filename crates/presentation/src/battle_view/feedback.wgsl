#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct Settings { parameters: vec4<f32> };
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var previous: texture_2d<f32>;
@group(0) @binding(3) var<uniform> settings: Settings;

@fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let amount = settings.parameters.x;
    // 1878 submits [-amount,-amount,640+2*amount,480+2*amount].
    // The logical 640x480 rectangle covers the original 640x448 framebuffer.
    let canvas = vec2<f32>(640., 480.);
    let uv = (in.uv * canvas + vec2<f32>(amount)) / (canvas + vec2<f32>(2. * amount));
    let current = textureSampleLevel(scene, linear_sampler, in.uv, 0.);
    let retained = textureSampleLevel(previous, linear_sampler, uv, 0.);
    return vec4<f32>(mix(current.rgb, retained.rgb, retained.a * settings.parameters.y), 1.);
}

@fragment
fn capture(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // GXCopyTex's half-size RGBA8 copy averages each 2x2 group. A linear sample
    // at the center of each output texel reproduces that at native resolution.
    return vec4<f32>(textureSampleLevel(scene, linear_sampler, in.uv, 0.).rgb, 1.);
}
