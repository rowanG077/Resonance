#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_bindings::mesh
#import resonance::surface_bindings::{surface_data, sample_primary, sample_secondary, sample_toon}

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
    return tint;
#else
    var color = vec4<f32>(1.0);
#ifdef VERTEX_COLORS
    color *= in.color;
#endif
#ifdef SCREEN_TEXTURE
    // 46DCC: stage 1 replaces stage 0 RGB with the captured scene, but
    // multiplies its alpha by raster alpha a second time (4ACB8/4AB74).
    let scene = sample_primary(slot, in.uv_b);
    let mask = sample_secondary(slot, in.uv);
    let screened = vec4<f32>(scene.rgb * color.rgb, mask.a * color.a * color.a) * tint;
    if screened.a < 1.0/255.0 { discard; }
    return screened;
#else
#ifdef MULTIPLY_ALPHA_ONLY
    let raster_alpha = color.a;
#endif
    var texture_color = vec4<f32>(1.0);
#ifdef VERTEX_UVS_A
    let primary = sample_primary(slot, in.uv * material.uv_scales.xy + uv_offsets.xy);
    texture_color *= primary;
    color *= primary;
#endif
#ifdef VERTEX_UVS_B
    let secondary = sample_secondary(slot, in.uv_b * material.uv_scales.zw + uv_offsets.zw);
#ifdef MULTIPLY_ALPHA_ONLY
    // The original two-stage particle TEV takes alpha from its second palette,
    // independently of the color palette's alpha (4AC18, 4AB74 and 47FA4).
    texture_color = vec4<f32>(texture_color.rgb, secondary.a);
    color = vec4<f32>(color.rgb, raster_alpha * secondary.a);
#else
    texture_color *= secondary;
    color *= secondary;
#endif
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
    ambient = round(in.color.rgb * 255.0 * material.ambient_scale.rgb);
#endif
    let base = min(floor(round(texture_color.rgb * 255.0)
        * (ambient + floor(ambient / 128.0)) / 64.0 + 0.5), vec3<f32>(255.0));
    let lit = min(floor(round(light * 255.0)
        * (base + floor(base / 128.0)) / 64.0 + 0.5), vec3<f32>(255.0));
    color = vec4<f32>(lit / 255.0, color.a);
#endif
#endif
    return color * tint;
#endif
#endif
}
