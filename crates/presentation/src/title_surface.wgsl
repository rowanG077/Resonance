#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_bindings::mesh
#import resonance::surface_bindings::{surface_data, sample_primary, sample_secondary, sample_toon}
#ifdef CLAMP_COLOR
#import bevy_pbr::mesh_view_bindings::view
#import resonance::effect_color::quantize
#endif
#ifdef DISTANCE_FOG
#import bevy_pbr::mesh_view_bindings::fog as view_fog
#endif

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef BINDLESS
    let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
#else
    let slot = 0u;
#endif
    let material = surface_data(slot);
    let tint = material.tint;
    let uv_offsets = material.uv_offsets;
    let shades = material.shade_colors;
#ifdef CONSTANT_COLOR
    var color = tint;
#else
    var color = vec4<f32>(1.0);
#ifdef VERTEX_COLORS
    color *= in.color;
#endif
    var texture_color = vec4<f32>(1.0);
#ifdef VERTEX_UVS_A
    let primary = sample_primary(slot, in.uv * material.uv_scales.xy + uv_offsets.xy);
    texture_color *= primary;
    color *= primary;
#endif
#ifdef VERTEX_UVS_B
    let secondary = sample_secondary(slot, in.uv_b * material.uv_scales.zw + uv_offsets.zw);
    texture_color *= secondary;
    color *= secondary;
#endif
#ifdef CLAMP_COLOR
    // Filtered particle alpha is rounded before testing coverage.
    color.a = round(color.a * 255.) / 255.;
#endif
    // Preserve transparent holes in both the color and focus depth layers.
    // Keep every nonzero eight-bit alpha value.
    if color.a < 1.0/255.0 { discard; }
#ifdef FIELD_LIGHTING
#ifdef VERTEX_NORMALS
    let weights = sample_toon(slot, in.world_normal.xy).rgb;
    let light = weights.r * shades[0].rgb + weights.g * shades[1].rgb + weights.b;
    // Ambient and palette colors each have fourfold gain. Round to bytes
    // between the two color products;
    // continuous float multiplication makes the characters slightly brighter.
    var ambient = vec3<f32>(255.0);
#ifdef VERTEX_COLORS
    ambient = round(in.color.rgb * 255.0);
#endif
    let base = min(floor(round(texture_color.rgb * 255.0)
        * (ambient + floor(ambient / 128.0)) / 64.0 + 0.5), vec3<f32>(255.0));
    let lit = min(floor(round(light * 255.0)
        * (base + floor(base / 128.0)) / 64.0 + 0.5), vec3<f32>(255.0));
    color = vec4<f32>(lit / 255.0, color.a);
#endif
#endif
#ifndef FIELD_LIGHTING
    if material.ambient_color.w != 0.0 {
        color = vec4<f32>(min(color.rgb * material.ambient_color.rgb / 64.0,
            vec3<f32>(1.0)), color.a);
    }
#endif
    color *= tint;
#endif
#ifdef CLAMP_COLOR
    color = vec4<f32>(quantize(color.rgb, vec2<f32>(in.position.x, view.viewport.w - in.position.y)), color.a);
#endif
    var fog_range = material.fog_range.xyz;
    var fog_color = material.fog_color.rgb;
#ifdef DISTANCE_FOG
    // W selects camera fog for field geometry and participating effects.
    // Overworld surfaces retain their own range and nonlinear exponent.
    if material.fog_range.w != 0.0 {
        fog_range = vec3<f32>(view_fog.be.xy, 2.0);
        fog_color = view_fog.base_color.rgb;
    }
#endif
    if fog_range.y != fog_range.x {
        let depth = 1.0 / in.position.w;
        var fog = clamp((depth - fog_range.x)
            / (fog_range.y - fog_range.x), 0.0, 1.0);
        if fog_range.z > 0.0 {
            // GX exponential curves operate on the clamped depth fraction.
            fog = 1.0 - exp2(-8.0 * pow(fog, fog_range.z));
        }
        color = vec4<f32>(mix(color.rgb, fog_color, fog), color.a);
    }
    return color;
}
