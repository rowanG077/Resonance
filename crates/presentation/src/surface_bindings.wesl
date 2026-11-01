@if(CLAMP_COLOR && !VERTEX_ALPHA)
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
    @if(CLAMP_COLOR && !VERTEX_ALPHA && BINDLESS)
    return sample_bytes(bindless_textures_2d[indices[slot].color], uv) / 255.;
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
fn sample_secondary(slot: u32, uv: vec2<f32>) -> vec4<f32> {
    @if(BINDLESS)
    return textureSample(bindless_textures_2d[indices[slot].multiply], bindless_samplers_filtering[indices[slot].multiply_sampler], uv);
    @else
    return textureSample(multiply_texture, multiply_sampler, uv);
}
fn sample_toon(slot: u32, uv: vec2<f32>) -> vec4<f32> {
    @if(BINDLESS)
    return textureSample(bindless_textures_2d[indices[slot].toon], bindless_samplers_filtering[indices[slot].toon_sampler], uv);
    @else
    return textureSample(toon_texture, toon_sampler, uv);
}

// GXInit table1 is [R,R,R,A]. Call on decoded TEV inputs, never toon weights.
fn tev_input(color: vec4<f32>) -> vec4<f32> {
    @if(RED_CHANNEL)
    return color.rrra;
    @if(!(RED_CHANNEL))
    return color;
}
