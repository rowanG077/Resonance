#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import resonance::effect_color::{quantize, sample_bytes}

struct Pulse { center: vec4<f32>, right: vec4<f32>, up: vec4<f32>, opacity: vec4<f32>, tint: vec4<f32> };
struct Settings {
    clip_from_world: mat4x4<f32>,
    uv: array<vec4<f32>, 2>,
    parameters: vec4<f32>,
    screen_copy: vec4<f32>,
};
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var displacement: texture_2d<f32>;
@group(0) @binding(3) var<uniform> settings: Settings;
@group(0) @binding(4) var scene_depth: texture_depth_2d;
@group(0) @binding(5) var air_displacement: texture_2d<f32>;
@group(0) @binding(6) var<storage, read> pulses: array<Pulse>;

fn scene_texel(pixel: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(scene));
    let rgba = textureLoad(scene, clamp(pixel, vec2<i32>(0), size - vec2<i32>(1)), 0);
    // Refraction captures are RGB565, quantized before bilinear filtering.
    let packed = vec3<u32>(round(rgba.rgb * 255.)) >> vec3<u32>(3u, 2u, 3u);
    let rgb = (packed << vec3<u32>(3u, 2u, 3u)) | (packed >> vec3<u32>(2u, 4u, 2u));
    return vec4<f32>(vec3<f32>(rgb) / 255., 1.);
}

fn captured_scene(uv: vec2<f32>) -> vec4<f32> {
    let grid = vec2<f32>(textureDimensions(scene)) * 128.;
    let pixel = trunc(uv * grid) / 128. - vec2<f32>(0.5);
    let base = vec2<i32>(floor(pixel));
    let weight = fract(pixel);
    let filtered = mix(mix(scene_texel(base), scene_texel(base + vec2<i32>(1, 0)), weight.x),
                       mix(scene_texel(base + vec2<i32>(0, 1)), scene_texel(base + vec2<i32>(1, 1)), weight.x), weight.y);
    return floor(filtered * 255.) / 255.;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    var color = textureSampleLevel(scene, linear_sampler, in.uv, 0.);
    let depth = textureLoad(scene_depth, vec2<i32>(in.position.xy), 0);
    // Enlarged copies with alpha 0x90. Pass B uses LEQUAL,
    // pass A GEQUAL; the scene depth buffer uses the reverse convention.
    if settings.screen_copy.y != 0. && (1. - settings.screen_copy.y / 200.) * (1. - settings.screen_copy.z) + settings.screen_copy.z >= depth {
        let uv = (in.uv * vec2<f32>(640., 480.) + vec2<f32>(3.)) / vec2<f32>(646., 486.);
        color = mix(color, textureSampleLevel(scene, linear_sampler, uv, 0.), 144. / 255.);
    }
    if settings.screen_copy.x != 0. && (1. - settings.screen_copy.x / 200.) * (1. - settings.screen_copy.z) + settings.screen_copy.z <= depth {
        let uv = (in.uv * vec2<f32>(640., 480.) + vec2<f32>(4.)) / vec2<f32>(648., 488.);
        color = mix(color, textureSampleLevel(scene, linear_sampler, uv, 0.), 144. / 255.);
    }
    return color;
}

struct RippleVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) pulse: u32,
};

@vertex
fn quad_vertex(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> RippleVertex {
    let pulse = pulses[instance];
    if pulse.opacity.x <= 0. {
        return RippleVertex(vec4<f32>(0., 0., 0., 1.), vec2<f32>(0.), instance);
    }
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, 0.5), vec2<f32>(0.5, -0.5), vec2<f32>(0.5, 0.5),
        vec2<f32>(-0.5, 0.5), vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, -0.5));
    let point = corners[vertex];
    return RippleVertex(pulse.center + point.x * pulse.right + point.y * pulse.up,
                        point * vec2<f32>(1., -1.) + vec2<f32>(0.5), instance);
}

@fragment
fn quad_fragment(in: RippleVertex) -> @location(0) vec4<f32> {
    let pulse = pulses[in.pulse];
    // Reverse-Z: ripples remain behind nearer scenery.
    if in.position.z <= textureLoad(scene_depth, vec2<i32>(in.position.xy), 0) { discard; }
    let bounds = settings.uv[u32(pulse.opacity.y)];
    let atlas_uv = mix(bounds.xy, bounds.zw, in.uv);
    var sample: vec2<f32>;
    if pulse.opacity.y == 1. { sample = sample_bytes(air_displacement, atlas_uv).ab; }
    else { sample = sample_bytes(displacement, atlas_uv).ab; }
    let offset = (sample - vec2<f32>(128.)) * settings.parameters.yz;
    // Scene-copy coordinates include the camera's raster-center offset.
    let screen = (in.position.xy + settings.parameters.w) / vec2<f32>(textureDimensions(scene));
    let captured = captured_scene(screen + offset);
    return vec4<f32>(quantize(captured.rgb * pulse.tint.rgb, in.position.xy), pulse.opacity.x);
}
