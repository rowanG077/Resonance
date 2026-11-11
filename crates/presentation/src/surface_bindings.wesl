@if(CLAMP_COLOR && !VERTEX_ALPHA && !BINDLESS)
import resonance_presentation::effect_color::sample_bytes;
@if(BINDLESS)
import bevy_render::bindless::{bindless_textures_2d, bindless_samplers_filtering};
struct SceneryLighting {
    ambient: vec4<f32>,
    color: vec4<f32>,
    attenuation: vec4<f32>,
    positions: array<vec4<f32>, 2>,
};

struct SurfaceUniform {
    uv_offsets: vec4<f32>,
    uv_scales: vec4<f32>,
    tint: vec4<f32>,
    ambient_color: vec4<f32>,
    field_light: vec4<f32>,
    shade_colors: array<vec4<f32>, 2>,
    scenery_lighting: SceneryLighting,
    fog_color: vec4<f32>,
    fog_range: vec4<f32>, // XYZ: material start/end/exponent; W: use field view fog.
    alpha_cutoff: f32,
    flags: u32, // Bits 0/1: red-channel/floor clip; bits 2/3: vertex color/toon lighting.
    ambient_scale: vec4<f32>,
};


@if(BINDLESS) {
    struct SurfaceIndices {
        color: u32,
        color_sampler: u32,
        multiply: u32,
        multiply_sampler: u32,
        data: u32,
        unused: u32,
        toon: u32,
        toon_sampler: u32,
    };
    @group(constants::MATERIAL_BIND_GROUP) @binding(0) var<storage> indices: array<SurfaceIndices>;
    @group(constants::MATERIAL_BIND_GROUP) @binding(10) var<storage> materials: array<SurfaceUniform>;
} @else {
    @group(constants::MATERIAL_BIND_GROUP) @binding(0) var color_texture: texture_2d<f32>;
    @group(constants::MATERIAL_BIND_GROUP) @binding(1) var color_sampler: sampler;
    @group(constants::MATERIAL_BIND_GROUP) @binding(2) var multiply_texture: texture_2d<f32>;
    @group(constants::MATERIAL_BIND_GROUP) @binding(3) var multiply_sampler: sampler;
    @group(constants::MATERIAL_BIND_GROUP) @binding(4) var<uniform> material: SurfaceUniform;
    @group(constants::MATERIAL_BIND_GROUP) @binding(6) var toon_texture: texture_2d<f32>;
    @group(constants::MATERIAL_BIND_GROUP) @binding(7) var toon_sampler: sampler;
}

fn surface_data(slot: u32) -> SurfaceUniform {
    @if(BINDLESS)
    return materials[indices[slot].data];
    @else
    return material;
}
fn sample_primary(slot: u32, uv: vec2<f32>) -> vec4<f32> {
    @if(CLAMP_COLOR && !VERTEX_ALPHA && BINDLESS) {
        // Naga 30 emits an invalid SPIR-V function argument when a binding-array
        // image is passed to sample_bytes. Keep these texture accesses local;
        // the byte filtering matches effect_color::sample_bytes exactly.
        let map = bindless_textures_2d[indices[slot].color];
        let size = vec2<i32>(textureDimensions(map));
        let pixel = trunc(uv * vec2<f32>(size) * 128.) / 128. - vec2<f32>(0.5);
        let base = vec2<i32>(floor(pixel));
        let weight = fract(pixel);
        let a = round(textureLoad(map, clamp(base, vec2<i32>(0), size - 1), 0) * 255.);
        let b = round(textureLoad(map, clamp(base + vec2<i32>(1, 0), vec2<i32>(0), size - 1), 0) * 255.);
        let c = round(textureLoad(map, clamp(base + vec2<i32>(0, 1), vec2<i32>(0), size - 1), 0) * 255.);
        let d = round(textureLoad(map, clamp(base + vec2<i32>(1, 1), vec2<i32>(0), size - 1), 0) * 255.);
        return floor(mix(mix(a, b, weight.x), mix(c, d, weight.x), weight.y)) / 255.;
    }
    @if(CLAMP_COLOR && !VERTEX_ALPHA && !(BINDLESS))
    return sample_bytes(color_texture, uv) / 255.;
    var coords = uv;
    // Effect textures use a 1/128-texel grid and whole color bytes.
    // Keep the material's own filtering and wrap modes.
    @if(CLAMP_COLOR) {
        @if(BINDLESS)
        let grid = vec2<f32>(textureDimensions(bindless_textures_2d[indices[slot].color])) * 128.;
        @else
        let grid = vec2<f32>(textureDimensions(color_texture)) * 128.;
        coords = trunc(coords * grid) / grid;
    }
    @if(BINDLESS)
    var color = textureSample(bindless_textures_2d[indices[slot].color], bindless_samplers_filtering[indices[slot].color_sampler], coords);
    @else
    var color = textureSample(color_texture, color_sampler, coords);
    @if(CLAMP_COLOR)
    color = floor(color * 255.) / 255.;
    return color;
}
fn sample_secondary(slot: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    @if(BINDLESS)
    return textureSampleGrad(bindless_textures_2d[indices[slot].multiply], bindless_samplers_filtering[indices[slot].multiply_sampler], uv, dx, dy);
    @else
    return textureSampleGrad(multiply_texture, multiply_sampler, uv, dx, dy);
}
fn sample_toon(slot: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    @if(BINDLESS)
    return textureSampleGrad(bindless_textures_2d[indices[slot].toon], bindless_samplers_filtering[indices[slot].toon_sampler], uv, dx, dy);
    @else
    return textureSampleGrad(toon_texture, toon_sampler, uv, dx, dy);
}

// GXInit table1 is [R,R,R,A]. Call on decoded TEV inputs, never toon weights.
fn tev_input(color: vec4<f32>, red_channel: bool) -> vec4<f32> {
    return select(color, color.rrra, red_channel);
}
