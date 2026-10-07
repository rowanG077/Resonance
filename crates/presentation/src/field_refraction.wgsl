#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import resonance::effect_color::quantize

struct Pulse { position_size: vec4<f32>, opacity: vec4<f32>, tint: vec4<f32>, basis_x: vec4<f32>, basis_y: vec4<f32> };
struct Settings {
    world_from_clip: mat4x4<f32>,
    clip_from_world: mat4x4<f32>,
    eye: vec4<f32>,
    uv: array<vec4<f32>, 2>,
    parameters: vec4<f32>,
    screen_copy: vec4<f32>,
    pulses: array<Pulse, 16>,
};
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var displacement: texture_2d<f32>;
@group(0) @binding(3) var<uniform> settings: Settings;
@group(0) @binding(4) var scene_depth: texture_depth_2d;
@group(0) @binding(5) var air_displacement: texture_2d<f32>;

fn scene_texel(pixel: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(scene));
    let rgba = textureLoad(scene, clamp(pixel, vec2<i32>(0), size - vec2<i32>(1)), 0);
    // Refraction captures are RGB565, quantized before bilinear filtering.
    let packed = vec3<u32>(round(rgba.rgb * 255.)) >> vec3<u32>(3u, 2u, 3u);
    let rgb = (packed << vec3<u32>(3u, 2u, 3u)) | (packed >> vec3<u32>(2u, 4u, 2u));
    return vec4<f32>(vec3<f32>(rgb) / 255., 1.);
}

fn captured_scene(uv: vec2<f32>) -> vec4<f32> {
    let pixel = uv * vec2<f32>(textureDimensions(scene)) - vec2<f32>(0.5);
    let base = vec2<i32>(floor(pixel));
    let weight = fract(pixel);
    return mix(mix(scene_texel(base), scene_texel(base + vec2<i32>(1, 0)), weight.x),
               mix(scene_texel(base + vec2<i32>(0, 1)), scene_texel(base + vec2<i32>(1, 1)), weight.x), weight.y);
}

fn displacement_sample(map: texture_2d<f32>, uv: vec2<f32>) -> vec2<f32> {
    // Byte filtering is explicit: host sampler rounding can change a displacement
    // by a whole pixel even when its color error would be invisible.
    let size = vec2<i32>(textureDimensions(map));
    let atlas_scale = vec2<f32>(size) * 128.;
    let atlas_uv = trunc(uv * atlas_scale) / atlas_scale;
    let pixel = atlas_uv * vec2<f32>(size) - vec2<f32>(0.5);
    let base = vec2<i32>(floor(pixel));
    let weight = fract(pixel);
    let a = textureLoad(map, clamp(base, vec2<i32>(0), size - 1), 0).ab;
    let b = textureLoad(map, clamp(base + vec2<i32>(1, 0), vec2<i32>(0), size - 1), 0).ab;
    let c = textureLoad(map, clamp(base + vec2<i32>(0, 1), vec2<i32>(0), size - 1), 0).ab;
    let d = textureLoad(map, clamp(base + vec2<i32>(1, 1), vec2<i32>(0), size - 1), 0).ab;
    return floor(mix(mix(round(a * 255.), round(b * 255.), weight.x),
                     mix(round(c * 255.), round(d * 255.), weight.x), weight.y));
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
    let projected = settings.world_from_clip * vec4<f32>(in.uv * vec2<f32>(2., -2.) + vec2<f32>(-1., 1.), 0.5, 1.);
    let ray = projected.xyz / projected.w - settings.eye.xyz;
    for (var i = 0u; i < u32(settings.parameters.x); i++) {
        let pulse = settings.pulses[i];
        let normal = cross(pulse.basis_x.xyz, pulse.basis_y.xyz);
        let denominator = dot(ray, normal);
        if abs(denominator) < 0.000001 || pulse.position_size.w <= 0. { continue; }
        let t = dot(pulse.position_size.xyz - settings.eye.xyz, normal) / denominator;
        let hit = settings.eye.xyz + ray * t;
        let clip = settings.clip_from_world * vec4<f32>(hit, 1.);
        // Reverse-Z: a world-space ripple must remain behind nearer scenery.
        let visible = clip.z / clip.w > depth;
        let delta = hit - pulse.position_size.xyz;
        let point = vec2<f32>(dot(delta, pulse.basis_x.xyz), dot(delta, pulse.basis_y.xyz)) / pulse.position_size.w;
        let uv = point * vec2<f32>(1., -1.) + vec2<f32>(0.5);
        if t > 0. && visible && all(uv >= vec2<f32>(0.)) && all(uv <= vec2<f32>(1.)) {
            let bounds = settings.uv[u32(pulse.opacity.y)];
            let atlas_uv = mix(bounds.xy, bounds.zw, uv);
            var sample: vec2<f32>;
            if pulse.opacity.y == 1. { sample = displacement_sample(air_displacement, atlas_uv); }
            else { sample = displacement_sample(displacement, atlas_uv); }
            let offset = (sample - vec2<f32>(128.)) * settings.parameters.yz;
            // Scene-copy coordinates include the camera's raster-center offset.
            let captured = captured_scene(in.uv + offset + settings.parameters.w / vec2<f32>(textureDimensions(scene)));
            let pixel = vec2<f32>(in.position.x, f32(textureDimensions(scene).y) - in.position.y);
            let refracted = vec4<f32>(quantize(captured.rgb * pulse.tint.rgb, pixel), captured.a);
            color = mix(color, refracted, pulse.opacity.x);
        }
    }
    return color;
}
