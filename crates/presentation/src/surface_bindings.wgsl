#define_import_path resonance::surface_bindings
#ifdef CLAMP_COLOR
#ifndef VERTEX_ALPHA
#import resonance::effect_color::sample_bytes
#endif
#endif

struct SurfaceUniform {
    uv_offsets: vec4<f32>,
    uv_scales: vec4<f32>,
    tint: vec4<f32>,
    ambient_color: vec4<f32>,
    field_light: vec4<f32>,
    shade_colors: array<vec4<f32>, 2>,
    fog_color: vec4<f32>,
    fog_range: vec4<f32>, // XYZ: material start/end/exponent; W: use field view fog.
    alpha_cutoff: f32,
};

#ifdef BINDLESS
#import bevy_render::bindless::{bindless_textures_2d, bindless_samplers_filtering}
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
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<storage> indices: array<SurfaceIndices>;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var<storage> materials: array<SurfaceUniform>;
#else
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var color_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var color_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var multiply_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var multiply_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<uniform> material: SurfaceUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var toon_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var toon_sampler: sampler;
#endif

fn surface_data(slot: u32) -> SurfaceUniform {
#ifdef BINDLESS
    return materials[indices[slot].data];
#else
    return material;
#endif
}
fn sample_primary(slot: u32, uv: vec2<f32>) -> vec4<f32> {
#ifdef CLAMP_COLOR
#ifndef VERTEX_ALPHA
#ifdef BINDLESS
    return sample_bytes(bindless_textures_2d[indices[slot].color], uv) / 255.;
#else
    return sample_bytes(color_texture, uv) / 255.;
#endif
#endif
#endif
    var coords = uv;
#ifdef CLAMP_COLOR
    // Effect textures use a 1/128-texel grid and whole color bytes.
    // Keep the material's own filtering and wrap modes.
#ifdef BINDLESS
    let grid = vec2<f32>(textureDimensions(bindless_textures_2d[indices[slot].color])) * 128.;
#else
    let grid = vec2<f32>(textureDimensions(color_texture)) * 128.;
#endif
    coords = trunc(coords * grid) / grid;
#endif
#ifdef BINDLESS
    var color = textureSample(bindless_textures_2d[indices[slot].color], bindless_samplers_filtering[indices[slot].color_sampler], coords);
#else
    var color = textureSample(color_texture, color_sampler, coords);
#endif
#ifdef CLAMP_COLOR
    color = floor(color * 255.) / 255.;
#endif
    return color;
}
fn sample_secondary(slot: u32, uv: vec2<f32>) -> vec4<f32> {
#ifdef BINDLESS
    return textureSample(bindless_textures_2d[indices[slot].multiply], bindless_samplers_filtering[indices[slot].multiply_sampler], uv);
#else
    return textureSample(multiply_texture, multiply_sampler, uv);
#endif
}
fn sample_toon(slot: u32, uv: vec2<f32>) -> vec4<f32> {
#ifdef BINDLESS
    return textureSample(bindless_textures_2d[indices[slot].toon], bindless_samplers_filtering[indices[slot].toon_sampler], uv);
#else
    return textureSample(toon_texture, toon_sampler, uv);
#endif
}
