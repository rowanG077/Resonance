#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct Pulse { position_size: vec4<f32>, opacity: vec4<f32> };
struct Settings {
    world_from_clip: mat4x4<f32>,
    clip_from_world: mat4x4<f32>,
    eye: vec4<f32>,
    uv: vec4<f32>,
    parameters: vec4<f32>,
    screen_copy: vec4<f32>,
    pulses: array<Pulse, 16>,
};
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var displacement: texture_2d<f32>;
@group(0) @binding(3) var<uniform> settings: Settings;
@group(0) @binding(4) var scene_depth: texture_depth_2d;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let original = textureSampleLevel(scene, linear_sampler, in.uv, 0.);
    var color = original;
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
    let projected = settings.world_from_clip * vec4<f32>(in.uv * vec2<f32>(2., -2.) + vec2<f32>(-1., 1.), 0.5, 1.);
    let ray = projected.xyz / projected.w - settings.eye.xyz;
    for (var i = 0u; i < u32(settings.parameters.x); i++) {
        let pulse = settings.pulses[i];
        let t = (pulse.position_size.z - settings.eye.z) / ray.z;
        let hit = settings.eye.xy + ray.xy * t;
        let clip = settings.clip_from_world * vec4<f32>(hit, pulse.position_size.z, 1.);
        // Reverse-Z: a world-space ripple must remain behind nearer scenery.
        let visible = clip.z / clip.w > textureLoad(scene_depth, vec2<i32>(in.position.xy), 0);
        let point = (hit - pulse.position_size.xy) / pulse.position_size.w;
        let uv = point * vec2<f32>(1., -1.) + vec2<f32>(0.5);
        if t > 0. && visible && all(uv >= vec2<f32>(0.)) && all(uv <= vec2<f32>(1.)) {
            let atlas_uv = mix(settings.uv.xy, settings.uv.zw, uv);
            let offset = (textureSampleLevel(displacement, linear_sampler, atlas_uv, 0.).ab * 255. - vec2<f32>(128.))
                * settings.parameters.yz;
            let refracted = textureSampleLevel(scene, linear_sampler, in.uv + offset, 0.);
            color = mix(color, refracted, pulse.opacity.x);
        }
    }
    return color;
}
