#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_bindings::mesh
#import resonance::surface_bindings::{surface_data, sample_primary, sample_secondary, sample_toon}

fn output_color(color: vec4<f32>) -> vec4<f32> {
#ifdef LINEAR_OUTPUT
    // Solari composites the retained toon actors/effects into a linear HDR view.
    let linear = select(color.rgb / 12.92,
        pow(max((color.rgb + 0.055) / 1.055, vec3(0.0)), vec3(2.4)), color.rgb > vec3(0.04045));
    return vec4(linear, color.a);
#else
    return color;
#endif
}

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
    return output_color(tint);
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
    return output_color(color * tint);
#endif
}
