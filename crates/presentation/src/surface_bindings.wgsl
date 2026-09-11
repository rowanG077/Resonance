#define_import_path resonance::surface_bindings

struct SurfaceUniform {
    uv_offsets: vec4<f32>,
    uv_scales: vec4<f32>,
    tint: vec4<f32>,
    field_light: vec4<f32>,
    shade_colors: array<vec4<f32>, 2>,
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
#ifdef BINDLESS
    return textureSample(bindless_textures_2d[indices[slot].color], bindless_samplers_filtering[indices[slot].color_sampler], uv);
#else
    return textureSample(color_texture, color_sampler, uv);
#endif
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
