@if(CLAMP_COLOR && !VERTEX_ALPHA)
import resonance_presentation::effect_color::sample_bytes;
struct SceneryLighting {
    ambient: vec4<f32>,
    color: vec4<f32>,
    attenuation: vec4<f32>,
    positions: array<vec4<f32>, 2>,
};

struct SurfaceUniform {
    uv_offsets: vec4<f32>,
    @if(WORLD_SURFACE)
    uv_motion: vec4<f32>,
    uv_scales: vec4<f32>,
    tint: vec4<f32>,
    @if(WORLD_SURFACE)
    tint_motion: vec4<f32>,
    ambient_color: vec4<f32>,
    @if(!WORLD_SURFACE)
    field_light: vec4<f32>,
    @if(!WORLD_SURFACE)
    shade_colors: array<vec4<f32>, 2>,
    @if(!WORLD_SURFACE)
    scenery_lighting: SceneryLighting,
    fog_color: vec4<f32>,
    fog_range: vec4<f32>, // XYZ: material range/curve; W: 1 field view, 2 world view.
    alpha_cutoff: f32,
    flags: u32, // Bits 0/1: red-channel/floor clip; 2/3: vertex color/toon; 4: mesh-tag fraction.
    @if(!WORLD_SURFACE)
    ambient_scale: vec4<f32>, // RGB: actor scale; W: enable first TEV fourfold gain.
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var color_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var color_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var multiply_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var multiply_sampler: sampler;
// The native frame binds both record tables once per frame; draws select a row.
@if(!WORLD_SURFACE)
@group(0) @binding(4) var<storage, read> native_title_states: array<SurfaceUniform>;
@if(WORLD_SURFACE)
@group(0) @binding(5) var<storage, read> native_world_states: array<SurfaceUniform>;
@if(!WORLD_SURFACE)
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var toon_texture: texture_2d<f32>;
@if(!WORLD_SURFACE)
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var toon_sampler: sampler;

fn surface_data(slot: u32) -> SurfaceUniform {
    @if(WORLD_SURFACE)
    return native_world_states[slot];
    @else
    return native_title_states[slot];
}
fn sample_primary(slot: u32, uv: vec2<f32>) -> vec4<f32> {
    @if(CLAMP_COLOR && !VERTEX_ALPHA)
    return sample_bytes(color_texture, uv) / 255.;
    var coords = uv;
    // Effect textures use a 1/128-texel grid and whole color bytes.
    // Keep the material's own filtering and wrap modes.
    @if(CLAMP_COLOR) {
        let grid = vec2<f32>(textureDimensions(color_texture)) * 128.;
        coords = trunc(coords * grid) / grid;
    }
    var color = textureSample(color_texture, color_sampler, coords);
    @if(CLAMP_COLOR)
    color = floor(color * 255.) / 255.;
    return color;
}
fn sample_secondary(slot: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    return textureSampleGrad(multiply_texture, multiply_sampler, uv, dx, dy);
}
@if(!WORLD_SURFACE)
fn sample_toon(slot: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    return textureSampleGrad(toon_texture, toon_sampler, uv, dx, dy);
}

// GXInit table1 is [R,R,R,A]. Call on decoded TEV inputs, never toon weights.
fn tev_input(color: vec4<f32>, red_channel: bool) -> vec4<f32> {
    return select(color, color.rrra, red_channel);
}
