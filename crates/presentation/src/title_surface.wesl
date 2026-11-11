import resonance_presentation::surface_vertex::VertexOutput;
import bevy_pbr::render::view_transformations::position_world_to_view;
import bevy_pbr::render::mesh_bindings::mesh;
import resonance_presentation::surface_bindings::{surface_data, sample_primary, sample_secondary, sample_toon, tev_input};
@if(CLAMP_COLOR)
import resonance_presentation::effect_color::dithered_bytes;
@if(DISTANCE_FOG)
import bevy_pbr::render::mesh_view_bindings::fog as view_fog;
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    @if(BINDLESS)
    let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    @else
    let slot = 0u;
    let material = surface_data(slot);
    let field_lighting = (material.flags & 8u) != 0u;
    let red_channel = (material.flags & 1u) != 0u;
    var tint = material.tint;
    @if(VERTEX_ALPHA && VERTEX_COLORS)
    if (in.channels & 8u) != 0u { tint.a = 1.0; }
    let uv_offsets = material.uv_offsets;
    let shades = material.shade_colors;
    var color = tint;
    @if(!CONSTANT_COLOR) {
        // Apply the channel swap to the unlit multiplier too; selector tint can be colored.
        tint = tev_input(tint, red_channel);
        var raster = vec4<f32>(1.0);
        @if(VERTEX_COLORS) {
            if (in.channels & 8u) != 0u {
                raster = in.color;
                @if(CLAMP_COLOR) {
                    let opacity = round(in.color.a * 255.);
                    raster.a = (opacity + floor(opacity / 128.)) / 256.;
                }
            }
        }
        @if(VERTEX_NORMALS)
        if !field_lighting && (in.channels & 1u) != 0u && material.scenery_lighting.ambient.w != 0.0 {
            raster = vec4<f32>(raster.rgb * in.world_normal, raster.a);
        }
        raster = tev_input(raster, red_channel);
        color = raster;
        @if(MULTIPLY_ALPHA_ONLY)
        let raster_alpha = color.a;
        var texture_color = vec4<f32>(1.0);
        @if(VERTEX_UVS_A) {
            let primary = tev_input(sample_primary(slot, in.uv * material.uv_scales.xy + uv_offsets.xy), red_channel);
            if (in.channels & 2u) != 0u {
                texture_color *= primary;
                color *= primary;
            }
        }
        @if(VERTEX_NORMALS)
        if !field_lighting && (in.channels & 1u) != 0u && material.scenery_lighting.ambient.w != 0.0 {
            // Scenery channel lighting doubles the first color stage before clamping.
            color = vec4<f32>(min(color.rgb * 2.0, vec3<f32>(1.0)), color.a);
        }
        @if(VERTEX_UVS_B) {
            // Gradients precede the channel branch so absent padded UVs cost
            // no texture fetch while present channels keep their mip selection.
            let secondary_uv = in.uv_b * material.uv_scales.zw + uv_offsets.zw;
            let secondary_dx = dpdx(secondary_uv);
            let secondary_dy = dpdy(secondary_uv);
            if (in.channels & 4u) != 0u {
                let secondary = tev_input(sample_secondary(slot, secondary_uv, secondary_dx, secondary_dy), red_channel);
                // Read particle alpha from the second palette independently of the color palette alpha.
                @if(MULTIPLY_ALPHA_ONLY) {
                    texture_color = vec4<f32>(texture_color.rgb, secondary.a);
                    color = vec4<f32>(color.rgb, raster_alpha * secondary.a);
                } @else {
                    texture_color *= secondary;
                    color *= secondary;
                }
            }
        }
        @if(VERTEX_NORMALS) {
            let toon_dx = dpdx(in.world_normal.xy);
            let toon_dy = dpdy(in.world_normal.xy);
            if field_lighting && (in.channels & 1u) != 0u {
                let weights = sample_toon(slot, in.world_normal.xy, toon_dx, toon_dy).rgb;
                let decoded_light = weights.r * shades[0].rgb + weights.g * shades[1].rgb + weights.b;
                let light = tev_input(vec4<f32>(decoded_light, 1.0), red_channel).rgb;
                // Round to bytes between the ambient and palette color products;
                // continuous float multiplication makes the characters slightly brighter.
                var ambient = vec3<f32>(255.0);
                // Apply channel lighting before swapping raster color channels.
                @if(VERTEX_COLORS) {
                    if (in.channels & 8u) != 0u {
                        let ambient_scale = tev_input(vec4<f32>(material.ambient_scale.rgb, 1.0), red_channel).rgb;
                        ambient = round(raster.rgb * 255.0 * ambient_scale);
                    }
                }
                // Custom opaque actors disable the first TEV gain; the later toon
                // palette stage still has its independent fourfold gain.
                let base_divisor = select(256.0, 64.0, material.ambient_scale.w != 0.0);
                let base = min(floor(round(texture_color.rgb * 255.0)
                    * (ambient + floor(ambient / 128.0)) / base_divisor + 0.5), vec3<f32>(255.0));
                let lit = min(floor(round(light * 255.0)
                    * (base + floor(base / 128.0)) / 64.0 + 0.5), vec3<f32>(255.0));
                color = vec4<f32>(lit / 255.0, color.a);
            }
        }
        // Model tints are rounded at the vertices before interpolation.
        @if(VERTEX_ALPHA)
        if !field_lighting { color = vec4<f32>(color.rgb * 4., color.a); }
        @if(!(VERTEX_ALPHA))
        if !field_lighting && material.ambient_color.w != 0.0 {
            color = vec4<f32>(min(color.rgb * material.ambient_color.rgb / 64.0,
                vec3<f32>(1.0)), color.a);
        }
        color *= tint;
    }

    // Filtered particle alpha is rounded before testing coverage.
    @if(CLAMP_COLOR)
    color.a = floor(color.a * 255. + 0.5) / 255.;
    if (material.flags & 2u) != 0u && in.world_position.y < 0. { discard; }
    if color.a < material.alpha_cutoff { discard; }
    // One-byte opacity covers geometry but contributes no blended color.
    @if(CLAMP_COLOR) {
        if color.a == 1. / 255. { color.a = 0.; }
        color = vec4<f32>(dithered_bytes(color.rgb, in.position.xy) / 255., color.a);
    }
    var fog_range = material.fog_range.xyz;
    var fog_color = material.fog_color.rgb;
    // W selects camera fog for field geometry and participating effects.
    // Overworld surfaces retain their own range and nonlinear exponent.
    @if(DISTANCE_FOG)
    if material.fog_range.w != 0.0 {
        fog_range = vec3<f32>(view_fog.be.xy, 2.0);
        fog_color = view_fog.base_color.rgb;
    }
    if fog_range.y != fog_range.x {
        // View depth avoids backend differences in fragment W and also works
        // with orthographic projections.
        let depth = -position_world_to_view(in.world_position.xyz).z;
        var fog = clamp((depth - fog_range.x)
            / (fog_range.y - fog_range.x), 0.0, 1.0);
        if fog_range.z > 0.0 {
            // GX exponential curves operate on the clamped depth fraction.
            fog = 1.0 - exp2(-8.0 * pow(fog, fog_range.z));
        }
        @if(CLAMP_COLOR)
        fog = round(fog * 256.) / 256.;
        color = vec4<f32>(mix(color.rgb, fog_color, fog), color.a);
    }
    // Store six-bit channels only after fog has been mixed into the color.
    @if(CLAMP_COLOR)
    color = vec4<f32>(floor(clamp(color.rgb, vec3<f32>(0.), vec3<f32>(1.)) * (255. / 4.)) / 63., color.a);
    return color;
}
