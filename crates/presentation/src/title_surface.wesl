import bevy_pbr::render::forward_io::VertexOutput;
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
    var tint = material.tint;
    @if(VERTEX_ALPHA && VERTEX_COLORS)
    tint.a = 1.0;
    let uv_offsets = material.uv_offsets;
    let shades = material.shade_colors;
    var color = tint;
    @if(!CONSTANT_COLOR) {
        // Apply the channel swap to the unlit multiplier too; selector tint can be colored.
        tint = tev_input(tint);
        var raster = vec4<f32>(1.0);
        @if(VERTEX_COLORS) {
            raster = in.color;
            @if(CLAMP_COLOR) {
                let opacity = round(in.color.a * 255.);
                raster.a = (opacity + floor(opacity / 128.)) / 256.;
            }
        }
        @if(!FIELD_LIGHTING && VERTEX_NORMALS)
        if material.scenery_lighting.ambient.w != 0.0 {
            raster = vec4<f32>(raster.rgb * in.world_normal, raster.a);
        }
        raster = tev_input(raster);
        color = raster;
        @if(MULTIPLY_ALPHA_ONLY)
        let raster_alpha = color.a;
        var texture_color = vec4<f32>(1.0);
        @if(VERTEX_UVS_A) {
            let primary = tev_input(sample_primary(slot, in.uv * material.uv_scales.xy + uv_offsets.xy));
            texture_color *= primary;
            color *= primary;
        }
        @if(!FIELD_LIGHTING && VERTEX_NORMALS)
        if material.scenery_lighting.ambient.w != 0.0 {
            // Scenery channel lighting doubles the first color stage before clamping.
            color = vec4<f32>(min(color.rgb * 2.0, vec3<f32>(1.0)), color.a);
        }
        @if(VERTEX_UVS_B) {
            let secondary = tev_input(sample_secondary(slot, in.uv_b * material.uv_scales.zw + uv_offsets.zw));
            // Read particle alpha from the second palette independently of the color palette alpha.
            @if(MULTIPLY_ALPHA_ONLY) {
                texture_color = vec4<f32>(texture_color.rgb, secondary.a);
                color = vec4<f32>(color.rgb, raster_alpha * secondary.a);
            } @else {
                texture_color *= secondary;
                color *= secondary;
            }
        }
        @if(FIELD_LIGHTING && VERTEX_NORMALS) {
            let weights = sample_toon(slot, in.world_normal.xy).rgb;
            let decoded_light = weights.r * shades[0].rgb + weights.g * shades[1].rgb + weights.b;
            let light = tev_input(vec4<f32>(decoded_light, 1.0)).rgb;
            // Ambient and palette colors each have fourfold gain. Round to bytes
            // between the two color products;
            // continuous float multiplication makes the characters slightly brighter.
            var ambient = vec3<f32>(255.0);
            // Apply channel lighting before swapping raster color channels.
            @if(VERTEX_COLORS) {
                let ambient_scale = tev_input(vec4<f32>(material.ambient_scale.rgb, 1.0)).rgb;
                ambient = round(raster.rgb * 255.0 * ambient_scale);
            }
            let base = min(floor(round(texture_color.rgb * 255.0)
                * (ambient + floor(ambient / 128.0)) / 64.0 + 0.5), vec3<f32>(255.0));
            let lit = min(floor(round(light * 255.0)
                * (base + floor(base / 128.0)) / 64.0 + 0.5), vec3<f32>(255.0));
            color = vec4<f32>(lit / 255.0, color.a);
        }
        // Model tints are rounded at the vertices before interpolation.
        @if(!FIELD_LIGHTING && VERTEX_ALPHA)
        color = vec4<f32>(color.rgb * 4., color.a);
        @if(!FIELD_LIGHTING && !(VERTEX_ALPHA))
        if material.ambient_color.w != 0.0 {
            color = vec4<f32>(min(color.rgb * material.ambient_color.rgb / 64.0,
                vec3<f32>(1.0)), color.a);
        }
        color *= tint;
    }

    // Filtered particle alpha is rounded before testing coverage.
    @if(CLAMP_COLOR)
    color.a = floor(color.a * 255. + 0.5) / 255.;
    @if(CLIP_BELOW_FLOOR)
    if in.world_position.y < 0. { discard; }
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
        let depth = 1.0 / in.position.w;
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
